//! Foyer HTTP API.
//!
//! The binary in `main.rs` only wires configuration and the listener; all
//! routes live here so they can be exercised in tests without a socket.

use std::sync::Arc;

pub mod config;
pub mod db;
mod routes;

pub use config::Config;
pub use routes::router;

/// Shared state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<libsql::Database>,
}

impl AppState {
    pub fn new(db: libsql::Database) -> Self {
        Self { db: Arc::new(db) }
    }
}
