//! A stand-in for the Foyer API and S3 used by the CLI tests.
//!
//! It implements the endpoints `foyer` calls with the agreed JSON shapes,
//! stores chunks in memory (checking each one's BLAKE3 hash), and can inject
//! storage failures. It deliberately uses `serde_json::Value` instead of the
//! CLI's own types so the tests also pin the wire format.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde_json::{Value, json};

pub const TOKEN: &str = "test-token";

/// The id the mock gives the n-th build (1-based).
pub fn build_id(n: usize) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

#[derive(Default)]
pub struct Store {
    /// Chunk bytes by hash.
    pub chunks: HashMap<String, Vec<u8>>,
    /// Builds by id: (game, request body, state).
    pub builds: BTreeMap<String, (String, Value, String)>,
    /// PUTs that were accepted, by hash.
    pub puts: Vec<String>,
    /// PUTs that carried an Authorization header (must stay empty).
    pub leaked_auth: usize,
    pub refreshes: usize,
    /// Private channels, by name.
    pub private: BTreeSet<String>,
    /// Tester keys, as the API lists them.
    pub keys: Vec<Value>,
    /// What a build ends up as after processing (default `ready`).
    pub final_state: Option<String>,
}

#[derive(Default)]
pub struct Faults {
    /// Answer this many chunk PUTs with 500 before accepting.
    pub fail_puts: AtomicUsize,
    /// Answer this many chunk PUTs with 403 (expired URL).
    pub expire_puts: AtomicUsize,
    /// Accept this many PUTs without storing them, so finalize reports them.
    pub drop_puts: AtomicUsize,
    /// Answer this many finalize calls with 503.
    pub fail_finalize: AtomicUsize,
    /// Report finalized builds as `processing` for this many polls.
    pub processing_polls: AtomicUsize,
}

#[derive(Clone)]
pub struct Mock {
    pub url: String,
    pub store: Arc<Mutex<Store>>,
    pub faults: Arc<Faults>,
}

pub fn start() -> Mock {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mock = Mock {
        url: url.clone(),
        store: Arc::default(),
        faults: Arc::default(),
    };
    let app = Router::new()
        .route("/v1/me", get(me))
        .route("/v1/games/{game}/builds", post(create_build))
        .route("/v1/games/{game}/channels", get(channels))
        .route("/v1/games/{game}/channels/{channel}", patch(update_channel))
        .route(
            "/v1/games/{game}/tester-keys",
            get(list_keys).post(create_key),
        )
        .route("/v1/games/{game}/tester-keys/{id}", delete(delete_key))
        .route("/v1/builds/{id}", get(get_build))
        .route("/v1/builds/{id}/uploads", post(refresh_uploads))
        .route("/v1/builds/{id}/finalize", post(finalize))
        .route("/storage/{hash}", put(put_chunk))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
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
    /// Run `foyer` against this mock with an isolated config directory.
    pub fn foyer(&self, config_dir: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_foyer"))
            .args(args)
            .env("FOYER_CONFIG_DIR", config_dir)
            .env("FOYER_API_URL", &self.url)
            .env_remove("FOYER_TOKEN")
            .output()
            .unwrap()
    }

    pub fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

#[allow(clippy::result_large_err)]
fn authorized(headers: &HeaderMap) -> Result<(), Response> {
    let expected = format!("Bearer {TOKEN}");
    match headers.get("authorization").and_then(|v| v.to_str().ok()) {
        Some(v) if v == expected => Ok(()),
        _ => Err(error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "invalid token",
        )),
    }
}

async fn me(headers: HeaderMap) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    Json(json!({ "id": "00000000-0000-4000-8000-0000000000aa", "username": "torsten", "email": "dev@example.com", "role": "user", "created_at": "2026-11-06T00:00:00Z" })).into_response()
}

