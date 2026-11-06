//! Wire types for the Foyer HTTP API.
//!
//! Everything the API sends or accepts as JSON is defined here, so the API,
//! the website, the CLI and the desktop client agree on one set of shapes.
//! The crate only depends on serde, uuid and chrono and builds for
//! `wasm32-unknown-unknown`. With the `openapi` feature every type also
//! derives `utoipa::ToSchema`, which the API uses to publish its OpenAPI
//! document.
//!
//! Conventions:
//! - ids are UUIDs, serialized as strings;
//! - timestamps are RFC 3339 strings in UTC;
//! - enums are serialized in `snake_case`;
//! - list endpoints return [`Page`].

pub mod auth;
pub mod builds;
pub mod developers;
pub mod downloads;
pub mod error;
pub mod games;
pub mod review;

pub use auth::*;
pub use builds::*;
pub use developers::*;
pub use downloads::*;
pub use error::*;
pub use games::*;
pub use review::*;

use serde::{Deserialize, Serialize};

/// The version prefix every endpoint lives under.
pub const API_PREFIX: &str = "/v1";

/// One page of a list. Pass `next_cursor` back as `cursor` to get the next
/// page; it is `None` on the last page. Cursors are opaque strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}
