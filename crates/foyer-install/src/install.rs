//! Installs and updates a build into a folder, moving only changed chunks.
//!
//! The steps, all resumable:
//!
//! 1. **Plan.** Fetch the target build's manifest and compare it with the
//!    manifest of what is installed. Files that are new or changed have to be
//!    written; their chunks come from three places, cheapest first: chunks
//!    already downloaded by an earlier, interrupted run; the installed files
//!    themselves (an update to one asset reuses every unchanged chunk); and
//!    storage.
//! 2. **Download** the chunks nobody has, verified by BLAKE3, into
//!    `.foyer-client/chunks/`.
//! 3. **Stage** every new or changed file in `.foyer-client/staged/` and check
//!    its hash. The game folder is untouched until here, so the old build
//!    keeps working while an update downloads.
//! 4. **Swap** the staged files into place, delete files the new build
//!    dropped, and record the new manifest. A marker file makes this step
//!    restart safely after a crash.
//!
//! Stopping (pause) between chunks or files keeps everything downloaded so
//! far; running the job again picks up where it stopped. Files in the game
//! folder that no manifest lists (saves, settings) are never touched.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use foyer_patch::{BuildSource, Chunk, Manifest, SourceError, hash_bytes};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::{Api, ApiFailure, DownloadEvent, FetchError};
use crate::library::write_atomic;

/// Folder inside every install that holds Foyer's own state.
pub const STATE_DIR: &str = ".foyer-client";

/// Chunks downloaded at once.
const CONCURRENCY: usize = 6;
/// Chunk URLs requested per API call. Small enough that URLs do not expire
/// before they are used.
const URL_BATCH: usize = 256;
const MAX_ATTEMPTS: u32 = 5;

/// What the job is doing right now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Preparing,
    Downloading,
    Installing,
    Finishing,
}

/// A point-in-time view of a running job, for the UI.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    /// Bytes downloaded (or, while installing, written) so far.
    pub done: u64,
    /// Bytes to download (or write) in this phase.
    pub total: u64,
    /// Average speed of the current phase in bytes per second.
    pub bytes_per_sec: f64,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f64 / self.total as f64).clamp(0.0, 1.0) as f32
        }
    }
}

/// Shared between the job and whoever shows its progress or stops it.
#[derive(Debug, Clone, Default)]
pub struct JobHandle {
    progress: Arc<Mutex<(Progress, Option<Instant>)>>,
    stop: Arc<AtomicBool>,
}

