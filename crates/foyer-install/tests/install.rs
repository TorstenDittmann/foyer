//! Install, update, pause/resume, repair and uninstall against an in-process
//! stand-in for the API and storage. The mock speaks raw JSON so these tests
//! also pin the wire format.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use foyer_install::api::InstalledBuild;
use foyer_install::install::{self, STATE_DIR};
use foyer_install::{Api, DownloadEvent, InstallError, InstallJob, JobHandle};
use foyer_patch::{ChunkerConfig, Manifest};
use serde_json::{Value, json};
use uuid::Uuid;

fn next_id() -> Uuid {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    Uuid::from_u128(NEXT.fetch_add(1, Ordering::SeqCst) as u128)
}

fn small_chunks() -> ChunkerConfig {
    ChunkerConfig {
        min_size: 2 * 1024,
        avg_size: 8 * 1024,
        max_size: 32 * 1024,
    }
}

fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

#[derive(Default)]
struct Store {
    chunks: HashMap<String, Vec<u8>>,
    manifests: HashMap<Uuid, Value>,
    /// Channel name -> current build id.
    channels: HashMap<String, Uuid>,
    /// Chunk GETs served, by hash.
    gets: Vec<String>,
    /// `event` values of download-link requests.
    events: Vec<Option<String>>,
    /// Hashes requested by download-link calls.
    link_requests: usize,
}

#[derive(Default)]
struct Faults {
    /// Answer this many chunk GETs with 403 (expired link).
    expire: AtomicUsize,
    /// Answer this many chunk GETs with 500.
    fail: AtomicUsize,
}

#[derive(Clone)]
struct Mock {
    url: String,
    store: Arc<Mutex<Store>>,
    faults: Arc<Faults>,
    /// Stop the job once this many chunks were served.
    stop_after: Arc<Mutex<Option<(usize, JobHandle)>>>,
}

fn start() -> Mock {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mock = Mock {
        url: format!("http://{}", listener.local_addr().unwrap()),
        store: Arc::default(),
        faults: Arc::default(),
        stop_after: Arc::default(),
    };
    let app = Router::new()
        .route("/v1/games/{game}/channels", get(channels))
        .route("/v1/builds/{id}/manifest", get(manifest))
        .route("/v1/builds/{id}/downloads", post(downloads))
        .route("/storage/{hash}", get(chunk))
        .with_state(mock.clone());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    mock
}

impl Mock {
    /// Publish the contents of `dir` as a build on the `linux` channel.
    fn publish(&self, dir: &Path) -> (Uuid, Manifest) {
        let manifest = Manifest::from_dir_with(dir, small_chunks()).unwrap();
        let id = next_id();
        let mut store = self.store.lock().unwrap();
        for (path, file) in &manifest.files {
            let data = fs::read(dir.join(path)).unwrap();
            let mut offset = 0;
            for chunk in &file.chunks {
                let end = offset + chunk.size as usize;
                store
                    .chunks
                    .insert(chunk.hash.clone(), data[offset..end].to_vec());
                offset = end;
            }
        }
        store
            .manifests
            .insert(id, serde_json::to_value(&manifest).unwrap());
        store.channels.insert("linux".into(), id);
        (id, manifest)
    }

    fn gets(&self) -> usize {
        self.store.lock().unwrap().gets.len()
    }

    fn api(&self) -> Api {
        Api::new(&self.url).unwrap()
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "not_found", "message": "not found" })),
    )
        .into_response()
}

async fn channels(State(mock): State<Mock>, UrlPath(game): UrlPath<String>) -> Response {
    if game != "moss" {
        return not_found();
    }
    let store = mock.store.lock().unwrap();
    let out: Vec<Value> = store
        .channels
        .iter()
        .map(|(name, id)| {
            json!({
                "channel": name,
                "platform": "linux",
                "latest": {
                    "id": id, "game": "moss", "channel": name, "platform": "linux",
                    "version": "1.0", "state": "ready", "total_size": 1, "chunk_count": 1,
                    "created_at": "2026-11-06T00:00:00Z"
                }
            })
        })
        .collect();
    Json(out).into_response()
}

