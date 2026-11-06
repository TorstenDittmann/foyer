//! Object storage: presigned URLs for clients, plus HEAD checks so the API
//! can confirm uploads without moving any bytes itself.
//!
//! Layout inside the bucket:
//! - `games/{game_id}/chunks/{hash}`: build chunks, one object per hash.
//!   Chunks are scoped per game so one developer can never overwrite or
//!   probe another game's chunks.
//! - `games/{game_id}/media/{media_id}`: page images.

use std::time::Duration;

use anyhow::Context;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use uuid::Uuid;

use crate::config::StorageConfig;

/// How long presigned URLs stay valid.
pub const PRESIGN_TTL: Duration = Duration::from_secs(60 * 60);

pub struct Storage {
    bucket: Bucket,
    credentials: Credentials,
    media_base_url: url::Url,
    http: reqwest::Client,
}

/// Result of a HEAD request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectState {
    Missing,
    Present { size: u64 },
}

impl Storage {
    pub fn new(config: &StorageConfig) -> anyhow::Result<Self> {
        let style = if config.virtual_hosted {
            UrlStyle::VirtualHost
        } else {
            UrlStyle::Path
        };
        let bucket = Bucket::new(
            config.endpoint.clone(),
            style,
            config.bucket.clone(),
            config.region.clone(),
        )
        .context("invalid S3 bucket configuration")?;
        let media_base_url = match &config.media_base_url {
            Some(url) => url.clone(),
            None => bucket.base_url().clone(),
        };
        Ok(Self {
            bucket,
            credentials: Credentials::new(&config.access_key_id, &config.secret_access_key),
            media_base_url,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
        })
    }

    pub fn chunk_key(game_id: Uuid, hash: &str) -> String {
        format!("games/{game_id}/chunks/{hash}")
    }

    pub fn media_key(game_id: Uuid, media_id: Uuid) -> String {
        format!("games/{game_id}/media/{media_id}")
    }

    pub fn presign_put(&self, key: &str) -> String {
        self.bucket
            .put_object(Some(&self.credentials), key)
            .sign(PRESIGN_TTL)
            .to_string()
    }

    pub fn presign_get(&self, key: &str) -> String {
        self.bucket
            .get_object(Some(&self.credentials), key)
            .sign(PRESIGN_TTL)
            .to_string()
    }

    /// Public, unsigned URL for an object, for images on game pages.
    pub fn public_url(&self, key: &str) -> String {
        let base = self.media_base_url.as_str().trim_end_matches('/');
        format!("{base}/{key}")
    }

    /// Checks whether an object exists and how large it is.
    pub async fn head(&self, key: &str) -> anyhow::Result<ObjectState> {
        let url = self
            .bucket
            .head_object(Some(&self.credentials), key)
            .sign(Duration::from_secs(60));
        let response = self.http.head(url).send().await?;
        match response.status() {
            status if status.is_success() => {
                let size = response
                    .headers()
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .context("HEAD response without Content-Length")?;
                Ok(ObjectState::Present { size })
            }
            reqwest::StatusCode::NOT_FOUND => Ok(ObjectState::Missing),
            status => anyhow::bail!("HEAD {key} returned {status}"),
        }
    }
}