impl JobHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().expect("progress lock").0.clone()
    }

    /// Ask the job to stop at the next chunk or file. Everything downloaded
    /// so far is kept, so the job can resume.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn check(&self) -> Result<(), InstallError> {
        if self.is_stopped() {
            Err(InstallError::Stopped)
        } else {
            Ok(())
        }
    }

    fn start_phase(&self, phase: Phase, total: u64) {
        let mut guard = self.progress.lock().expect("progress lock");
        *guard = (
            Progress {
                phase,
                done: 0,
                total,
                bytes_per_sec: 0.0,
            },
            Some(Instant::now()),
        );
    }

    fn advance(&self, bytes: u64) {
        let mut guard = self.progress.lock().expect("progress lock");
        let (progress, started) = &mut *guard;
        progress.done += bytes;
        if let Some(started) = started {
            let secs = started.elapsed().as_secs_f64();
            if secs > 0.2 {
                progress.bytes_per_sec = progress.done as f64 / secs;
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("stopped")]
    Stopped,
    #[error(transparent)]
    Api(#[from] ApiFailure),
    #[error("download failed: {0}")]
    Download(String),
    #[error("the build's file list is invalid: {0}")]
    BadManifest(String),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0} does not look like a Foyer install; refusing to change it")]
    NotOurs(PathBuf),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> InstallError + '_ {
    move |source| InstallError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// What to install.
#[derive(Debug, Clone)]
pub struct InstallJob {
    pub install_dir: PathBuf,
    pub build_id: Uuid,
    /// Counted by the API on the first chunk request. `None` for repairs.
    pub event: Option<DownloadEvent>,
    /// Re-hash every installed file and replace the ones that differ.
    pub repair: bool,
}

/// What ended up on disk.
#[derive(Debug, Clone)]
pub struct Installed {
    pub build_id: Uuid,
    pub manifest: Manifest,
    /// Bytes fetched from storage by this run.
    pub downloaded: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BuildManifest {
    build_id: Uuid,
    manifest: Manifest,
}

/// Paths of the state kept inside an install.
struct State {
    root: PathBuf,
    dir: PathBuf,
}

impl State {
    fn new(install_dir: &Path) -> Self {
        Self {
            root: install_dir.to_path_buf(),
            dir: install_dir.join(STATE_DIR),
        }
    }
    fn installed(&self) -> PathBuf {
        self.dir.join("installed.json")
    }
    fn target(&self) -> PathBuf {
        self.dir.join("target.json")
    }
    fn swapping(&self) -> PathBuf {
        self.dir.join("swapping")
    }
    fn chunks(&self) -> PathBuf {
        self.dir.join("chunks")
    }
    fn chunk(&self, hash: &str) -> PathBuf {
        self.chunks().join(hash)
    }
    fn staged(&self) -> PathBuf {
        self.dir.join("staged")
    }
    fn file(&self, base: &Path, path: &str) -> PathBuf {
        let mut out = base.to_path_buf();
        out.extend(path.split('/'));
        out
    }

    fn read(&self, path: &Path) -> Result<Option<BuildManifest>, InstallError> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(io_err(path)(err)),
        }
    }

    fn write(&self, path: &Path, value: &BuildManifest) -> Result<(), InstallError> {
        let json = serde_json::to_vec(value).expect("manifest serializes");
        write_atomic(path, &json).map_err(io_err(path))
    }
}

/// The manifest of the build installed in `install_dir`, if any.
pub fn installed_manifest(install_dir: &Path) -> Option<(Uuid, Manifest)> {
    let state = State::new(install_dir);
    state
        .read(&state.installed())
        .ok()
        .flatten()
        .map(|b| (b.build_id, b.manifest))
}

/// Install or update `job.install_dir` to `job.build_id`.
pub async fn install(
    api: &Api,
    job: &InstallJob,
    handle: &JobHandle,
) -> Result<Installed, InstallError> {
    let state = State::new(&job.install_dir);
    handle.start_phase(Phase::Preparing, 0);
    fs::create_dir_all(&state.dir).map_err(io_err(&state.dir))?;

    let installed = state.read(&state.installed())?;

    // A swap that was interrupted finishes first, whatever was asked.
    if state.swapping().exists()
        && let Some(target) = state.read(&state.target())?
    {
        let plan = plan_files(&state, installed.as_ref(), &target.manifest, false)?;
        swap(&state, &target, &plan, handle)?;
        if target.build_id == job.build_id && !job.repair {
            return Ok(Installed {
                build_id: target.build_id,
                manifest: target.manifest,
                downloaded: 0,
            });
        }
        return Box::pin(install(api, job, handle)).await;
    }

    let target = match state.read(&state.target())? {
        Some(t) if t.build_id == job.build_id => t,
        _ => {
            let manifest = api.manifest(job.build_id).await?;
            check_manifest(&manifest)?;
            // Staged files belong to the previous target; chunks are
            // content-addressed and stay useful.
            remove_dir_if_exists(&state.staged())?;
            let target = BuildManifest {
                build_id: job.build_id,
                manifest,
            };
            state.write(&state.target(), &target)?;
            target
        }
    };
    handle.check()?;

    let plan = plan_files(&state, installed.as_ref(), &target.manifest, job.repair)?;

    // Where each needed chunk can be read locally.
    let cache = state.chunks();
    fs::create_dir_all(&cache).map_err(io_err(&cache))?;
    let cached: HashSet<String> = fs::read_dir(&cache)
        .map_err(io_err(&cache))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| !name.ends_with(".part"))
        .collect();
    let local = installed
        .as_ref()
        .map(|i| LocalSource::new(&state.root, &i.manifest, &plan.trusted));

    let mut needed: BTreeMap<String, u64> = BTreeMap::new();
    for path in &plan.write {
        if state.file(&state.staged(), path).exists() {
            continue;
        }
        for chunk in &target.manifest.files[path].chunks {
            needed.insert(chunk.hash.clone(), chunk.size);
        }
    }
    let missing: Vec<Chunk> = needed
        .into_iter()
        .filter(|(hash, _)| !cached.contains(hash) && !local.as_ref().is_some_and(|l| l.has(hash)))
        .map(|(hash, size)| Chunk { hash, size })
        .collect();

    let downloaded = download(api, job, &state, &missing, handle).await?;

    stage(api, job, &state, &target, &plan, local, handle).await?;

    // Last chance to stop: once the marker exists the swap has to finish.
    handle.check()?;
    fs::write(state.swapping(), b"").map_err(io_err(&state.swapping()))?;
    swap(&state, &target, &plan, handle)?;

    Ok(Installed {
        build_id: target.build_id,
        manifest: target.manifest,
        downloaded,
    })
}

