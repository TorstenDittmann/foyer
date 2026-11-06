//! Processing a finalized build: download and verify its chunks, scan every
//! file with ClamAV, pack the zip for browser downloads, and mark it ready.
//!
//! A malware hit rejects the build; it is never served and shows up on the
//! game's submission in the review queue. Files clamd can't take (over its
//! stream limit) make the scan `incomplete`: the build still goes out, and
//! reviewers see the flag.

use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use foyer_types::{BuildScan, BuildState, Manifest, ScanFinding, ScanStatus};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::{JobError, WorkerConfig, app_error};
use crate::AppState;
use crate::clamd::{self, Verdict};
use crate::model::{self, BuildRecord, fmt_ts};
use crate::storage::Storage;

/// Payload of a `process_build` job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessBuild {
    pub build_id: Uuid,
}

/// Chunks downloaded in parallel.
const DOWNLOAD_CONCURRENCY: usize = 16;

pub(super) async fn process(
    state: &AppState,
    config: &WorkerConfig,
    build_id: Uuid,
) -> Result<(), JobError> {
    let conn = state.conn().await.map_err(app_error)?;
    let record = match model::build_by_id(&conn, build_id).await {
        Ok(record) => record,
        // Deleted together with its game; nothing to do.
        Err(crate::error::AppError::NotFound(_)) => return Ok(()),
        Err(err) => return Err(app_error(err)),
    };
    if record.build.state != BuildState::Processing {
        return Ok(());
    }
    let storage = state.storage().map_err(app_error)?;
    let manifest = load_manifest(&conn, build_id).await?;
    drop(conn);

    let work = tempfile::Builder::new()
        .prefix(&format!("foyer-build-{build_id}-"))
        .tempdir()?;
    let files = work.path().join("files");
    assemble(storage, record.game_id, &manifest, &files).await?;

    let scan = scan(config, &manifest, &files).await?;
    if scan.status == ScanStatus::Infected {
        tracing::warn!(build = %build_id, findings = ?scan.findings, "malware found; build rejected");
        return reject(state, &record, &scan).await;
    }

    let zip_path = work.path().join("build.zip");
    let root = model::archive_stem(&record.build);
    {
        let manifest = manifest.clone();
        let files = files.clone();
        let zip_path = zip_path.clone();
        tokio::task::spawn_blocking(move || pack(&manifest, &files, &zip_path, &root)).await??;
    }
    let archive_size = tokio::fs::metadata(&zip_path).await?.len();
    let key = Storage::archive_key(record.game_id, build_id);
    storage
        .upload_file(&key, &zip_path, "application/zip")
        .await?;

    promote(state, &record, &scan, archive_size).await
}

async fn load_manifest(conn: &libsql::Connection, id: Uuid) -> Result<Manifest, JobError> {
    let mut rows = conn
        .query(
            "SELECT manifest FROM builds WHERE id = ?1",
            [id.to_string()],
        )
        .await?;
    let row = rows
        .next()
        .await?
        .ok_or_else(|| JobError::Permanent("build disappeared".into()))?;
    let manifest: Manifest = model::json(&row, 0)?;
    manifest
        .validate()
        .map_err(|e| JobError::Permanent(format!("invalid manifest: {e}")))?;
    Ok(manifest)
}

/// How one chunk of the build gets into place.
enum Source {
    /// Download it from storage.
    Fetch,
    /// Copy it from where it was written earlier in the build.
    Copy { path: PathBuf, offset: u64 },
}

struct Piece {
    /// Index into the sorted file list.
    file: usize,
    hash: String,
    size: u64,
    source: Source,
}

