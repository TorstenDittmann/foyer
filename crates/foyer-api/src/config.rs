use std::env;

use anyhow::Context;

use crate::ratelimit::{ClientIpConfig, RateLimits};

/// Runtime configuration, read from `FOYER_*` environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address to listen on. `FOYER_BIND`, default `127.0.0.1:8080`.
    pub bind: String,
    pub database: DatabaseConfig,
    /// Object storage for builds and media. `None` when `FOYER_S3_BUCKET` is
    /// unset; storage endpoints then answer 503.
    pub storage: Option<StorageConfig>,
    /// Users who register with one of these emails become admins.
    /// `FOYER_ADMIN_EMAILS`, comma-separated.
    pub admin_emails: Vec<String>,
    /// clamd's `host:port` for the worker's malware scan.
    /// `FOYER_CLAMD_ADDR`; without it builds are marked `skipped`.
    pub clamd_addr: Option<String>,
    /// Per-IP limits; `None` when `FOYER_RATE_LIMITS=off`.
    pub rate_limits: Option<RateLimits>,
    /// `FOYER_PROXY_HOPS` and `FOYER_INTERNAL_KEY`, for finding client IPs.
    pub client_ip: ClientIpConfig,
    /// The website's public URL, for links in emails and the Discord
    /// redirect. `FOYER_PUBLIC_WEB_URL`, default `http://localhost:3000`.
    pub public_web_url: url::Url,
    /// `FOYER_SMTP_URL`, e.g. `smtps://user:pass@smtp.example.com`. The worker
    /// sends email with it; without it emails are only logged.
    pub smtp_url: Option<String>,
    /// `FOYER_MAIL_FROM`, e.g. `Foyer <hello@foyer.lol>`.
    pub mail_from: String,
    /// Discord sign-in; `None` unless `FOYER_DISCORD_CLIENT_ID` and
    /// `FOYER_DISCORD_CLIENT_SECRET` are set.
    pub discord: Option<DiscordConfig>,
}

#[derive(Debug, Clone)]
pub struct DiscordConfig {
    pub client_id: String,
    pub client_secret: String,
    /// `FOYER_DISCORD_API_URL`, default `https://discord.com/api`. Tests
    /// point it at a fake.
    pub api_url: url::Url,
    /// `FOYER_DISCORD_AUTHORIZE_URL`, default
    /// `https://discord.com/oauth2/authorize`.
    pub authorize_url: url::Url,
}

/// Any S3-compatible object store (R2, B2, MinIO, AWS).
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// `FOYER_S3_ENDPOINT`, e.g. `https://<account>.r2.cloudflarestorage.com`.
    pub endpoint: url::Url,
    /// `FOYER_S3_BUCKET`.
    pub bucket: String,
    /// `FOYER_S3_REGION`, default `auto`.
    pub region: String,
    /// `FOYER_S3_ACCESS_KEY_ID`.
    pub access_key_id: String,
    /// `FOYER_S3_SECRET_ACCESS_KEY`.
    pub secret_access_key: String,
    /// `FOYER_S3_VIRTUAL_HOSTED=true` for `https://<bucket>.<endpoint>` URLs.
    /// Path-style is the default because every S3-compatible store supports it.
    pub virtual_hosted: bool,
    /// `FOYER_MEDIA_BASE_URL`: public URL that serves the bucket (a CDN or
    /// the bucket's public domain), used for game images. Defaults to the
    /// bucket's own URL.
    pub media_base_url: Option<url::Url>,
}