fn check_manifest(manifest: &Manifest) -> Result<(), InstallError> {
    manifest
        .validate()
        .map_err(|err| InstallError::BadManifest(err.to_string()))?;
    if let Some(path) = manifest
        .files
        .keys()
        .find(|p| p.split('/').next() == Some(STATE_DIR))
    {
        return Err(InstallError::BadManifest(format!(
            "{path} clashes with the client's own folder"
        )));
    }
    Ok(())
}

struct FilePlan {
    /// Files to (re)write, sorted.
    write: Vec<String>,
    /// Installed files that the new build no longer has.
    remove: Vec<String>,
    /// Installed files whose size matches the installed manifest (and, when
    /// repairing, whose hash does too): usable as a chunk source.
    trusted: HashSet<String>,
}

fn plan_files(
    state: &State,
    installed: Option<&BuildManifest>,
    target: &Manifest,
    repair: bool,
) -> Result<FilePlan, InstallError> {
    let empty = Manifest::default();
    let old = installed.map(|i| &i.manifest).unwrap_or(&empty);
    let diff = foyer_patch::diff(old, target);

    let mut trusted = HashSet::new();
    for (path, entry) in &old.files {
        let file = state.file(&state.root, path);
        let ok = match fs::metadata(&file) {
            Ok(meta) => meta.is_file() && meta.len() == entry.size,
            Err(_) => false,
        };
        let ok = ok && (!repair || hash_file(&file).ok().as_deref() == Some(entry.hash.as_str()));
        if ok {
            trusted.insert(path.clone());
        }
    }

    let mut write: Vec<String> = diff.added.into_iter().chain(diff.changed).collect();
    // An unchanged file that is missing or damaged on disk is written again.
    write.extend(
        diff.unchanged
            .into_iter()
            .filter(|path| !trusted.contains(path)),
    );
    write.sort();
    Ok(FilePlan {
        write,
        remove: diff.removed,
        trusted,
    })
}

/// Chunks that can be read out of the installed files.
struct LocalSource {
    source: BuildSource,
    hashes: HashSet<String>,
}

impl LocalSource {
    fn new(root: &Path, manifest: &Manifest, trusted: &HashSet<String>) -> Self {
        let usable = Manifest {
            chunker: manifest.chunker,
            files: manifest
                .files
                .iter()
                .filter(|(path, _)| trusted.contains(*path))
                .map(|(p, f)| (p.clone(), f.clone()))
                .collect(),
        };
        Self {
            hashes: usable.unique_chunks().into_keys().collect(),
            source: BuildSource::new(root, &usable),
        }
    }

    fn has(&self, hash: &str) -> bool {
        self.hashes.contains(hash)
    }
}

