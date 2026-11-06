//! `foyer push`: upload a build directory to a channel.
//!
//! 1. Chunk and hash the build locally (the manifest).
//! 2. `POST /v1/games/{game}/builds` with the manifest; the API answers with
//!    presigned URLs for the chunks storage does not have yet.
//! 3. PUT those chunks, several at a time, retrying transient failures and
//!    asking for fresh URLs when they expire.
//! 4. `POST /v1/builds/{id}/finalize`.
//!
//! Chunks are deduplicated by hash on the server, so a push of a patched
//! build only uploads what changed, and re-running an interrupted push picks
//! up where it stopped.

use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, bail};
use foyer_patch::{BuildSource, Manifest};
use futures::StreamExt;
use reqwest::StatusCode;

use crate::api::{Build, ChunkUpload, CreateBuild, Platform, validate_channel};
use crate::client::{ApiClient, ApiFailure, ChunkUploader, PutError};
use crate::format::bytes;

pub struct PushArgs {
    pub dir: PathBuf,
    pub game: String,
    pub channel: String,
    pub platform: Platform,
    pub version: Option<String>,
    pub concurrency: usize,
    pub dry_run: bool,
    pub json: bool,
}

/// Attempts per chunk before giving up on a transient error.
const ATTEMPTS: u32 = 4;
/// Rounds of "ask the API what is still missing, upload it".
const ROUNDS: u32 = 3;

pub async fn run(api: &ApiClient, args: PushArgs) -> anyhow::Result<()> {
    validate_channel(&args.channel).map_err(anyhow::Error::msg)?;
    if !args.dir.is_dir() {
        bail!("{} is not a directory", args.dir.display());
    }

    let dir = args.dir.clone();
    let manifest = tokio::task::spawn_blocking(move || Manifest::from_dir(&dir))
        .await?
        .with_context(|| format!("failed to scan {}", args.dir.display()))?;
    if manifest.files.is_empty() {
        bail!("{} has no files to push", args.dir.display());
    }
    let unique = manifest.unique_chunks();
    eprintln!(
        "Scanned {} files, {} in {} chunks",
        manifest.files.len(),
        bytes(manifest.total_size()),
        unique.len()
    );
    if args.dry_run {
        eprintln!("Dry run: nothing was uploaded.");
        return Ok(());
    }

    let created = api
        .create_build(
            &args.game,
            &CreateBuild {
                channel: args.channel.clone(),
                platform: args.platform,
                version: args.version.clone(),
                manifest: manifest.clone(),
            },
        )
        .await
        .context("failed to create the build")?;
    let build_id = created.build.id.to_string();
    let unique_bytes: u64 = unique.values().sum();
    let missing_bytes: u64 = created.uploads.iter().map(|u| u.size).sum();
    eprintln!(
        "Build {build_id}: uploading {} of {} chunks ({} of {}), the rest is already stored",
        created.uploads.len(),
        unique.len(),
        bytes(missing_bytes),
        bytes(unique_bytes),
    );

    let source = Arc::new(BuildSource::new(&args.dir, &manifest));
    let uploader = ChunkUploader::new()?;
    let progress = Progress::new(missing_bytes);
    let concurrency = args.concurrency.max(1);

    let mut pending = created.uploads;
    let mut round = 0;
    let build = loop {
        round += 1;
        let expired = upload_all(&uploader, &source, pending, concurrency, &progress).await?;
        if !expired.is_empty() {
            if round >= ROUNDS {
                bail!(
                    "{} chunks could not be uploaded; run the push again to resume",
                    expired.len()
                );
            }
            pending = refresh(api, &build_id).await?;
            continue;
        }

        match retry(|| api.finalize(&build_id)).await {
            Ok(build) => break build,
            Err(err) => {
                let missing = match err.downcast_ref::<ApiFailure>() {
                    Some(f) if f.status == StatusCode::CONFLICT => f.body.missing.clone(),
                    _ => None,
                };
                let Some(missing) = missing.filter(|m| !m.is_empty()) else {
                    return Err(err.context("failed to finalize the build"));
                };
                if round >= ROUNDS {
                    bail!(
                        "storage is still missing {} chunks after {ROUNDS} attempts; run the push again to resume",
                        missing.len()
                    );
                }
                pending = refresh(api, &build_id).await?;
            }
        }
    };
    progress.finish();
    report(&build, progress.done(), args.json)
}

/// Ask the API for fresh URLs for every chunk storage still lacks.
async fn refresh(api: &ApiClient, build_id: &str) -> anyhow::Result<Vec<ChunkUpload>> {
    let fresh = retry(|| api.refresh_uploads(build_id))
        .await
        .context("failed to get new upload URLs")?;
    Ok(fresh.uploads)
}

