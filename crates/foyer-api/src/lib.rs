//! Foyer HTTP API.
//!
//! The binary in `main.rs` only wires configuration and the listener; all
//! routes live here so they can be exercised in tests without a socket.

use std::sync::Arc;

mod analytics;
pub mod auth;
mod clamd;
pub mod config;
pub mod db;
pub mod error;
pub mod jobs;
pub mod mail;
mod model;
mod openapi;
pub mod ratelimit;
pub mod releases;
mod routes;
pub mod storage;
mod validate;
pub mod worker;

pub use config::{Config, DiscordConfig};
pub use openapi::openapi;
pub use routes::router;

use crate::error::{AppError, AppResult};
use crate::ratelimit::{ClientIpConfig, RateLimiter, RateLimits};
use crate::storage::Storage;

/// Shared state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<libsql::Database>,
    pub storage: Option<Arc<Storage>>,
    /// Lowercase emails that become admins on registration.
    pub admin_emails: Arc<Vec<String>>,
    pub rate_limiter: Arc<RateLimiter>,
    /// The website's public URL, for links in emails.
    pub public_web_url: Arc<url::Url>,
    pub discord: Option<Arc<DiscordConfig>>,
    /// Talks to Discord.
    pub http: reqwest::Client,
    /// Passkeys, for the website at `public_web_url`. `None` when that URL
    /// has no domain name (WebAuthn doesn't work on IP addresses).
    pub webauthn: Option<Arc<webauthn_rs::Webauthn>>,
}

impl AppState {
    pub fn new(db: libsql::Database) -> Self {
        let web_url: url::Url = "http://localhost:3000".parse().expect("valid URL");
        Self {
            db: Arc::new(db),
            storage: None,
            admin_emails: Arc::default(),
            rate_limiter: Arc::new(RateLimiter::new(
                Some(RateLimits::default()),
                ClientIpConfig::default(),
            )),
            webauthn: webauthn_for(&web_url).map(Arc::new),
            public_web_url: Arc::new(web_url),
            discord: None,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("HTTP client"),
        }
    }

    /// Sets the website's address, which is also what passkeys are bound
    /// to: its host is the WebAuthn relying party id, its origin the only
    /// origin allowed.
    pub fn with_public_web_url(mut self, url: url::Url) -> Self {
        self.webauthn = webauthn_for(&url).map(Arc::new);
        if self.webauthn.is_none() {
            tracing::warn!(%url, "passkeys are off: the public web URL needs a domain name");
        }
        self.public_web_url = Arc::new(url);
        self
    }

    pub fn with_discord(mut self, discord: DiscordConfig) -> Self {
        self.discord = Some(Arc::new(discord));
        self
    }

    /// A link to a page of the website, e.g. `web_link("/verify-email", &[("token", t)])`.
    pub fn web_link(&self, path: &str, query: &[(&str, &str)]) -> String {
        let mut url = (*self.public_web_url).clone();
        url.set_path(&format!(
            "{}{path}",
            self.public_web_url.path().trim_end_matches('/')
        ));
        if query.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(query);
        }
        url.to_string()
    }

    /// Replaces the rate limits; `None` turns limiting off.
    pub fn with_rate_limits(
        mut self,
        limits: Option<RateLimits>,
        client_ip: ClientIpConfig,
    ) -> Self {
        self.rate_limiter = Arc::new(RateLimiter::new(limits, client_ip));
        self
    }

    /// State for the API and the worker, from the configuration (everything
    /// but storage, which needs its own setup).
    pub fn from_config(db: libsql::Database, config: &Config) -> anyhow::Result<Self> {
        let mut state = Self::new(db)
            .with_admin_emails(config.admin_emails.clone())
            .with_rate_limits(config.rate_limits, config.client_ip.clone())
            .with_public_web_url(config.public_web_url.clone());
        if let Some(discord) = &config.discord {
            state = state.with_discord(discord.clone());
        }
        Ok(state)
    }

    pub fn with_storage(mut self, storage: Storage) -> Self {
        self.storage = Some(Arc::new(storage));
        self
    }

    pub fn with_admin_emails(mut self, emails: Vec<String>) -> Self {
        self.admin_emails = Arc::new(emails.into_iter().map(|e| e.to_lowercase()).collect());
        self
    }

    pub(crate) async fn conn(&self) -> AppResult<libsql::Connection> {
        Ok(db::conn(&self.db).await?)
    }

    pub(crate) fn storage(&self) -> AppResult<&Storage> {
        self.storage.as_deref().ok_or(AppError::StorageUnavailable)
    }

    pub(crate) fn webauthn(&self) -> AppResult<&webauthn_rs::Webauthn> {
        self.webauthn.as_deref().ok_or_else(|| {
            AppError::coded(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "passkeys_unavailable",
                "passkeys are not set up on this server",
            )
        })
    }
}

fn webauthn_for(url: &url::Url) -> Option<webauthn_rs::Webauthn> {
    let rp_id = url.domain()?;
    let origin: url::Url = url.origin().ascii_serialization().parse().ok()?;
    webauthn_rs::WebauthnBuilder::new(rp_id, &origin)
        .ok()?
        .rp_name("Foyer")
        .build()
        .ok()
}
