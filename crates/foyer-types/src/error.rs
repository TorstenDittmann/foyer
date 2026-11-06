use serde::{Deserialize, Serialize};

/// Body of every 4xx and 5xx response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ApiError {
    /// Stable machine-readable code, e.g. `not_found`, `validation`,
    /// `missing_chunks`. Match on this, not on `message`.
    pub error: String,
    /// Human-readable explanation.
    pub message: String,
    /// For `missing_chunks`: the chunk hashes that are not in storage yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<Vec<String>>,
}
