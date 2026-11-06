use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where players can support a developer or a game. Foyer does not sell
/// games; pages link out to these instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    Patreon,
    Kofi,
    GithubSponsors,
    Itchio,
    Website,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SupportLink {
    pub kind: SupportKind,
    /// An `https://` URL.
    pub url: String,
    /// Optional button text, e.g. "Buy me a coffee".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A developer (person or studio). Each user can own one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Developer {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub bio: String,
    pub website_url: Option<String>,
    pub support_links: Vec<SupportLink>,
    pub created_at: DateTime<Utc>,
}

/// The developer as shown on a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DeveloperSummary {
    pub slug: String,
    pub display_name: String,
}

/// `POST /v1/developers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateDeveloper {
    /// 2 to 40 characters: lowercase letters, digits and `-`.
    pub slug: String,
    pub display_name: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub website_url: Option<String>,
    #[serde(default)]
    pub support_links: Vec<SupportLink>,
}

/// `PATCH /v1/developers/{slug}`. Omitted fields stay unchanged. The slug
/// cannot be changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateDeveloper {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    /// Send `""` to clear it.
    pub website_url: Option<String>,
    pub support_links: Option<Vec<SupportLink>>,
}
