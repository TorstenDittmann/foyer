//! Wire types for the Foyer API, as agreed with the API.
//!
//! These are provisional: they move to the shared `foyer-types` crate once
//! it lands, and this module becomes a re-export.

use std::fmt;
use std::str::FromStr;

use foyer_patch::Manifest;
use serde::{Deserialize, Serialize};

/// `GET /v1/me`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Macos,
    Linux,
    Web,
}

impl Platform {
    pub const ALL: [Platform; 4] = [Self::Windows, Self::Macos, Self::Linux, Self::Web];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Web => "web",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Platform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|p| p.as_str() == s.to_ascii_lowercase())
            .ok_or_else(|| {
                format!("unknown platform {s:?} (expected windows, macos, linux or web)")
            })
    }
}

/// `POST /v1/games/{game}/builds`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBuild {
    pub channel: String,
    pub platform: Platform,
    pub version: Option<String>,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBuildResponse {
    pub build: Build,
    /// Chunks storage does not have yet, with presigned PUT URLs.
    pub uploads: Vec<ChunkUpload>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// `POST /v1/builds/{id}/uploads`: fresh URLs for chunks still missing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadUrls {
    pub uploads: Vec<ChunkUpload>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkUpload {
    pub hash: String,
    pub size: u64,
    /// Presigned S3 PUT URL. Opaque to the CLI; never send the API token to it.
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildState {
    Uploading,
    Processing,
    Ready,
    Failed,
    Rejected,
}

impl fmt::Display for BuildState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Uploading => "uploading",
            Self::Processing => "processing",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Rejected => "rejected",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    pub id: String,
    pub game: String,
    pub channel: String,
    pub platform: Platform,
    pub version: Option<String>,
    pub state: BuildState,
    pub total_size: u64,
    pub chunk_count: u64,
    pub created_at: String,
}

/// `GET /v1/games/{game}/channels`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStatus {
    pub channel: String,
    pub platform: Platform,
    pub latest: Option<Build>,
}

/// Error body for every non-2xx API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<Vec<String>>,
}

/// Channel names are short lowercase slugs such as `stable` or `beta`.
pub fn validate_channel(channel: &str) -> Result<(), String> {
    let ok = !channel.is_empty()
        && channel.len() <= 32
        && channel
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !channel.starts_with('-')
        && !channel.ends_with('-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid channel {channel:?}: use 1-32 lowercase letters, digits and dashes"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_parses_case_insensitively() {
        assert_eq!("MacOS".parse::<Platform>().unwrap(), Platform::Macos);
        assert!("amiga".parse::<Platform>().is_err());
        assert_eq!(serde_json::to_string(&Platform::Web).unwrap(), "\"web\"");
    }

    #[test]
    fn channel_names() {
        for ok in ["stable", "beta", "windows-beta", "v2"] {
            validate_channel(ok).unwrap();
        }
        for bad in ["", "Beta", "-x", "x-", "a b", "a/b", &"a".repeat(33)] {
            assert!(validate_channel(bad).is_err(), "{bad:?}");
        }
    }
}