async fn manifest(State(mock): State<Mock>, UrlPath(id): UrlPath<Uuid>) -> Response {
    match mock.store.lock().unwrap().manifests.get(&id) {
        Some(m) => Json(m.clone()).into_response(),
        None => not_found(),
    }
}

async fn downloads(
    State(mock): State<Mock>,
    UrlPath(id): UrlPath<Uuid>,
    Json(body): Json<Value>,
) -> Response {
    let mut store = mock.store.lock().unwrap();
    if !store.manifests.contains_key(&id) {
        return not_found();
    }
    let hashes: Vec<String> = body["hashes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h.as_str().unwrap().to_string())
        .collect();
    assert!(!hashes.is_empty(), "an empty list would mean every chunk");
    store
        .events
        .push(body.get("event").and_then(|e| e.as_str()).map(String::from));
    store.link_requests += 1;
    let downloads: Vec<Value> = hashes
        .iter()
        .map(|h| {
            json!({
                "hash": h,
                "size": store.chunks[h].len(),
                "url": format!("{}/storage/{h}", mock.url),
            })
        })
        .collect();
    Json(json!({ "downloads": downloads, "expires_at": "2026-11-06T01:00:00Z" })).into_response()
}

async fn chunk(State(mock): State<Mock>, UrlPath(hash): UrlPath<String>) -> Response {
    if mock
        .faults
        .expire
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if mock
        .faults
        .fail
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let mut store = mock.store.lock().unwrap();
    let Some(bytes) = store.chunks.get(&hash).cloned() else {
        return not_found();
    };
    store.gets.push(hash);
    if let Some((limit, handle)) = &*mock.stop_after.lock().unwrap()
        && store.gets.len() >= *limit
    {
        handle.stop();
    }
    bytes.into_response()
}

fn job(dir: &Path, build_id: Uuid, event: DownloadEvent) -> InstallJob {
    InstallJob {
        install_dir: dir.to_path_buf(),
        build_id,
        event: Some(event),
        repair: false,
    }
}

/// Every manifest file in `dir` has the manifest's hash.
fn assert_matches(dir: &Path, manifest: &Manifest) {
    for (path, entry) in &manifest.files {
        let data = fs::read(dir.join(path)).unwrap_or_else(|_| panic!("{path} missing"));
        assert_eq!(foyer_patch::hash_bytes(&data), entry.hash, "{path}");
    }
}

fn write_build_v1(dir: &Path) {
    fs::create_dir_all(dir.join("data")).unwrap();
    let mut elf = b"\x7fELF".to_vec();
    elf.extend(noise(1, 20_000));
    fs::write(dir.join("moss"), elf).unwrap();
    fs::write(dir.join("data/assets.pak"), noise(2, 300_000)).unwrap();
    fs::write(dir.join("data/old.txt"), b"going away").unwrap();
    fs::write(dir.join("readme.txt"), b"hello").unwrap();
}

#[tokio::test]
async fn installs_then_updates_with_only_changed_chunks() {
    let mock = start();
    let src = tempfile::tempdir().unwrap();
    write_build_v1(src.path());
    let (v1, m1) = mock.publish(src.path());

    let games = tempfile::tempdir().unwrap();
    let dir = games.path().join("moss");
    let handle = JobHandle::new();
    let api = mock.api();

    let done = install::install(&api, &job(&dir, v1, DownloadEvent::Install), &handle)
        .await
        .unwrap();
    assert_eq!(done.build_id, v1);
    assert_matches(&dir, &m1);
    assert_eq!(mock.gets(), m1.unique_chunks().len());
    assert_eq!(done.downloaded, m1.unique_chunks().values().sum::<u64>());
    #[cfg(unix)]
    {
        assert!(foyer_install::launch::has_exec_bit(&dir.join("moss")));
        assert!(!foyer_install::launch::has_exec_bit(
            &dir.join("readme.txt")
        ));
    }
    assert_eq!(install::installed_manifest(&dir).unwrap().0, v1);
    assert!(!dir.join(STATE_DIR).join("chunks").exists());

    // The player's own files survive updates.
    fs::write(dir.join("save.dat"), b"level 3").unwrap();

    // v2: patch the middle of the pak, drop a file, add one.
    let mut pak = fs::read(src.path().join("data/assets.pak")).unwrap();
    pak.splice(150_000..150_000, *b"patched!");
    fs::write(src.path().join("data/assets.pak"), &pak).unwrap();
    fs::remove_file(src.path().join("data/old.txt")).unwrap();
    fs::write(src.path().join("data/new.txt"), b"brand new").unwrap();
    let (v2, m2) = mock.publish(src.path());

    let expected = install::update_download_size(&dir, &m2);
    let before = mock.gets();
    let done = install::install(&api, &job(&dir, v2, DownloadEvent::Update), &handle)
        .await
        .unwrap();
    let fetched = mock.gets() - before;
    assert_matches(&dir, &m2);
    assert!(!dir.join("data/old.txt").exists());
    assert_eq!(fs::read(dir.join("save.dat")).unwrap(), b"level 3");
    assert_eq!(done.downloaded, expected);
    assert!(
        fetched <= 4,
        "update fetched {fetched} of {} chunks",
        m2.unique_chunks().len()
    );

    let events = mock.store.lock().unwrap().events.clone();
    assert_eq!(events.first().unwrap().as_deref(), Some("install"));
    assert_eq!(
        events.iter().filter(|e| e.is_some()).count(),
        2,
        "one counted event per install or update: {events:?}"
    );
    assert!(events.contains(&Some("update".into())));
}

#[tokio::test]
async fn pausing_keeps_downloaded_chunks_for_resume() {
    let mock = start();
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("big.pak"), noise(5, 600_000)).unwrap();
    let (v1, m1) = mock.publish(src.path());
    let total = m1.unique_chunks().len();
    assert!(total > 20);

    let dir = tempfile::tempdir().unwrap().path().join("moss");
    let handle = JobHandle::new();
    *mock.stop_after.lock().unwrap() = Some((10, handle.clone()));
    let api = mock.api();

    let err = install::install(&api, &job(&dir, v1, DownloadEvent::Install), &handle)
        .await
        .unwrap_err();
    assert!(matches!(err, InstallError::Stopped), "{err}");
    assert!(!dir.join("big.pak").exists(), "nothing is swapped in early");
    let served = mock.gets();
    assert!(served < total);

    *mock.stop_after.lock().unwrap() = None;
    install::install(
        &api,
        &job(&dir, v1, DownloadEvent::Install),
        &JobHandle::new(),
    )
    .await
    .unwrap();
    assert_matches(&dir, &m1);
    // Chunks fetched before the pause (minus any in flight when it hit) are
    // not fetched again.
    assert!(
        mock.gets() <= total + in_flight(),
        "{} gets for {total} chunks",
        mock.gets()
    );
}

