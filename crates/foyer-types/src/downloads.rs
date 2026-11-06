//! Anonymous downloads: zip archives for the browser, update checks for the
//! desktop client, and the download counts developers see.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Build, Platform};

/// A zip of a channel's current build, for downloading in the browser
/// (`GET /v1/games/{slug}/downloads`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GameDownload {
    pub platform: Platform,
    pub channel: String,
    /// True for the channel the game page offers for this platform: the one
    /// named after the platform (`windows`), else the most recently updated.
    pub default: bool,
    pub build: Build,
    /// Size of the zip in bytes.
    pub archive_size: u64,
    /// Suggested file name, e.g. `lantern-1.0-windows.zip`.
    pub filename: String,
}

/// `POST /v1/games/{slug}/channels/{channel}/archive`: a short-lived link
/// to the zip. Each call counts as one browser download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ArchiveDownload {
    pub url: String,
    pub filename: String,
    pub size: u64,
    pub expires_at: DateTime<Utc>,
}

/// A build the desktop client has installed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstalledBuild {
    /// The game's slug.
    pub game: String,
    pub channel: String,
    pub build_id: Uuid,
}

/// `POST /v1/updates`. At most 500 entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateCheck {
    pub installed: Vec<InstalledBuild>,
}

/// A newer build for an installed game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AvailableUpdate {
    pub game: String,
    pub channel: String,
    pub installed_build_id: Uuid,
    /// The channel's current build. Fetch its manifest and plan the patch
    /// with `foyer_patch::diff`.
    pub latest: Build,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateCheckResponse {
    /// Only entries whose channel serves a different build now.
    pub updates: Vec<AvailableUpdate>,
    /// Entries whose game or channel is no longer public (e.g. delisted).
    /// The installed copy keeps working, but there is nothing to update to.
    pub unavailable: Vec<InstalledBuild>,
}

/// Downloads of one platform, or of one day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DownloadCounts {
    /// Zip downloads from the website.
    pub web: u64,
    /// Installs from the desktop client.
    pub client_installs: u64,
    /// Updates from the desktop client.
    pub client_updates: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PlatformDownloads {
    pub platform: Platform,
    #[serde(flatten)]
    pub counts: DownloadCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DailyDownloads {
    /// UTC day.
    pub day: NaiveDate,
    #[serde(flatten)]
    pub counts: DownloadCounts,
}

/// `GET /v1/games/{slug}/stats`: download counts for the developer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GameStats {
    /// All-time totals.
    pub total: DownloadCounts,
    /// All-time totals per platform.
    pub platforms: Vec<PlatformDownloads>,
    /// One entry per day of the requested range, oldest first, including
    /// days without downloads.
    pub daily: Vec<DailyDownloads>,
}
