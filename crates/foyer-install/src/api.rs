//! Anonymous client for the public parts of the Foyer API.
//!
//! The desktop client never logs in: browsing, installing and updating only
//! use endpoints that answer without a token. Chunk bytes come straight from
//! storage through presigned URLs, which are fetched without any headers.

use std::fmt;
use std::time::Duration;

use foyer_types::{ApiError, Build, ChannelStatus, DownloadUrls, Game, Manifest, Page};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Default API base URL. Override with `FOYER_API_URL` or the `api_url`
/// setting.
pub const DEFAULT_API_URL: &str = "https://api.foyer.play";

/// Most chunk URLs the API hands out per request.
pub const MAX_DOWNLOADS_PER_REQUEST: usize = 10_000;

/// Why a download is being started, so the API can count installs and
/// updates (an anonymous counter, nothing identifies the player).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadEvent {
    Install,
    Update,
}

/// `POST /v1/builds/{id}/downloads` with the optional counting field.
///
/// Mirrors `foyer_types::DownloadRequest` plus the `event` field the backend
/// is adding; switch to the shared type once it carries `event`.
#[derive(Debug, Clone, Default, Serialize)]
struct DownloadRequest<'a> {
    hashes: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<DownloadEvent>,
}

/// One installed build, as sent to `POST /v1/updates`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledBuild {
    /// The game's slug.
    pub game: String,
    pub channel: String,
    pub build_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
struct UpdateCheck<'a> {
    installed: &'a [InstalledBuild],
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AvailableUpdate {
    pub game: String,
    pub channel: String,
    pub installed_build_id: Uuid,
    pub latest: Build,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct UpdateCheckResponse {
    pub updates: Vec<AvailableUpdate>,
    /// Games or channels that are no longer public (e.g. delisted).
    #[serde(default)]
    pub unavailable: Vec<InstalledBuild>,
}

/// A failed API or storage request.
#[derive(Debug, thiserror::Error)]
pub enum ApiFailure {
    #[error("could not reach {0}: {1}")]
    Network(String, String),
    #[error("too many requests; try again in {0:?}")]
    RateLimited(Duration),
    #[error("{}", .body.message)]
    Status { status: StatusCode, body: ApiError },
    #[error("unexpected response from {0}: {1}")]
    Decode(String, String),
}

impl ApiFailure {
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            ApiFailure::Status { status, .. } => Some(*status),
            ApiFailure::RateLimited(_) => Some(StatusCode::TOO_MANY_REQUESTS),
            _ => None,
        }
    }

    /// True for errors where waiting and retrying can help.
    pub fn is_transient(&self) -> bool {
        match self {
            ApiFailure::Network(..) | ApiFailure::RateLimited(_) => true,
            ApiFailure::Status { status, .. } => status.is_server_error(),
            ApiFailure::Decode(..) => false,
        }
    }
}

/// Filters for the catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameQuery {
    pub q: Option<String>,
    pub tag: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    /// For presigned storage URLs and images: never carries API headers.
    storage: reqwest::Client,
    base: String,
}

impl fmt::Debug for Api {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Api").field("base", &self.base).finish()
    }
}

const USER_AGENT: &str = concat!("foyer-desktop/", env!("CARGO_PKG_VERSION"));

