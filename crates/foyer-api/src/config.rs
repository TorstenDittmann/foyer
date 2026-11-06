use std::env;

/// Runtime configuration, read from `FOYER_*` environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address to listen on. `FOYER_BIND`, default `127.0.0.1:8080`.
    pub bind: String,
    pub database: DatabaseConfig,
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
        Ok(Self { bind, database })
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