/// Retry an idempotent API call on network errors and 5xx answers.
///
/// Used for finalize and refresh, so a hiccup after every chunk is uploaded
/// does not fail the push. Creating the build is not retried: it is not
/// idempotent, and a re-run resumes anyway.
async fn retry<T, F, Fut>(mut call: F) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match call().await {
            Err(err) if attempt < ATTEMPTS && is_transient(&err) => {
                let delay = Duration::from_millis(250 * 2u64.pow(attempt - 1));
                tracing_retry("API call", &err.to_string(), delay);
                tokio::time::sleep(delay).await;
            }
            result => return result,
        }
    }
}

fn is_transient(err: &anyhow::Error) -> bool {
    if let Some(failure) = err.downcast_ref::<ApiFailure>() {
        return failure.status.is_server_error() || failure.status == StatusCode::TOO_MANY_REQUESTS;
    }
    err.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(|e| e.is_connect() || e.is_timeout() || e.is_request())
    })
}

/// Upload every chunk; returns the hashes whose URL expired.
async fn upload_all(
    uploader: &ChunkUploader,
    source: &Arc<BuildSource>,
    uploads: Vec<ChunkUpload>,
    concurrency: usize,
    progress: &Progress,
) -> anyhow::Result<HashSet<String>> {
    let mut results = futures::stream::iter(uploads)
        .map(|upload| upload_one(uploader, source, upload, progress))
        .buffer_unordered(concurrency);

    let mut expired = HashSet::new();
    while let Some(result) = results.next().await {
        if let Some(hash) = result? {
            expired.insert(hash);
        }
    }
    Ok(expired)
}

/// Returns `Some(hash)` if the URL expired and the chunk needs a new one.
async fn upload_one(
    uploader: &ChunkUploader,
    source: &Arc<BuildSource>,
    upload: ChunkUpload,
    progress: &Progress,
) -> anyhow::Result<Option<String>> {
    let src = Arc::clone(source);
    let hash = upload.hash.clone();
    let data = tokio::task::spawn_blocking(move || src.read_chunk(&hash)).await??;
    if data.len() as u64 != upload.size {
        bail!(
            "API asked for {} bytes of chunk {}, the build has {}",
            upload.size,
            upload.hash,
            data.len()
        );
    }

    let mut attempt = 0;
    loop {
        attempt += 1;
        match uploader.put(&upload.url, data.clone()).await {
            Ok(()) => {
                progress.add(upload.size);
                return Ok(None);
            }
            Err(PutError::Expired) => return Ok(Some(upload.hash)),
            Err(PutError::Transient(msg)) if attempt < ATTEMPTS => {
                let delay = Duration::from_millis(250 * 2u64.pow(attempt - 1));
                tracing_retry(&format!("chunk {}", upload.hash), &msg, delay);
                tokio::time::sleep(delay).await;
            }
            Err(PutError::Transient(msg) | PutError::Fatal(msg)) => {
                bail!("failed to upload chunk {}: {msg}", upload.hash)
            }
        }
    }
}

fn tracing_retry(what: &str, msg: &str, delay: Duration) {
    if std::env::var_os("FOYER_DEBUG").is_some() {
        eprintln!("retrying {what} in {delay:?}: {msg}");
    }
}

fn report(build: &Build, uploaded: u64, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(build)?);
        return Ok(());
    }
    let version = build.version.as_deref().unwrap_or("(no version)");
    println!(
        "Pushed {} {version} to {}/{}: build {} is {}. Uploaded {} of {}.",
        build.game,
        build.channel,
        build.platform.as_str(),
        build.id,
        build.state.as_str(),
        bytes(uploaded),
        bytes(build.total_size),
    );
    Ok(())
}

/// Upload progress on stderr, only when it is a terminal.
struct Progress {
    total: u64,
    done: AtomicU64,
    tty: bool,
}

impl Progress {
    fn new(total: u64) -> Self {
        Self {
            total,
            done: AtomicU64::new(0),
            tty: std::io::stderr().is_terminal(),
        }
    }

    fn add(&self, n: u64) {
        let done = self.done.fetch_add(n, Ordering::Relaxed) + n;
        if self.tty {
            eprint!("\rUploaded {} of {}   ", bytes(done), bytes(self.total));
            let _ = std::io::stderr().flush();
        }
    }

    fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    fn finish(&self) {
        if self.tty && self.done() > 0 {
            eprintln!();
        }
    }
}
