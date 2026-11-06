use std::env;

use anyhow::Context;

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
        })
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