#[derive(Debug, Clone)]
pub enum DatabaseConfig {
    /// A local SQLite file, or `:memory:`.
    Local { path: String },
    /// A remote libSQL server (sqld / Turso).
    Remote { url: String, auth_token: String },
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = env::var("FOYER_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
        let url = env::var("FOYER_DATABASE_URL").unwrap_or_else(|_| "foyer.db".into());
        let database = DatabaseConfig::from_url(url, env::var("FOYER_DATABASE_AUTH_TOKEN").ok());
        let admin_emails = env::var("FOYER_ADMIN_EMAILS")
            .unwrap_or_default()
            .split(',')
            .map(|e| e.trim().to_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        Ok(Self {
            bind,
            database,
            storage: StorageConfig::from_env()?,
            admin_emails,
            clamd_addr: non_empty("FOYER_CLAMD_ADDR"),
            rate_limits: match non_empty("FOYER_RATE_LIMITS").as_deref() {
                Some("off") => None,
                None | Some("on") => Some(RateLimits::default()),
                Some(other) => anyhow::bail!("FOYER_RATE_LIMITS must be on or off, not {other:?}"),
            },
            client_ip: ClientIpConfig {
                proxy_hops: non_empty("FOYER_PROXY_HOPS")
                    .map(|v| v.parse())
                    .transpose()
                    .context("FOYER_PROXY_HOPS must be a number")?
                    .unwrap_or(0),
                internal_key: non_empty("FOYER_INTERNAL_KEY"),
            },
            public_web_url: non_empty("FOYER_PUBLIC_WEB_URL")
                .unwrap_or_else(|| "http://localhost:3000".into())
                .parse()
                .context("FOYER_PUBLIC_WEB_URL is not a valid URL")?,
            smtp_url: non_empty("FOYER_SMTP_URL"),
            mail_from: non_empty("FOYER_MAIL_FROM")
                .unwrap_or_else(|| "Foyer <noreply@foyer.lol>".into()),
            discord: DiscordConfig::from_env()?,
        })
    }
}

fn non_empty(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl DiscordConfig {
    fn from_env() -> anyhow::Result<Option<Self>> {
        let (Some(client_id), Some(client_secret)) = (
            non_empty("FOYER_DISCORD_CLIENT_ID"),
            non_empty("FOYER_DISCORD_CLIENT_SECRET"),
        ) else {
            return Ok(None);
        };
        Ok(Some(Self {
            client_id,
            client_secret,
            api_url: non_empty("FOYER_DISCORD_API_URL")
                .unwrap_or_else(|| "https://discord.com/api".into())
                .parse()
                .context("FOYER_DISCORD_API_URL is not a valid URL")?,
            authorize_url: non_empty("FOYER_DISCORD_AUTHORIZE_URL")
                .unwrap_or_else(|| "https://discord.com/oauth2/authorize".into())
                .parse()
                .context("FOYER_DISCORD_AUTHORIZE_URL is not a valid URL")?,
        }))
    }
}

impl StorageConfig {
    fn from_env() -> anyhow::Result<Option<Self>> {
        let Ok(bucket) = env::var("FOYER_S3_BUCKET") else {
            return Ok(None);
        };
        let required = |name: &str| {
            env::var(name)
                .with_context(|| format!("{name} is required when FOYER_S3_BUCKET is set"))
        };
        let endpoint = required("FOYER_S3_ENDPOINT")?
            .parse()
            .context("FOYER_S3_ENDPOINT is not a valid URL")?;
        let media_base_url = env::var("FOYER_MEDIA_BASE_URL")
            .ok()
            .map(|u| u.parse().context("FOYER_MEDIA_BASE_URL is not a valid URL"))
            .transpose()?;
        Ok(Some(Self {
            endpoint,
            bucket,
            region: env::var("FOYER_S3_REGION").unwrap_or_else(|_| "auto".into()),
            access_key_id: required("FOYER_S3_ACCESS_KEY_ID")?,
            secret_access_key: required("FOYER_S3_SECRET_ACCESS_KEY")?,
            virtual_hosted: env::var("FOYER_S3_VIRTUAL_HOSTED").is_ok_and(|v| v == "true"),
            media_base_url,
        }))
    }
}

impl DatabaseConfig {
    /// `libsql://`, `http://` and `https://` URLs are remote; anything else is
    /// treated as a local file path.
    pub fn from_url(url: String, auth_token: Option<String>) -> Self {
        let remote = ["libsql://", "http://", "https://"]
            .iter()
            .any(|scheme| url.starts_with(scheme));
        if remote {
            Self::Remote {
                url,
                auth_token: auth_token.unwrap_or_default(),
            }
        } else {
            Self::Local { path: url }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_backend_from_url() {
        assert!(matches!(
            DatabaseConfig::from_url("foyer.db".into(), None),
            DatabaseConfig::Local { .. }
        ));
        assert!(matches!(
            DatabaseConfig::from_url("libsql://foyer.turso.io".into(), Some("t".into())),
            DatabaseConfig::Remote { .. }
        ));
    }
}