/// Download `missing` into the chunk cache. Returns the bytes fetched.
async fn download(
    api: &Api,
    job: &InstallJob,
    state: &State,
    missing: &[Chunk],
    handle: &JobHandle,
) -> Result<u64, InstallError> {
    let total: u64 = missing.iter().map(|c| c.size).sum();
    handle.start_phase(Phase::Downloading, total);
    let mut event = job.event;

    for batch in missing.chunks(URL_BATCH) {
        handle.check()?;
        let hashes: Vec<String> = batch.iter().map(|c| c.hash.clone()).collect();
        let urls = with_retries(|| api.download_urls(job.build_id, &hashes, event)).await?;
        event = None;
        let urls: HashMap<String, String> = urls
            .downloads
            .into_iter()
            .map(|d| (d.hash, d.url))
            .collect();

        let build_id = job.build_id;
        let fetches: Vec<_> = batch
            .iter()
            .cloned()
            .map(|chunk| {
                let url = urls.get(&chunk.hash).cloned();
                async move {
                    let url = url.ok_or_else(|| {
                        InstallError::Download(format!("no download link for chunk {}", chunk.hash))
                    })?;
                    fetch_chunk(api, build_id, state, &chunk, url, handle).await
                }
            })
            .collect();
        let mut tasks = futures::stream::iter(fetches).buffer_unordered(CONCURRENCY);
        while let Some(result) = tasks.next().await {
            result?;
        }
    }
    Ok(total)
}

async fn with_retries<T, F, Fut>(mut call: F) -> Result<T, ApiFailure>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ApiFailure>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match call().await {
            Ok(value) => return Ok(value),
            Err(err) if err.is_transient() && attempt < MAX_ATTEMPTS => {
                let wait = match &err {
                    ApiFailure::RateLimited(wait) => *wait,
                    _ => backoff(attempt),
                };
                tracing::warn!("API request failed ({err}); retrying in {wait:?}");
                tokio::time::sleep(wait).await;
            }
            Err(err) => return Err(err),
        }
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500 * 2u64.pow(attempt.min(5)))
}

/// Fetch one chunk into the cache, verifying it and refreshing an expired
/// URL.
async fn fetch_chunk(
    api: &Api,
    build_id: Uuid,
    state: &State,
    chunk: &Chunk,
    mut url: String,
    handle: &JobHandle,
) -> Result<(), InstallError> {
    let mut attempt = 0;
    let bytes = loop {
        handle.check()?;
        attempt += 1;
        let err = match api.fetch(&url).await {
            Ok(bytes) if bytes.len() as u64 == chunk.size && hash_bytes(&bytes) == chunk.hash => {
                break bytes;
            }
            Ok(_) => FetchError::Transient(format!("chunk {} arrived damaged", chunk.hash)),
            Err(err) => err,
        };
        if attempt >= MAX_ATTEMPTS || matches!(err, FetchError::Fatal(_)) {
            return Err(InstallError::Download(err.to_string()));
        }
        match err {
            FetchError::Expired => {
                let fresh = with_retries(|| {
                    api.download_urls(build_id, std::slice::from_ref(&chunk.hash), None)
                })
                .await?;
                url = fresh
                    .downloads
                    .into_iter()
                    .next()
                    .map(|d| d.url)
                    .ok_or_else(|| InstallError::Download("no download link".into()))?;
            }
            FetchError::RateLimited(wait) => tokio::time::sleep(wait).await,
            _ => tokio::time::sleep(backoff(attempt)).await,
        }
    };

    // Written synchronously (chunks are at most a few MiB): a blocking
    // task would keep writing after a stop and race `discard`.
    let path = state.chunk(&chunk.hash);
    let part = path.with_extension("part");
    fs::write(&part, &bytes).map_err(io_err(&part))?;
    fs::rename(&part, &path).map_err(io_err(&path))?;
    handle.advance(chunk.size);
    Ok(())
}

