use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{DeveloperSummary, PageStyle, Platform, SupportLink};

/// Where a game is in its life cycle. Only `published` games are public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    /// Being set up; never reviewed.
    Draft,
    /// A submission is waiting for or under review.
    InReview,
    /// The reviewer asked for changes; the developer can resubmit.
    ChangesRequested,
    /// Approved and listed.
    Published,
    /// Rejected under a content rule; cannot be resubmitted.
    Rejected,
    /// Taken down by staff after being published.
    Delisted,
}

impl GameStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            GameStatus::Draft => "draft",
            GameStatus::InReview => "in_review",
            GameStatus::ChangesRequested => "changes_requested",
            GameStatus::Published => "published",
            GameStatus::Rejected => "rejected",
            GameStatus::Delisted => "delisted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "draft" => GameStatus::Draft,
            "in_review" => GameStatus::InReview,
            "changes_requested" => GameStatus::ChangesRequested,
            "published" => GameStatus::Published,
            "rejected" => GameStatus::Rejected,
            "delisted" => GameStatus::Delisted,
            _ => return None,
        })
    }
}

/// Content a player may want to know about before playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ContentDescriptor {
    Violence,
    Blood,
    Horror,
    StrongLanguage,
    DrugsAlcohol,
    MatureThemes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// The cover image used in lists. A game has at most one.
    Capsule,
    Screenshot,
    /// Art for the page design: a hero banner, a logo or an image block. Not
    /// shown in the screenshot gallery.
    Artwork,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaKind::Capsule => "capsule",
            MediaKind::Screenshot => "screenshot",
            MediaKind::Artwork => "artwork",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "capsule" => Some(MediaKind::Capsule),
            "screenshot" => Some(MediaKind::Screenshot),
            "artwork" => Some(MediaKind::Artwork),
            _ => None,
        }
    }
}

/// An image on a game page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Media {
    pub id: Uuid,
    pub kind: MediaKind,
    /// Public URL of the image.
    pub url: String,
    /// Sort order among images of the same kind, starting at 0.
    pub position: i64,
}

/// A game page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Game {
    pub id: Uuid,
    pub slug: String,
    pub developer: DeveloperSummary,
    pub title: String,
    /// One line shown in lists.
    pub tagline: String,
    /// Markdown.
    pub description: String,
    /// Lowercase slugs such as `roguelike` or `pixel-art`.
    pub tags: Vec<String>,
    pub content_descriptors: Vec<ContentDescriptor>,
    /// How generative AI was used, if at all. Disclosure is required by the
    /// content guidelines.
    pub ai_disclosure: Option<String>,
    pub support_links: Vec<SupportLink>,
    /// Platforms with at least one ready build.
    pub platforms: Vec<Platform>,
    pub media: Vec<Media>,
    /// How the page looks: colours, art and the order of its sections.
    #[serde(default)]
    pub page: PageStyle,
    pub status: GameStatus,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `POST /v1/games`. The game is created as a draft owned by the caller's
/// developer profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateGame {
    /// 2 to 40 characters: lowercase letters, digits and `-`. Unique.
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub content_descriptors: Vec<ContentDescriptor>,
    #[serde(default)]
    pub ai_disclosure: Option<String>,
    #[serde(default)]
    pub support_links: Vec<SupportLink>,
}

/// `PATCH /v1/games/{slug}`. Omitted fields stay unchanged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateGame {
    pub title: Option<String>,
    pub tagline: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub content_descriptors: Option<Vec<ContentDescriptor>>,
    /// Send `""` to clear it.
    pub ai_disclosure: Option<String>,
    pub support_links: Option<Vec<SupportLink>>,
    /// Replaces the whole page design. Colours must stay readable; see
    /// `PageStyle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageStyle>,
}

/// `POST /v1/games/{slug}/media`: reserve an image and get an upload URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateMedia {
    pub kind: MediaKind,
    /// `image/png`, `image/jpeg`, `image/webp` or `image/avif`.
    pub content_type: String,
}

/// Upload the image with `PUT upload_url`, sending the same `Content-Type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MediaUpload {
    pub media: Media,
    pub upload_url: String,
    pub expires_at: DateTime<Utc>,
}
