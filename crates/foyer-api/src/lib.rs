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
mod mail;
mod model;
mod openapi;
mod routes;
pub mod storage;
mod validate;
pub mod worker;

pub use config::Config;
pub use openapi::openapi;
pub use routes::router;

use crate::error::{AppError, AppResult};
use crate::storage::Storage;

/// Shared state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<libsql::Database>,
    pub storage: Option<Arc<Storage>>,
    /// Lowercase emails that become admins on registration.
    pub admin_emails: Arc<Vec<String>>,
}

impl AppState {
    pub fn new(db: libsql::Database) -> Self {
        Self {
            db: Arc::new(db),
            storage: None,
            admin_emails: Arc::default(),
        }
    }

    /// State for the API and the worker, from the configuration (everything
    /// but storage, which needs its own setup).
    pub fn from_config(db: libsql::Database, config: &Config) -> anyhow::Result<Self> {
        Ok(Self::new(db).with_admin_emails(config.admin_emails.clone()))
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
}