/// Assemble every file in `plan.write` into the staging folder.
async fn stage(
    api: &Api,
    job: &InstallJob,
    state: &State,
    target: &BuildManifest,
    plan: &FilePlan,
    local: Option<LocalSource>,
    handle: &JobHandle,
) -> Result<(), InstallError> {
    let total: u64 = plan
        .write
        .iter()
        .map(|p| target.manifest.files[p].size)
        .sum();
    handle.start_phase(Phase::Installing, total);
    let local = Arc::new(local);

    for path in &plan.write {
        handle.check()?;
        let entry = target.manifest.files[path].clone();
        let dest = state.file(&state.staged(), path);
        loop {
            let task = {
                let dest = dest.clone();
                let entry = entry.clone();
                let local = local.clone();
                let cache = state.chunks();
                tokio::task::spawn_blocking(move || {
                    assemble(&dest, &entry, &cache, local.as_ref().as_ref())
                })
            };
            match task
                .await
                .map_err(|e| InstallError::Download(e.to_string()))?
            {
                Ok(()) => break,
                // An installed file was not what its manifest says; fetch
                // the chunk from storage instead and try again.
                Err(AssembleError::Missing(chunk)) => {
                    let urls = with_retries(|| {
                        api.download_urls(target.build_id, std::slice::from_ref(&chunk.hash), None)
                    })
                    .await?;
                    let url = urls
                        .downloads
                        .into_iter()
                        .next()
                        .map(|d| d.url)
                        .ok_or_else(|| InstallError::Download("no download link".into()))?;
                    fetch_chunk(api, job.build_id, state, &chunk, url, &JobHandle::new()).await?;
                }
                Err(AssembleError::Io(err)) => return Err(err),
            }
        }
        handle.advance(entry.size);
    }
    Ok(())
}

enum AssembleError {
    Missing(Chunk),
    Io(InstallError),
}

fn assemble(
    dest: &Path,
    entry: &foyer_patch::FileEntry,
    cache: &Path,
    local: Option<&LocalSource>,
) -> Result<(), AssembleError> {
    // Already staged by an earlier run?
    if fs::metadata(dest).is_ok_and(|m| m.len() == entry.size)
        && hash_file(dest).ok().as_deref() == Some(entry.hash.as_str())
    {
        return Ok(());
    }
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| AssembleError::Io(InstallError::Io { path, source })
    };
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(io(parent))?;
    }
    let part = dest.with_file_name(format!(
        "{}.part",
        dest.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    let mut out = File::create(&part).map_err(io(&part))?;
    let mut hasher = foyer_patch_hasher();
    let mut head = Vec::new();

    for chunk in &entry.chunks {
        let cached = cache.join(&chunk.hash);
        let bytes = match fs::read(&cached) {
            Ok(bytes) if hash_bytes(&bytes) == chunk.hash => bytes,
            _ => match local.map(|l| l.source.read_chunk(&chunk.hash)) {
                Some(Ok(bytes)) => bytes,
                Some(Err(SourceError::Io { .. } | SourceError::Changed { .. }))
                | Some(Err(SourceError::UnknownChunk(_)))
                | None => {
                    let _ = fs::remove_file(&cached);
                    return Err(AssembleError::Missing(chunk.clone()));
                }
            },
        };
        if head.len() < 4 {
            head.extend_from_slice(&bytes[..bytes.len().min(4)]);
        }
        hasher.update(&bytes);
        out.write_all(&bytes).map_err(io(&part))?;
    }
    out.sync_all().map_err(io(&part))?;
    drop(out);

    if hasher.finalize().to_hex().as_str() != entry.hash {
        let _ = fs::remove_file(&part);
        return Err(AssembleError::Io(InstallError::Download(format!(
            "{} did not match its hash after assembly",
            dest.display()
        ))));
    }
    // Builds pushed from Windows carry no exec bits, so also sniff.
    if entry.executable || looks_executable(&head, dest) {
        crate::launch::make_executable(&part).map_err(io(&part))?;
    }
    fs::rename(&part, dest).map_err(io(dest))?;
    Ok(())
}

/// File hashes in manifests are BLAKE3, like chunk hashes.
fn foyer_patch_hasher() -> blake3::Hasher {
    blake3::Hasher::new()
}

/// Unix binaries and scripts lose their executable bit in the manifest, so
/// recognise them by their first bytes.
fn looks_executable(head: &[u8], path: &Path) -> bool {
    const MAGIC: [&[u8]; 6] = [
        b"\x7fELF",
        b"#!",
        &[0xfe, 0xed, 0xfa, 0xce],
        &[0xfe, 0xed, 0xfa, 0xcf],
        &[0xce, 0xfa, 0xed, 0xfe],
        &[0xcf, 0xfa, 0xed, 0xfe],
    ];
    MAGIC.iter().any(|m| head.starts_with(m))
        || (head.starts_with(&[0xca, 0xfe, 0xba, 0xbe])
            && path.to_string_lossy().contains(".app/Contents/MacOS/"))
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = foyer_patch_hasher();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Move staged files into place and record the new build.
fn swap(
    state: &State,
    target: &BuildManifest,
    plan: &FilePlan,
    handle: &JobHandle,
) -> Result<(), InstallError> {
    handle.start_phase(Phase::Finishing, plan.write.len() as u64);

    for path in &plan.remove {
        let file = state.file(&state.root, path);
        match fs::remove_file(&file) {
            Ok(()) => prune_empty_parents(&state.root, &file),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(io_err(&file)(err)),
        }
    }
    for path in &plan.write {
        let staged = state.file(&state.staged(), path);
        let dest = state.file(&state.root, path);
        if staged.exists() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(io_err(parent))?;
            }
            if dest.is_dir() {
                fs::remove_dir_all(&dest).map_err(io_err(&dest))?;
            }
            fs::rename(&staged, &dest).map_err(io_err(&dest))?;
        } else if !dest.is_file() {
            // Neither staged nor in place: the staging folder was lost.
            return Err(InstallError::Download(format!(
                "{path} is missing; resume the install to fetch it again"
            )));
        }
        handle.advance(1);
    }

    state.write(&state.installed(), target)?;
    for path in [state.target(), state.swapping()] {
        let _ = fs::remove_file(path);
    }
    remove_dir_if_exists(&state.staged())?;
    remove_dir_if_exists(&state.chunks())?;
    Ok(())
}