/// All distinct chunks of a manifest JSON, with sizes.
fn manifest_chunks(manifest: &Value) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for file in manifest["files"].as_object().unwrap().values() {
        for chunk in file["chunks"].as_array().unwrap() {
            out.insert(
                chunk["hash"].as_str().unwrap().to_string(),
                chunk["size"].as_u64().unwrap(),
            );
        }
    }
    out
}

fn missing_uploads(mock: &Mock, store: &Store, manifest: &Value) -> Vec<Value> {
    manifest_chunks(manifest)
        .into_iter()
        .filter(|(hash, _)| !store.chunks.contains_key(hash))
        .map(|(hash, size)| json!({ "hash": hash, "size": size, "url": format!("{}/storage/{hash}", mock.url) }))
        .collect()
}

fn build_json(id: &str, game: &str, req: &Value, state: &str) -> Value {
    let chunks = manifest_chunks(&req["manifest"]);
    let total: u64 = req["manifest"]["files"]
        .as_object()
        .unwrap()
        .values()
        .map(|f| f["size"].as_u64().unwrap())
        .sum();
    json!({
        "id": id,
        "game": game,
        "channel": req["channel"],
        "platform": req["platform"],
        "version": req["version"],
        "state": state,
        "total_size": total,
        "chunk_count": chunks.len(),
        "created_at": "2026-11-06T12:00:00Z",
    })
}

async fn create_build(
    State(mock): State<Mock>,
    UrlPath(game): UrlPath<String>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    if game == "in-review" {
        return error(StatusCode::CONFLICT, "game_in_review", "game is in review");
    }
    if game != "lantern" {
        return error(StatusCode::NOT_FOUND, "not_found", "no such game");
    }
    let mut store = mock.store();
    let id = build_id(store.builds.len() + 1);
    let channel = req["channel"].as_str().unwrap().to_string();
    match req["private"].as_bool() {
        Some(true) => {
            store.private.insert(channel);
        }
        Some(false) => {
            store.private.remove(&channel);
        }
        None => {}
    }
    let uploads = missing_uploads(&mock, &store, &req["manifest"]);
    let build = build_json(&id, &game, &req, "uploading");
    store.builds.insert(id, (game, req, "uploading".into()));
    Json(json!({ "build": build, "uploads": uploads, "expires_at": "2026-11-06T13:00:00Z" }))
        .into_response()
}

async fn refresh_uploads(
    State(mock): State<Mock>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    let mut store = mock.store();
    store.refreshes += 1;
    let Some((_, req, _)) = store.builds.get(&id) else {
        return error(StatusCode::NOT_FOUND, "not_found", "no such build");
    };
    let uploads = missing_uploads(&mock, &store, &req["manifest"]);
    Json(json!({ "uploads": uploads, "expires_at": "2026-11-06T13:00:00Z" })).into_response()
}

async fn finalize(
    State(mock): State<Mock>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    if take(&mock.faults.fail_finalize) {
        return error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", "try again");
    }
    let mut store = mock.store();
    let Some((game, req, _)) = store.builds.get(&id).cloned() else {
        return error(StatusCode::NOT_FOUND, "not_found", "no such build");
    };
    let missing: Vec<String> = manifest_chunks(&req["manifest"])
        .into_keys()
        .filter(|h| !store.chunks.contains_key(h))
        .collect();
    if !missing.is_empty() {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "missing_chunks", "message": "chunks missing", "missing": missing })),
        )
            .into_response();
    }
    let state = if mock.faults.processing_polls.load(Ordering::SeqCst) > 0 {
        "processing"
    } else {
        "ready"
    };
    store.builds.get_mut(&id).unwrap().2 = state.into();
    Json(build_json(&id, &game, &req, state)).into_response()
}

async fn get_build(
    State(mock): State<Mock>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    let mut store = mock.store();
    let final_state = store.final_state.clone().unwrap_or_else(|| "ready".into());
    match store.builds.get_mut(&id) {
        Some((game, req, state)) => {
            if state == "processing" && !take(&mock.faults.processing_polls) {
                *state = final_state;
            }
            let mut build = build_json(&id, game, req, state);
            if state == "failed" {
                build["failure"] = json!("zip could not be packed");
            }
            Json(build).into_response()
        }
        None => error(StatusCode::NOT_FOUND, "not_found", "no such build"),
    }
}

