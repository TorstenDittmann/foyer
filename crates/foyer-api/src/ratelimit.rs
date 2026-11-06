//! Per-IP rate limits for sign-in, uploads and downloads.
//!
//! Each bucket is a GCRA limiter (a token bucket without a timer): a client
//! may send `burst` requests at once and then `per_minute` on average.
//! State lives in memory, so every API replica limits on its own.
//!
//! The client's IP comes from, in order:
//! 1. `X-Foyer-Client-IP`, when the request also carries the shared
//!    `X-Foyer-Internal-Key` (`FOYER_INTERNAL_KEY`). The website sends these,
//!    since it calls the API from its server on behalf of visitors.
//! 2. `X-Forwarded-For`, counting `FOYER_PROXY_HOPS` entries from the right
//!    (the ones added by our own reverse proxies).
//! 3. The TCP peer address.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::AppError;

pub const CLIENT_IP_HEADER: &str = "x-foyer-client-ip";
pub const INTERNAL_KEY_HEADER: &str = "x-foyer-internal-key";

/// Entries kept before stale ones are swept.
const SWEEP_AT: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    /// Register, log in, password resets, email verification, Discord.
    Auth,
    /// Creating and finalizing builds, upload URLs, image uploads.
    Upload,
    /// Chunk URLs, manifests, zips and update checks.
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub per_minute: u32,
    pub burst: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimits {
    pub auth: Limit,
    pub upload: Limit,
    pub download: Limit,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            auth: Limit {
                per_minute: 10,
                burst: 10,
            },
            upload: Limit {
                per_minute: 120,
                burst: 60,
            },
            download: Limit {
                per_minute: 300,
                burst: 120,
            },
        }
    }
}

impl RateLimits {
    fn get(&self, bucket: Bucket) -> Limit {
        match bucket {
            Bucket::Auth => self.auth,
            Bucket::Upload => self.upload,
            Bucket::Download => self.download,
        }
    }
}

/// How the API finds a client's address.
#[derive(Debug, Clone, Default)]
pub struct ClientIpConfig {
    /// `FOYER_PROXY_HOPS`: reverse proxies in front of the API.
    pub proxy_hops: usize,
    /// `FOYER_INTERNAL_KEY`: lets the website name the visitor's address.
    pub internal_key: Option<String>,
}

pub struct RateLimiter {
    /// `None` turns limiting off.
    limits: Option<RateLimits>,
    client_ip: ClientIpConfig,
    /// Theoretical arrival time per bucket and client.
    state: Mutex<HashMap<(Bucket, IpAddr), Instant>>,
}

impl RateLimiter {
    pub fn new(limits: Option<RateLimits>, client_ip: ClientIpConfig) -> Self {
        Self {
            limits,
            client_ip,
            state: Mutex::default(),
        }
    }

    /// `Err(retry_after)` when the client is over the limit.
    pub fn check(&self, bucket: Bucket, ip: IpAddr) -> Result<(), Duration> {
        let Some(limits) = &self.limits else {
            return Ok(());
        };
        self.check_at(limits.get(bucket), bucket, ip, Instant::now())
    }

    fn check_at(
        &self,
        limit: Limit,
        bucket: Bucket,
        ip: IpAddr,
        now: Instant,
    ) -> Result<(), Duration> {
        let interval = Duration::from_secs(60) / limit.per_minute.max(1);
        let tolerance = interval * limit.burst.max(1).saturating_sub(1);
        let mut state = self.state.lock().expect("rate limiter lock");
        if state.len() >= SWEEP_AT {
            state.retain(|_, tat| *tat > now);
        }
        let tat = state.get(&(bucket, ip)).copied().unwrap_or(now).max(now);
        let ahead = tat - now;
        if ahead > tolerance {
            return Err(ahead - tolerance);
        }
        state.insert((bucket, ip), tat + interval);
        Ok(())
    }

    /// The address to limit a request by.
    pub fn client_ip(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> IpAddr {
        let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        if let Some(key) = &self.client_ip.internal_key
            && header(INTERNAL_KEY_HEADER).is_some_and(|sent| constant_time_eq(sent, key))
            && let Some(ip) = header(CLIENT_IP_HEADER).and_then(|v| v.trim().parse().ok())
        {
            return ip;
        }
        let hops = self.client_ip.proxy_hops;
        if hops > 0
            && let Some(forwarded) = header("x-forwarded-for")
        {
            let entries: Vec<&str> = forwarded.split(',').map(str::trim).collect();
            let index = entries.len().saturating_sub(hops);
            if let Some(ip) = entries.get(index).and_then(|e| e.parse().ok()) {
                return ip;
            }
        }
        peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Middleware for one bucket. The limiter comes from the request's
/// extensions (see `routes::router`); without one, nothing is limited.
pub async fn limit(bucket: Bucket, request: Request, next: Next) -> Response {
    if let Some(limiter) = request.extensions().get::<Arc<RateLimiter>>() {
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        let ip = limiter.client_ip(request.headers(), peer);
        if let Err(retry_after) = limiter.check(bucket, ip) {
            return AppError::RateLimited(retry_after).into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn allows_a_burst_then_the_rate() {
        let limiter = RateLimiter::new(None, ClientIpConfig::default());
        let limit = Limit {
            per_minute: 60,
            burst: 3,
        };
        let now = Instant::now();
        let a = ip("10.0.0.1");
        for _ in 0..3 {
            assert!(limiter.check_at(limit, Bucket::Auth, a, now).is_ok());
        }
        let wait = limiter.check_at(limit, Bucket::Auth, a, now).unwrap_err();
        assert_eq!(wait, Duration::from_secs(1));
        // Other clients and buckets are separate.
        assert!(
            limiter
                .check_at(limit, Bucket::Auth, ip("10.0.0.2"), now)
                .is_ok()
        );
        assert!(limiter.check_at(limit, Bucket::Download, a, now).is_ok());
        // One request per second refills.
        let later = now + Duration::from_secs(1);
        assert!(limiter.check_at(limit, Bucket::Auth, a, later).is_ok());
        assert!(limiter.check_at(limit, Bucket::Auth, a, later).is_err());
    }

    #[test]
    fn finds_the_client_ip() {
        let peer = Some(ip("192.168.1.10"));
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "6.6.6.6, 1.2.3.4".parse().unwrap());
        headers.insert(CLIENT_IP_HEADER, "5.5.5.5".parse().unwrap());

        // No proxies configured: the peer, headers ignored.
        let direct = RateLimiter::new(None, ClientIpConfig::default());
        assert_eq!(direct.client_ip(&headers, peer), ip("192.168.1.10"));

        // One proxy: the rightmost forwarded entry; spoofed entries further
        // left are ignored.
        let proxied = RateLimiter::new(
            None,
            ClientIpConfig {
                proxy_hops: 1,
                internal_key: Some("secret".into()),
            },
        );
        assert_eq!(proxied.client_ip(&headers, peer), ip("1.2.3.4"));

        // The website's header counts only with the right key.
        headers.insert(INTERNAL_KEY_HEADER, "wrong".parse().unwrap());
        assert_eq!(proxied.client_ip(&headers, peer), ip("1.2.3.4"));
        headers.insert(INTERNAL_KEY_HEADER, "secret".parse().unwrap());
        assert_eq!(proxied.client_ip(&headers, peer), ip("5.5.5.5"));
    }
}