/// Rebuilds every file of the build below `dir`, downloading each distinct
/// chunk once. Chunk and file hashes are checked, since storage only
/// confirmed sizes at finalize.
async fn assemble(
    storage: &Storage,
    game_id: Uuid,
    manifest: &Manifest,
    dir: &Path,
) -> Result<(), JobError> {
    let entries: Vec<_> = manifest.files.iter().collect();

    // Plan where each chunk comes from.
    let mut first_seen: HashMap<&str, (PathBuf, u64)> = HashMap::new();
    let mut pieces = Vec::new();
    for (index, (path, entry)) in entries.iter().enumerate() {
        let target = dir.join(path.as_str());
        let mut offset = 0;
        for chunk in &entry.chunks {
            let source = match first_seen.get(chunk.hash.as_str()) {
                Some((path, offset)) => Source::Copy {
                    path: path.clone(),
                    offset: *offset,
                },
                None => {
                    first_seen.insert(&chunk.hash, (target.clone(), offset));
                    Source::Fetch
                }
            };
            pieces.push(Piece {
                file: index,
                hash: chunk.hash.clone(),
                size: chunk.size,
                source,
            });
            offset += chunk.size;
        }
    }

    // Create every file up front, so empty files exist too.
    for (path, _) in &entries {
        let target = dir.join(path.as_str());
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::File::create(&target).await?;
    }

    // Download in parallel, write in order.
    let mut fetched = futures::stream::iter(pieces)
        .map(|piece| async move {
            let bytes = match piece.source {
                Source::Fetch => {
                    let bytes = storage
                        .get(&Storage::chunk_key(game_id, &piece.hash))
                        .await?;
                    if bytes.len() as u64 != piece.size
                        || foyer_patch::hash_bytes(&bytes) != piece.hash
                    {
                        return Err(JobError::Permanent(format!(
                            "chunk {} in storage does not match its hash; push the build again",
                            piece.hash
                        )));
                    }
                    Some(bytes)
                }
                Source::Copy { .. } => None,
            };
            Ok::<_, JobError>((piece, bytes))
        })
        .buffered(DOWNLOAD_CONCURRENCY);

    let mut current: Option<(usize, tokio::fs::File, blake3::Hasher)> = None;
    while let Some(result) = fetched.next().await {
        let (piece, bytes) = result?;
        if current
            .as_ref()
            .is_none_or(|(index, _, _)| *index != piece.file)
        {
            if let Some((index, file, hasher)) = current.take() {
                finish_file(entries[index].0, entries[index].1, file, hasher).await?;
            }
            let target = dir.join(entries[piece.file].0.as_str());
            let file = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&target)
                .await?;
            current = Some((piece.file, file, blake3::Hasher::new()));
        }
        let (_, file, hasher) = current.as_mut().expect("set above");
        let bytes = match (bytes, piece.source) {
            (Some(bytes), _) => bytes,
            (None, Source::Copy { path, offset }) => {
                // The source may be the file being written; flush it first.
                file.flush().await?;
                let mut source = tokio::fs::File::open(&path).await?;
                source.seek(std::io::SeekFrom::Start(offset)).await?;
                let mut bytes = vec![0; piece.size as usize];
                source.read_exact(&mut bytes).await?;
                bytes
            }
            (None, Source::Fetch) => unreachable!("fetched pieces carry bytes"),
        };
        hasher.update(&bytes);
        file.write_all(&bytes).await?;
    }
    if let Some((index, file, hasher)) = current.take() {
        finish_file(entries[index].0, entries[index].1, file, hasher).await?;
    }
    // Files without chunks are empty; their hash still has to match.
    for (path, entry) in &entries {
        if entry.chunks.is_empty() && blake3::hash(b"").to_hex().as_str() != entry.hash {
            return Err(JobError::Permanent(format!(
                "{path}: the manifest's file hash does not match its contents"
            )));
        }
    }
    Ok(())
}

async fn finish_file(
    path: &str,
    entry: &foyer_patch::FileEntry,
    mut file: tokio::fs::File,
    hasher: blake3::Hasher,
) -> Result<(), JobError> {
    file.flush().await?;
    if hasher.finalize().to_hex().as_str() != entry.hash {
        return Err(JobError::Permanent(format!(
            "{path}: the manifest's file hash does not match its contents"
        )));
    }
    Ok(())
}

async fn scan(
    config: &WorkerConfig,
    manifest: &Manifest,
    dir: &Path,
) -> Result<BuildScan, JobError> {
    let Some(addr) = &config.clamd_addr else {
        return Ok(BuildScan {
            status: ScanStatus::Skipped,
            findings: Vec::new(),
            scanned_at: model::now(),
        });
    };
    let mut infected = Vec::new();
    let mut unscanned = Vec::new();
    for (path, entry) in &manifest.files {
        if entry.size == 0 {
            continue;
        }
        match clamd::scan_file(addr, &dir.join(path)).await? {
            Verdict::Clean => {}
            Verdict::Found(signature) => infected.push(ScanFinding {
                path: path.clone(),
                detail: signature,
            }),
            Verdict::TooLarge => unscanned.push(ScanFinding {
                path: path.clone(),
                detail: "not scanned: larger than the scanner's limit".into(),
            }),
        }
    }
    let status = if !infected.is_empty() {
        ScanStatus::Infected
    } else if !unscanned.is_empty() {
        ScanStatus::Incomplete
    } else {
        ScanStatus::Clean
    };
    infected.extend(unscanned);
    Ok(BuildScan {
        status,
        findings: infected,
        scanned_at: model::now(),
    })
}

/// Extensions of formats that are already compressed; stored as-is.
const STORED: &[&str] = &[
    "7z", "aac", "avif", "br", "bz2", "flac", "gif", "gz", "jar", "jpeg", "jpg", "ktx2", "m4a",
    "mkv", "mov", "mp3", "mp4", "ogg", "ogv", "opus", "png", "rar", "webm", "webp", "xz", "zip",
    "zst",
];

