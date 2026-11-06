//! HTTP client for the Foyer API and for presigned chunk uploads.

use std::fmt;
use std::time::Duration;

use anyhow::Context;
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::api::{
    ApiError, Build, ChannelStatus, CreateBuild, CreateBuildResponse, UploadUrls, User,
};

/// A non-2xx answer from the API, with its decoded error body.
#[derive(Debug)]
pub struct ApiFailure {
    pub status: StatusCode,
    pub body: ApiError,
}

impl fmt::Display for ApiFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.status == StatusCode::UNAUTHORIZED {
            write!(
                f,
                "not authorized ({}); run `foyer login` or set FOYER_TOKEN",
                self.body.message
            )
        } else {
            write!(
                f,
                "API error {}: {}",
                self.status.as_u16(),
                self.body.message
            )
        }
    }
}

impl std::error::Error for ApiFailure {}

pub struct ApiClient {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

impl ApiClient {
    pub fn new(base: &str, token: Option<String>) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("foyer-cli/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token,
        })
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + ?Sized)>,
    ) -> anyhow::Result<T> {
        let url = format!("{}{path}", self.base);
        let mut req = self.http.request(method, &url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        let res = req
            .send()
            .await
            .with_context(|| format!("could not reach the Foyer API at {}", self.base))?;
        let status = res.status();
        if status.is_success() {
            return res
                .json()
                .await
                .with_context(|| format!("unexpected response from {url}"));
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
        Err(ApiFailure { status, body }.into())
    }

    pub async fn me(&self) -> anyhow::Result<User> {
        self.request(Method::GET, "/v1/me", None::<&()>).await
    }

    pub async fn create_build(
        &self,
        game: &str,
        body: &CreateBuild,
    ) -> anyhow::Result<CreateBuildResponse> {
        let path = format!("/v1/games/{}/builds", segment(game));
        self.request(Method::POST, &path, Some(body)).await
    }

    pub async fn refresh_uploads(&self, build_id: &str) -> anyhow::Result<UploadUrls> {
        let path = format!("/v1/builds/{}/uploads", segment(build_id));
        self.request(Method::POST, &path, None::<&()>).await
    }

    pub async fn finalize(&self, build_id: &str) -> anyhow::Result<Build> {
        let path = format!("/v1/builds/{}/finalize", segment(build_id));
        self.request(Method::POST, &path, None::<&()>).await
    }

    pub async fn build(&self, build_id: &str) -> anyhow::Result<Build> {
        let path = format!("/v1/builds/{}", segment(build_id));
        self.request(Method::GET, &path, None::<&()>).await
    }

    pub async fn channels(&self, game: &str) -> anyhow::Result<Vec<ChannelStatus>> {
        let path = format!("/v1/games/{}/channels", segment(game));
        self.request(Method::GET, &path, None::<&()>).await
    }
}

/// Percent-encode a path segment (slugs and ids are plain, but be safe).
fn segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Why a chunk PUT failed.
#[derive(Debug)]
pub enum PutError {
    /// The presigned URL expired or was refused; ask the API for a new one.
    Expired,
    /// Network error or 5xx; worth retrying.
    Transient(String),
    /// Anything else; retrying will not help.
    Fatal(String),
}

/// Uploads chunk bytes to presigned storage URLs. Has no API token on
/// purpose: storage must never see it.
#[derive(Clone)]
pub struct ChunkUploader {
    http: reqwest::Client,
}

impl ChunkUploader {
    pub fn new() -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("foyer-cli/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self { http })
    }

    pub async fn put(&self, url: &str, data: Vec<u8>) -> Result<(), PutError> {
        let res = self
            .http
            .put(url)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(data)
            .send()
            .await
            .map_err(|err| PutError::Transient(err.to_string()))?;
        let status = res.status();
        match status {
            s if s.is_success() => Ok(()),
            StatusCode::FORBIDDEN => Err(PutError::Expired),
            s if s.is_server_error() || s == StatusCode::TOO_MANY_REQUESTS => {
                Err(PutError::Transient(format!("storage answered {s}")))
            }
            s => Err(PutError::Fatal(format!(
                "storage answered {s}: {}",
                res.text().await.unwrap_or_default()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::segment;

    #[test]
    fn encodes_path_segments() {
        assert_eq!(segment("my-game_2"), "my-game_2");
        assert_eq!(segment("a/b c"), "a%2Fb%20c");
    }
}