fn prune_empty_parents(root: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d == root || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

fn remove_dir_if_exists(dir: &Path) -> Result<(), InstallError> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(io_err(dir)(err)),
    }
}

/// Throw away a paused install or update. The installed build, if any,
/// stays; a game that was never fully installed is removed.
pub fn discard(install_dir: &Path) -> Result<(), InstallError> {
    let state = State::new(install_dir);
    if state.swapping().exists() {
        // Half swapped: the folder is a mix of builds. Keep the target so
        // the next run finishes the swap instead of leaving it broken.
        return Ok(());
    }
    if state.read(&state.installed())?.is_none() {
        return uninstall(install_dir);
    }
    let _ = fs::remove_file(state.target());
    remove_dir_if_exists(&state.staged())?;
    remove_dir_if_exists(&state.chunks())
}

/// Delete an install folder. Refuses folders the client did not create.
pub fn uninstall(install_dir: &Path) -> Result<(), InstallError> {
    if !install_dir.exists() {
        return Ok(());
    }
    if !install_dir.join(STATE_DIR).is_dir() {
        return Err(InstallError::NotOurs(install_dir.to_path_buf()));
    }
    fs::remove_dir_all(install_dir).map_err(io_err(install_dir))
}

/// Files in an install whose contents do not match the installed manifest.
pub fn verify(install_dir: &Path) -> Result<Vec<String>, InstallError> {
    let state = State::new(install_dir);
    let installed = state
        .read(&state.installed())?
        .ok_or_else(|| InstallError::NotOurs(install_dir.to_path_buf()))?;
    Ok(installed
        .manifest
        .files
        .iter()
        .filter(|(path, entry)| {
            hash_file(&state.file(&state.root, path)).ok().as_deref() != Some(entry.hash.as_str())
        })
        .map(|(path, _)| path.clone())
        .collect())
}

/// Bytes an update from the installed build to `target` has to download,
/// assuming the installed files are intact.
pub fn update_download_size(install_dir: &Path, target: &Manifest) -> u64 {
    match installed_manifest(install_dir) {
        Some((_, installed)) => foyer_patch::chunk_delta(&installed, target)
            .iter()
            .map(|c| c.size)
            .sum(),
        None => target.unique_chunks().values().sum(),
    }
}