/// Downloads that may complete after a stop was requested.
fn in_flight() -> usize {
    6
}

#[tokio::test]
async fn recovers_from_expired_links_and_storage_errors() {
    let mock = start();
    let src = tempfile::tempdir().unwrap();
    write_build_v1(src.path());
    let (v1, m1) = mock.publish(src.path());
    mock.faults.expire.store(2, Ordering::SeqCst);
    mock.faults.fail.store(2, Ordering::SeqCst);

    let dir = tempfile::tempdir().unwrap().path().join("moss");
    install::install(
        &mock.api(),
        &job(&dir, v1, DownloadEvent::Install),
        &JobHandle::new(),
    )
    .await
    .unwrap();
    assert_matches(&dir, &m1);
    // The two expired links were refreshed one by one.
    assert!(mock.store.lock().unwrap().link_requests >= 3);
}

#[tokio::test]
async fn repair_replaces_damaged_files_and_update_survives_them() {
    let mock = start();
    let src = tempfile::tempdir().unwrap();
    write_build_v1(src.path());
    let (v1, m1) = mock.publish(src.path());
    let dir = tempfile::tempdir().unwrap().path().join("moss");
    let api = mock.api();
    install::install(
        &api,
        &job(&dir, v1, DownloadEvent::Install),
        &JobHandle::new(),
    )
    .await
    .unwrap();

    // Same size, different bytes: only a full hash check notices.
    let mut pak = fs::read(dir.join("data/assets.pak")).unwrap();
    pak[1000] ^= 0xff;
    fs::write(dir.join("data/assets.pak"), &pak).unwrap();
    fs::remove_file(dir.join("readme.txt")).unwrap();
    let broken = install::verify(&dir).unwrap();
    assert_eq!(broken, ["data/assets.pak", "readme.txt"]);

    let repair = InstallJob {
        repair: true,
        event: None,
        ..job(&dir, v1, DownloadEvent::Install)
    };
    install::install(&api, &repair, &JobHandle::new())
        .await
        .unwrap();
    assert_matches(&dir, &m1);
    assert!(install::verify(&dir).unwrap().is_empty());

    // An update that wants to reuse chunks from a damaged file falls back
    // to downloading them.
    let mut pak = fs::read(dir.join("data/assets.pak")).unwrap();
    pak[1000] ^= 0xff;
    fs::write(dir.join("data/assets.pak"), &pak).unwrap();
    let mut v2_pak = fs::read(src.path().join("data/assets.pak")).unwrap();
    v2_pak.extend(b"more");
    fs::write(src.path().join("data/assets.pak"), &v2_pak).unwrap();
    let (v2, m2) = mock.publish(src.path());
    install::install(
        &api,
        &job(&dir, v2, DownloadEvent::Update),
        &JobHandle::new(),
    )
    .await
    .unwrap();
    assert_matches(&dir, &m2);
}

