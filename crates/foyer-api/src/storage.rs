//! Object storage: presigned URLs for clients, plus HEAD checks so the API
//! can confirm uploads without moving any bytes itself.
//!
//! Layout inside the bucket:
//! - `games/{game_id}/chunks/{hash}`: build chunks, one object per hash.
//!   Chunks are scoped per game so one developer can never overwrite or
//!   probe another game's chunks.
//! - `games/{game_id}/media/{media_id}`: page images.
//! - `games/{game_id}/archives/{build_id}.zip`: the zip of a build for
//!   browser downloads, written by the worker.

use std::time::Duration;

use anyhow::Context;
use rusty_s3::actions::{CompleteMultipartUpload, CreateMultipartUpload};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::config::StorageConfig;

/// How long presigned URLs stay valid.
pub const PRESIGN_TTL: Duration = Duration::from_secs(60 * 60);

pub struct Storage {
    bucket: Bucket,
    credentials: Credentials,
    media_base_url: url::Url,
    http: reqwest::Client,
    /// Files larger than this are uploaded in parts of this size.
    part_size: u64,
}

/// S3 parts must be at least 5 MiB (except the last); 64 MiB keeps a 64 GiB
/// zip under the 10 000 part limit.
pub const DEFAULT_PART_SIZE: u64 = 64 * 1024 * 1024;

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
                .connect_timeout(Duration::from_secs(30))
                .read_timeout(Duration::from_secs(120))
                .build()?,
            part_size: DEFAULT_PART_SIZE,
        })
    }

    /// Changes the multipart threshold and part size. Tests use small parts.
    pub fn with_part_size(mut self, part_size: u64) -> Self {
        self.part_size = part_size;
        self
    }

    pub fn archive_key(game_id: Uuid, build_id: Uuid) -> String {
        format!("games/{game_id}/archives/{build_id}.zip")
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

    /// Presigned GET that makes browsers save the object as `filename`.
    pub fn presign_download(&self, key: &str, filename: &str) -> String {
        let mut action = self.bucket.get_object(Some(&self.credentials), key);
        action.query_mut().insert(
            "response-content-disposition",
            format!("attachment; filename=\"{filename}\""),
        );
        action.sign(PRESIGN_TTL).to_string()
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

    /// Downloads a whole object. Meant for chunks, which are at most a few
    /// MiB.
    pub async fn get(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let url = self
            .bucket
            .get_object(Some(&self.credentials), key)
            .sign(Duration::from_secs(300));
        let response = self.http.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("GET {key} returned {status}");
        }
        Ok(response.bytes().await?.to_vec())
    }

    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let url = self
            .bucket
            .delete_object(Some(&self.credentials), key)
            .sign(Duration::from_secs(60));
        let response = self.http.delete(url).send().await?;
        let status = response.status();
        if !status.is_success() && status != reqwest::StatusCode::NOT_FOUND {
            anyhow::bail!("DELETE {key} returned {status}");
        }
        Ok(())
    }

    /// Uploads a local file, in parts when it is larger than the part size.
    pub async fn upload_file(
        &self,
        key: &str,
        path: &std::path::Path,
        content_type: &str,
    ) -> anyhow::Result<()> {
        let mut file = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("open {}", path.display()))?;
        let size = file.metadata().await?.len();
        if size <= self.part_size {
            let mut body = Vec::with_capacity(size as usize);
            file.read_to_end(&mut body).await?;
            return self.put(key, body, Some(content_type)).await;
        }

        let mut create = self
            .bucket
            .create_multipart_upload(Some(&self.credentials), key);
        create
            .headers_mut()
            .insert("content-type", content_type.to_string());
        let response = self
            .http
            .post(create.sign(Duration::from_secs(300)))
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("starting a multipart upload of {key} returned {status}");
        }
        let created = CreateMultipartUpload::parse_response(response.bytes().await?)
            .context("bad CreateMultipartUpload response")?;
        let upload_id = created.upload_id().to_string();

        let result = self.upload_parts(key, &upload_id, &mut file, size).await;
        let etags = match result {
            Ok(etags) => etags,
            Err(err) => {
                let abort = self
                    .bucket
                    .abort_multipart_upload(Some(&self.credentials), key, &upload_id)
                    .sign(Duration::from_secs(60));
                let _ = self.http.delete(abort).send().await;
                return Err(err);
            }
        };

        let complete = CompleteMultipartUpload::new(
            &self.bucket,
            Some(&self.credentials),
            key,
            &upload_id,
            etags.iter().map(String::as_str),
        );
        let url = complete.sign(Duration::from_secs(300));
        let response = self.http.post(url).body(complete.body()).send().await?;
        let status = response.status();
        // S3 can answer 200 with an error document when completing fails.
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() || body.contains("<Error>") {
            anyhow::bail!("completing the upload of {key} returned {status}: {body}");
        }
        Ok(())
    }

    async fn upload_parts(
        &self,
        key: &str,
        upload_id: &str,
        file: &mut tokio::fs::File,
        size: u64,
    ) -> anyhow::Result<Vec<String>> {
        let parts = size.div_ceil(self.part_size);
        let mut etags = Vec::with_capacity(parts as usize);
        for number in 1..=parts {
            let len = self.part_size.min(size - (number - 1) * self.part_size);
            let mut body = vec![0; len as usize];
            file.read_exact(&mut body).await?;
            let number = u16::try_from(number).context("too many parts")?;
            let url = self
                .bucket
                .upload_part(Some(&self.credentials), key, number, upload_id)
                .sign(Duration::from_secs(600));
            let response = self.http.put(url).body(body).send().await?;
            let status = response.status();
            if !status.is_success() {
                anyhow::bail!("uploading part {number} of {key} returned {status}");
            }
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .context("part upload without ETag")?;
            etags.push(etag.to_string());
        }
        Ok(etags)
    }

    async fn put(
        &self,
        key: &str,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> anyhow::Result<()> {
        let url = self
            .bucket
            .put_object(Some(&self.credentials), key)
            .sign(Duration::from_secs(600));
        let mut request = self.http.put(url).body(body);
        if let Some(content_type) = content_type {
            request = request.header(reqwest::header::CONTENT_TYPE, content_type);
        }
        let status = request.send().await?.status();
        if !status.is_success() {
            anyhow::bail!("PUT {key} returned {status}");
        }
        Ok(())
    }
}