/// Writes the zip. Everything goes under one folder named like the file, so
/// unpacking never spills into the player's Downloads folder.
fn pack(manifest: &Manifest, dir: &Path, zip_path: &Path, root: &str) -> anyhow::Result<()> {
    let out = std::fs::File::create(zip_path)?;
    let mut zip = ZipWriter::new(BufWriter::new(out));
    for (path, entry) in &manifest.files {
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        let method = if extension.is_some_and(|e| STORED.contains(&e.as_str())) {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .large_file(entry.size >= u64::from(u32::MAX))
            .unix_permissions(if entry.executable { 0o755 } else { 0o644 });
        zip.start_file(format!("{root}/{path}"), options)?;
        let mut file = std::fs::File::open(dir.join(path))?;
        std::io::copy(&mut file, &mut zip)?;
    }
    zip.finish()?.flush()?;
    Ok(())
}

/// Marks the build ready and points its channel at it, unless the channel
/// already serves a newer build. Only the channel's current build keeps a
/// zip; the one it replaces loses its archive.
async fn promote(
    state: &AppState,
    record: &BuildRecord,
    scan: &BuildScan,
    archive_size: u64,
) -> Result<(), JobError> {
    let storage = state.storage().map_err(app_error)?;
    let conn = state.conn().await.map_err(app_error)?;
    let build = &record.build;
    let now = fmt_ts(model::now());
    let tx = conn.transaction().await?;
    let updated = tx
        .execute(
            "UPDATE builds SET state = 'ready', scan_status = ?1, scan_findings = ?2,
               scanned_at = ?3, archive_size = ?4
             WHERE id = ?5 AND state = 'processing'",
            libsql::params![
                scan.status.as_str(),
                serde_json::to_string(&scan.findings)?,
                fmt_ts(scan.scanned_at),
                i64::try_from(archive_size)?,
                build.id.to_string(),
            ],
        )
        .await?;
    if updated == 0 {
        // Someone else finished it (a retried job racing a slow one).
        return Ok(());
    }
    let mut rows = tx
        .query(
            "SELECT c.current_build_id, cb.created_at FROM channels c
             LEFT JOIN builds cb ON cb.id = c.current_build_id
             WHERE c.game_id = ?1 AND c.name = ?2",
            [record.game_id.to_string(), build.channel.clone()],
        )
        .await?;
    let (previous, previous_created): (Option<String>, Option<String>) = match rows.next().await? {
        Some(row) => (row.get(0)?, row.get(1)?),
        None => (None, None),
    };
    drop(rows);
    let newer = previous_created.is_none_or(|p| p < fmt_ts(build.created_at));
    if newer {
        tx.execute(
            "UPDATE channels SET current_build_id = ?1, updated_at = ?2
             WHERE game_id = ?3 AND name = ?4",
            [
                build.id.to_string(),
                now,
                record.game_id.to_string(),
                build.channel.clone(),
            ],
        )
        .await?;
    }
    // The zip that is no longer served: the replaced build's, or this one's
    // if a newer build got there first.
    let stale = if newer {
        previous.filter(|p| *p != build.id.to_string())
    } else {
        Some(build.id.to_string())
    };
    if let Some(stale) = &stale {
        tx.execute(
            "UPDATE builds SET archive_size = NULL WHERE id = ?1",
            [stale.clone()],
        )
        .await?;
    }
    tx.commit().await?;

    if let Some(stale) = stale {
        let stale = Uuid::parse_str(&stale)?;
        if let Err(err) = storage
            .delete(&Storage::archive_key(record.game_id, stale))
            .await
        {
            // Only costs storage; the row no longer points at it.
            tracing::warn!(build = %stale, error = %err, "could not delete old archive");
        }
    }
    tracing::info!(build = %build.id, scan = scan.status.as_str(), archive_size, "build ready");
    Ok(())
}

async fn reject(state: &AppState, record: &BuildRecord, scan: &BuildScan) -> Result<(), JobError> {
    let conn = state.conn().await.map_err(app_error)?;
    conn.execute(
        "UPDATE builds SET state = 'rejected', scan_status = ?1, scan_findings = ?2,
           scanned_at = ?3, failure = ?4
         WHERE id = ?5 AND state = 'processing'",
        libsql::params![
            scan.status.as_str(),
            serde_json::to_string(&scan.findings)?,
            fmt_ts(scan.scanned_at),
            "the malware scan found a threat; contact the Foyer team if this is a false positive",
            record.build.id.to_string(),
        ],
    )
    .await?;
    Ok(())
}

/// Marks a build `failed` once its job gave up.
pub(super) async fn mark_failed(
    state: &AppState,
    build_id: Uuid,
    message: &str,
) -> anyhow::Result<()> {
    let conn = state.conn().await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
    conn.execute(
        "UPDATE builds SET state = 'failed', failure = ?1 WHERE id = ?2 AND state = 'processing'",
        [message.to_string(), build_id.to_string()],
    )
    .await?;
    Ok(())
}