#[tokio::test]
async fn discard_and_uninstall_only_touch_our_folders() {
    let mock = start();
    let src = tempfile::tempdir().unwrap();
    write_build_v1(src.path());
    let (v1, _) = mock.publish(src.path());
    let games = tempfile::tempdir().unwrap();

    // A folder the client did not create is never deleted.
    let foreign = games.path().join("documents");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("taxes.pdf"), b"!").unwrap();
    assert!(matches!(
        install::uninstall(&foreign),
        Err(InstallError::NotOurs(_))
    ));
    assert!(foreign.join("taxes.pdf").exists());

    // Discarding a first install that was paused removes it entirely.
    let dir = games.path().join("moss");
    let handle = JobHandle::new();
    *mock.stop_after.lock().unwrap() = Some((1, handle.clone()));
    let _ = install::install(&mock.api(), &job(&dir, v1, DownloadEvent::Install), &handle).await;
    assert!(dir.join(STATE_DIR).exists());
    install::discard(&dir).unwrap();
    assert!(!dir.exists());

    *mock.stop_after.lock().unwrap() = None;
    install::install(
        &mock.api(),
        &job(&dir, v1, DownloadEvent::Install),
        &JobHandle::new(),
    )
    .await
    .unwrap();
    install::uninstall(&dir).unwrap();
    assert!(!dir.exists());
}

#[tokio::test]
async fn rejects_manifests_that_escape_the_folder() {
    let mock = start();
    let id = next_id();
    let hash = "a".repeat(64);
    mock.store.lock().unwrap().manifests.insert(
        id,
        json!({ "files": { "../evil": { "size": 1, "hash": hash, "chunks": [{ "hash": hash, "size": 1 }] } } }),
    );
    let dir = tempfile::tempdir().unwrap().path().join("moss");
    let err = install::install(
        &mock.api(),
        &job(&dir, id, DownloadEvent::Install),
        &JobHandle::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InstallError::BadManifest(_)), "{err}");
}

#[tokio::test]
async fn update_check_falls_back_to_channels() {
    let mock = start();
    let src = tempfile::tempdir().unwrap();
    write_build_v1(src.path());
    let (v1, _) = mock.publish(src.path());
    let old = next_id();
    let res = mock
        .api()
        .check_updates(&[
            InstalledBuild {
                game: "moss".into(),
                channel: "linux".into(),
                build_id: old,
            },
            InstalledBuild {
                game: "gone".into(),
                channel: "linux".into(),
                build_id: old,
            },
        ])
        .await
        .unwrap();
    assert_eq!(res.updates.len(), 1);
    assert_eq!(res.updates[0].latest.id, v1);
    assert_eq!(res.unavailable.len(), 1);
    assert_eq!(res.unavailable[0].game, "gone");
}
