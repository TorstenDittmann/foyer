use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use foyer_patch::Manifest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Macos,
    Linux,
    Web,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Macos => "macos",
            Platform::Linux => "linux",
            Platform::Web => "web",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "windows" => Some(Platform::Windows),
            "macos" => Some(Platform::Macos),
            "linux" => Some(Platform::Linux),
            "web" => Some(Platform::Web),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum BuildState {
    /// Created; chunks are being uploaded.
    Uploading,
    /// All chunks are uploaded; automated checks are running.
    Processing,
    /// Downloadable, and the channel points at it.
    Ready,
    /// Checks could not complete.
    Failed,
    /// Checks found a problem, e.g. malware.
    Rejected,
}

impl BuildState {
    pub fn as_str(self) -> &'static str {
        match self {
            BuildState::Uploading => "uploading",
            BuildState::Processing => "processing",
            BuildState::Ready => "ready",
            BuildState::Failed => "failed",
            BuildState::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "uploading" => BuildState::Uploading,
            "processing" => BuildState::Processing,
            "ready" => BuildState::Ready,
            "failed" => BuildState::Failed,
            "rejected" => BuildState::Rejected,
            _ => return None,
        })
    }
}

/// One uploaded build of a game on a channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Build {
    pub id: Uuid,
    /// The game's slug.
    pub game: String,
    /// e.g. `windows` or `windows-beta`.
    pub channel: String,
    pub platform: Platform,
    /// Free-form version label set by the developer.
    pub version: Option<String>,
    pub state: BuildState,
    /// Sum of all file sizes in bytes.
    pub total_size: u64,
    /// Number of distinct chunks in the manifest.
    pub chunk_count: u64,
    pub created_at: DateTime<Utc>,
}

/// `POST /v1/games/{slug}/builds`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateBuild {
    /// 1 to 40 characters: lowercase letters, digits and `-`. A channel is
    /// tied to the platform of its first build.
    pub channel: String,
    pub platform: Platform,
    #[serde(default)]
    pub version: Option<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = ManifestSchema))]
    pub manifest: Manifest,
}

/// Upload a chunk with `PUT url` and the chunk's bytes as the body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChunkUpload {
    pub hash: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateBuildResponse {
    pub build: Build,
    /// Only the chunks storage does not have yet. Empty when every chunk was
    /// uploaded before; call finalize right away.
    pub uploads: Vec<ChunkUpload>,
    /// When the upload URLs stop working. Get fresh ones from
    /// `POST /v1/builds/{id}/uploads`.
    pub expires_at: DateTime<Utc>,
}

/// `POST /v1/builds/{id}/uploads`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UploadUrls {
    pub uploads: Vec<ChunkUpload>,
    pub expires_at: DateTime<Utc>,
}

/// A channel and the build it currently serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChannelStatus {
    pub channel: String,
    pub platform: Platform,
    pub latest: Option<Build>,
}

/// `POST /v1/builds/{id}/downloads`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DownloadRequest {
    /// Chunks to download. Empty means every chunk in the build. At most
    /// 10 000 per request.
    #[serde(default)]
    pub hashes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChunkDownload {
    pub hash: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DownloadUrls {
    pub downloads: Vec<ChunkDownload>,
    pub expires_at: DateTime<Utc>,
}

/// OpenAPI description of [`Manifest`], which lives in `foyer-patch`.
#[cfg(feature = "openapi")]
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct ManifestSchema {
    /// Chunker settings the build was split with.
    chunker: Option<ChunkerConfigSchema>,
    /// Every file in the build, keyed by its `/`-separated path relative to
    /// the build root.
    files: std::collections::BTreeMap<String, ManifestFileSchema>,
}

#[cfg(feature = "openapi")]
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct ChunkerConfigSchema {
    min_size: u32,
    avg_size: u32,
    max_size: u32,
}

#[cfg(feature = "openapi")]
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct ManifestFileSchema {
    /// Size in bytes.
    size: u64,
    /// Lowercase hex BLAKE3 hash of the file contents.
    hash: String,
    /// The file's content-defined chunks, in order. Empty for an empty file.
    chunks: Vec<ChunkSchema>,
}

#[cfg(feature = "openapi")]
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub struct ChunkSchema {
    /// Lowercase hex BLAKE3 hash of the chunk bytes.
    hash: String,
    /// Size in bytes.
    size: u64,
}