async fn update_channel(
    State(mock): State<Mock>,
    UrlPath((game, channel)): UrlPath<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    let mut store = mock.store();
    let Some((_, body, _)) = store
        .builds
        .values()
        .find(|(g, b, _)| *g == game && b["channel"] == channel.as_str())
        .cloned()
    else {
        return error(StatusCode::NOT_FOUND, "not_found", "no such channel");
    };
    let private = req["private"].as_bool().unwrap();
    if private {
        store.private.insert(channel.clone());
    } else {
        store.private.remove(&channel);
    }
    Json(json!({ "channel": channel, "platform": body["platform"], "latest": null, "private": private }))
        .into_response()
}

async fn list_keys(State(mock): State<Mock>, headers: HeaderMap) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    Json(mock.store().keys.clone()).into_response()
}

async fn create_key(
    State(mock): State<Mock>,
    UrlPath(game): UrlPath<String>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    let mut store = mock.store();
    let n = store.keys.len() + 1;
    let key = json!({
        "id": format!("00000000-0000-4000-9000-{n:012}"),
        "label": req["label"],
        "channels": req["channels"],
        "prefix": "fyk_abc123",
        "created_at": "2026-11-06T12:00:00Z",
        "expires_at": "2026-12-06T12:00:00Z",
        "last_used_at": null,
    });
    store.keys.push(key.clone());
    Json(json!({
        "key": key,
        "secret": format!("fyk_abc123secret{n}"),
        "link": format!("https://foyer.lol/tester?key=fyk_abc123secret{n}&game={game}"),
    }))
    .into_response()
}

async fn delete_key(
    State(mock): State<Mock>,
    UrlPath((_, id)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    let mut store = mock.store();
    let before = store.keys.len();
    store.keys.retain(|k| k["id"] != id.as_str());
    if store.keys.len() == before {
        return error(StatusCode::NOT_FOUND, "not_found", "no such key");
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn channels(
    State(mock): State<Mock>,
    UrlPath(game): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(res) = authorized(&headers) {
        return res;
    }
    let store = mock.store();
    // Latest ready build per (channel, platform).
    let mut latest: BTreeMap<(String, String), Value> = BTreeMap::new();
    for (id, (g, req, state)) in &store.builds {
        if g == &game && state == "ready" {
            let key = (
                req["channel"].as_str().unwrap().to_string(),
                req["platform"].as_str().unwrap().to_string(),
            );
            latest.insert(key, build_json(id, g, req, state));
        }
    }
    let list: Vec<Value> = latest
        .into_iter()
        .map(|((channel, platform), build)| {
            let private = store.private.contains(&channel);
            json!({ "channel": channel, "platform": platform, "latest": build, "private": private })
        })
        .collect();
    Json(list).into_response()
}

fn take(counter: &AtomicUsize) -> bool {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
}

async fn put_chunk(
    State(mock): State<Mock>,
    UrlPath(hash): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers.contains_key("authorization") {
        mock.store().leaked_auth += 1;
    }
    if take(&mock.faults.fail_puts) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if take(&mock.faults.expire_puts) {
        return (StatusCode::FORBIDDEN, "Request has expired").into_response();
    }
    if blake3::hash(&body).to_hex().as_str() != hash {
        return (StatusCode::BAD_REQUEST, "hash mismatch").into_response();
    }
    if take(&mock.faults.drop_puts) {
        return StatusCode::OK.into_response();
    }
    let mut store = mock.store();
    store.puts.push(hash.clone());
    store.chunks.insert(hash, body.to_vec());
    StatusCode::OK.into_response()
}

/// Deterministic incompressible bytes.
pub fn noise(seed: u64, len: usize) -> Vec<u8> {
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