impl Api {
    pub fn new(base: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build()?;
        let storage = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self {
            http,
            storage,
            base: base.trim_end_matches('/').to_string(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + ?Sized)>,
    ) -> Result<T, ApiFailure> {
        let url = format!("{}{path}", self.base);
        let mut req = self.http.request(method, &url);
        if let Some(body) = body {
            req = req.json(body);
        }
        let res = req
            .send()
            .await
            .map_err(|err| ApiFailure::Network(self.base.clone(), err.to_string()))?;
        let status = res.status();
        if status.is_success() {
            let text = res
                .text()
                .await
                .map_err(|err| ApiFailure::Network(self.base.clone(), err.to_string()))?;
            return serde_json::from_str(&text)
                .map_err(|err| ApiFailure::Decode(url, err.to_string()));
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(ApiFailure::RateLimited(retry_after(&res)));
        }
        let text = res.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or_else(|_| ApiError {
            error: "http_error".into(),
            message: if text.is_empty() {
                status.canonical_reason().unwrap_or("request failed").into()
            } else {
                text
            },
            missing: None,
        });
        Err(ApiFailure::Status { status, body })
    }

    /// Published games, newest first.
    pub async fn games(&self, query: &GameQuery) -> Result<Page<Game>, ApiFailure> {
        let mut params = Vec::new();
        if let Some(q) = query.q.as_deref().filter(|q| !q.trim().is_empty()) {
            params.push(format!("q={}", encode(q.trim())));
        }
        if let Some(tag) = &query.tag {
            params.push(format!("tag={}", encode(tag)));
        }
        if let Some(cursor) = &query.cursor {
            params.push(format!("cursor={}", encode(cursor)));
        }
        if let Some(limit) = query.limit {
            params.push(format!("limit={limit}"));
        }
        let path = if params.is_empty() {
            "/v1/games".to_string()
        } else {
            format!("/v1/games?{}", params.join("&"))
        };
        self.request(Method::GET, &path, None::<&()>).await
    }

    pub async fn game(&self, slug: &str) -> Result<Game, ApiFailure> {
        let path = format!("/v1/games/{}", encode(slug));
        self.request(Method::GET, &path, None::<&()>).await
    }

    /// Public channels of a game and the build each one serves.
    pub async fn channels(&self, slug: &str) -> Result<Vec<ChannelStatus>, ApiFailure> {
        let path = format!("/v1/games/{}/channels", encode(slug));
        self.request(Method::GET, &path, None::<&()>).await
    }

    pub async fn manifest(&self, build_id: Uuid) -> Result<Manifest, ApiFailure> {
        let path = format!("/v1/builds/{build_id}/manifest");
        self.request(Method::GET, &path, None::<&()>).await
    }

    /// Presigned URLs for some of a build's chunks (at most
    /// [`MAX_DOWNLOADS_PER_REQUEST`]).
    pub async fn download_urls(
        &self,
        build_id: Uuid,
        hashes: &[String],
        event: Option<DownloadEvent>,
    ) -> Result<DownloadUrls, ApiFailure> {
        let path = format!("/v1/builds/{build_id}/downloads");
        let body = DownloadRequest { hashes, event };
        self.request(Method::POST, &path, Some(&body)).await
    }

    /// Which installed builds have a newer build on their channel.
    ///
    /// Uses `POST /v1/updates` and falls back to asking each game's channels
    /// when the API does not have that endpoint yet.
    pub async fn check_updates(
        &self,
        installed: &[InstalledBuild],
    ) -> Result<UpdateCheckResponse, ApiFailure> {
        let mut out = UpdateCheckResponse::default();
        for batch in installed.chunks(500) {
            match self
                .request::<UpdateCheckResponse>(
                    Method::POST,
                    "/v1/updates",
                    Some(&UpdateCheck { installed: batch }),
                )
                .await
            {
                Ok(res) => {
                    out.updates.extend(res.updates);
                    out.unavailable.extend(res.unavailable);
                }
                Err(err)
                    if matches!(
                        err.status(),
                        Some(StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED)
                    ) =>
                {
                    let res = self.check_updates_by_channel(batch).await?;
                    out.updates.extend(res.updates);
                    out.unavailable.extend(res.unavailable);
                }
                Err(err) => return Err(err),
            }
        }
        Ok(out)
    }

    async fn check_updates_by_channel(
        &self,
        installed: &[InstalledBuild],
    ) -> Result<UpdateCheckResponse, ApiFailure> {
        let mut out = UpdateCheckResponse::default();
        for entry in installed {
            let channels = match self.channels(&entry.game).await {
                Ok(channels) => channels,
                Err(err) if err.status() == Some(StatusCode::NOT_FOUND) => {
                    out.unavailable.push(entry.clone());
                    continue;
                }
                Err(err) => return Err(err),
            };
            match channels
                .into_iter()
                .find(|c| c.channel == entry.channel)
                .and_then(|c| c.latest)
            {
                Some(latest) if latest.id != entry.build_id => out.updates.push(AvailableUpdate {
                    game: entry.game.clone(),
                    channel: entry.channel.clone(),
                    installed_build_id: entry.build_id,
                    latest,
                }),
                Some(_) => {}
                None => out.unavailable.push(entry.clone()),
            }
        }
        Ok(out)
    }

    /// GET a presigned storage URL (or a public image URL).
    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let res = self
            .storage
            .get(url)
            .send()
            .await
            .map_err(|err| FetchError::Transient(err.to_string()))?;
        let status = res.status();
        if status.is_success() {
            return res
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|err| FetchError::Transient(err.to_string()));
        }
        Err(match status {
            StatusCode::FORBIDDEN => FetchError::Expired,
            StatusCode::TOO_MANY_REQUESTS => FetchError::RateLimited(retry_after(&res)),
            s if s.is_server_error() => FetchError::Transient(format!("storage answered {s}")),
            s => FetchError::Fatal(format!("storage answered {s}")),
        })
    }
}

/// Why a storage GET failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The presigned URL expired or was refused; get a new one.
    #[error("download link expired")]
    Expired,
    #[error("rate limited")]
    RateLimited(Duration),
    /// Network error or 5xx; worth retrying.
    #[error("{0}")]
    Transient(String),
    #[error("{0}")]
    Fatal(String),
}

fn retry_after(res: &reqwest::Response) -> Duration {
    res.headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| Duration::from_secs(s.clamp(1, 300)))
        .unwrap_or(Duration::from_secs(5))
}

/// Percent-encode a path segment or query value.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_characters() {
        assert_eq!(encode("pixel art & co"), "pixel%20art%20%26%20co");
        assert_eq!(encode("my-game"), "my-game");
    }

    #[test]
    fn download_request_omits_event_when_unset() {
        let hashes = vec!["a".to_string()];
        let body = serde_json::to_value(DownloadRequest {
            hashes: &hashes,
            event: None,
        })
        .unwrap();
        assert_eq!(body, serde_json::json!({ "hashes": ["a"] }));
        let body = serde_json::to_value(DownloadRequest {
            hashes: &hashes,
            event: Some(DownloadEvent::Update),
        })
        .unwrap();
        assert_eq!(body["event"], "update");
    }
}
