//! Test harness: the API router on a temporary database, with an in-process
//! fake S3 server that stores objects in memory.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use foyer_api::config::{DatabaseConfig, StorageConfig};
use foyer_api::ratelimit::{ClientIpConfig, RateLimits};
use foyer_api::storage::Storage;
use foyer_api::worker::{self, WorkerConfig};
use foyer_api::{AppState, db, router};

pub const ADMIN_EMAIL: &str = "admin@foyer.test";

type Objects = Arc<Mutex<HashMap<String, Bytes>>>;

pub struct TestApp {
    pub router: Router,
    pub state: AppState,
    pub worker: WorkerConfig,
    pub objects: Objects,
    pub s3_base: String,
    _dir: tempfile::TempDir,
}

pub struct Reply {
    pub status: StatusCode,
    pub body: Value,
}

impl TestApp {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let database = db::connect(&DatabaseConfig::Local {
            path: dir.path().join("test.db").to_string_lossy().into_owned(),
        })
        .await
        .unwrap();
        db::migrate(&database).await.unwrap();

        let objects: Objects = Arc::default();
        let s3 = Router::new()
            .route("/{bucket}/{*key}", any(fake_s3))
            .with_state(objects.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, s3).await.unwrap() });
        let s3_base = format!("http://{addr}");

        let storage = Storage::new(&StorageConfig {
            endpoint: s3_base.parse().unwrap(),
            bucket: "foyer".into(),
            region: "auto".into(),
            access_key_id: "test".into(),
            secret_access_key: "test".into(),
            virtual_hosted: false,
            media_base_url: Some("https://media.foyer.test".parse().unwrap()),
        })
        .unwrap()
        // Small parts, so zips larger than this go through multipart upload.
        .with_part_size(64 * 1024);
        let state = AppState::new(database)
            .with_storage(storage)
            .with_admin_emails(vec![ADMIN_EMAIL.into()])
            // Tests share one address; `rate_limits` tests turn limits on.
            .with_rate_limits(None, Default::default());
        Self {
            router: router(state.clone()),
            state,
            worker: WorkerConfig::default(),
            objects,
            s3_base,
            _dir: dir,
        }
    }

    /// Rebuilds the router with these rate limits.
    pub fn with_rate_limits(mut self, limits: RateLimits, client_ip: ClientIpConfig) -> Self {
        self.state = self.state.with_rate_limits(Some(limits), client_ip);
        self.router = router(self.state.clone());
        self
    }

    /// Runs every due background job, like `foyer-api worker` would.
    pub async fn run_jobs(&self) -> usize {
        worker::run_pending(&self.state, &self.worker)
            .await
            .unwrap()
    }

    /// The object stored under `key` (without the bucket).
    pub fn object(&self, key: &str) -> Option<Bytes> {
        self.objects
            .lock()
            .unwrap()
            .get(&format!("foyer/{key}"))
            .cloned()
    }

    pub async fn call(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Reply {
        self.call_with_keys(method, path, token, &[], body).await
    }

    /// Like [`Self::call`], sending tester keys in `X-Foyer-Key`.
    pub async fn call_with_keys(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        keys: &[&str],
        body: Option<Value>,
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if !keys.is_empty() {
            req = req.header("x-foyer-key", keys.join(", "));
        }
        let req = match body {
            Some(body) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| panic!("non-JSON body: {}", String::from_utf8_lossy(&bytes)))
        };
        Reply { status, body }
    }

    pub async fn get(&self, path: &str, token: Option<&str>) -> Reply {
        self.call(Method::GET, path, token, None).await
    }

    pub async fn post(&self, path: &str, token: Option<&str>, body: Value) -> Reply {
        self.call(Method::POST, path, token, Some(body)).await
    }

    pub async fn patch(&self, path: &str, token: Option<&str>, body: Value) -> Reply {
        self.call(Method::PATCH, path, token, Some(body)).await
    }

    /// Registers a user, confirms their email and returns their session
    /// token.
    pub async fn register(&self, username: &str, email: &str) -> String {
        let token = self.register_unverified(username, email).await;
        let link = self.last_email_link(email).await;
        let reply = self
            .post(
                "/v1/auth/verify-email",
                None,
                json!({ "token": token_from_link(&link) }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        token
    }

    /// Registers a user without confirming their email.
    pub async fn register_unverified(&self, username: &str, email: &str) -> String {
        let reply = self
            .post(
                "/v1/auth/register",
                None,
                json!({ "username": username, "email": email, "password": "correct horse battery" }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        reply.body["token"].as_str().unwrap().to_string()
    }

    /// The emails queued for `to` (not yet sent by the worker), oldest first.
    pub async fn queued_emails(&self, to: &str) -> Vec<Value> {
        let conn = self.state.db.connect().unwrap();
        let mut rows = conn
            .query(
                "SELECT payload FROM jobs WHERE kind = 'send_email' ORDER BY created_at, id",
                (),
            )
            .await
            .unwrap();
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            let payload: Value = serde_json::from_str(&row.get::<String>(0).unwrap()).unwrap();
            if payload["to"] == to.to_lowercase() {
                out.push(payload);
            }
        }
        out
    }

    /// The link in the newest email queued for `to`.
    pub async fn last_email_link(&self, to: &str) -> String {
        let emails = self.queued_emails(to).await;
        let body = emails.last().expect("no email queued")["body"]
            .as_str()
            .unwrap()
            .to_string();
        body.split_whitespace()
            .find(|w| w.starts_with("http"))
            .expect("no link in the email")
            .to_string()
    }

    /// Registers a developer with a profile and a draft game.
    pub async fn developer_with_game(&self, username: &str, game: &str) -> String {
        let token = self
            .register(username, &format!("{username}@example.com"))
            .await;
        let reply = self
            .post(
                "/v1/developers",
                Some(&token),
                json!({ "slug": username, "display_name": username.to_uppercase() }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        let reply = self
            .post(
                "/v1/games",
                Some(&token),
                json!({
                    "slug": game,
                    "title": "Lantern Keeper",
                    "tagline": "Keep the light alive",
                    "description": "A short puzzle game about lighthouses.",
                    "tags": ["Puzzle", "pixel-art"],
                    "content_descriptors": ["horror"],
                    "support_links": [{ "kind": "kofi", "url": "https://ko-fi.com/dev" }],
                }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        token
    }

    /// Submits the game and has a new admin account (`root`) approve it.
    /// Returns the admin's token.
    pub async fn publish(&self, dev: &str, game: &str) -> String {
        let admin = self.register("root", ADMIN_EMAIL).await;
        let sub = self
            .post(
                &format!("/v1/games/{game}/submissions"),
                Some(dev),
                json!({}),
            )
            .await;
        assert_eq!(sub.status, StatusCode::CREATED, "{}", sub.body);
        let id = sub.body["id"].as_str().unwrap();
        let decided = self
            .post(
                &format!("/v1/review/submissions/{id}/decision"),
                Some(&admin),
                json!({ "outcome": "approved" }),
            )
            .await;
        assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
        admin
    }

    /// PUTs bytes to a presigned URL, like a client would.
    pub async fn upload(&self, url: &str, bytes: &[u8]) {
        let status = reqwest::Client::new()
            .put(url)
            .body(bytes.to_vec())
            .send()
            .await
            .unwrap()
            .status();
        assert!(status.is_success(), "upload failed: {status}");
    }

    /// Pushes a one-file build and finalizes it.
    pub async fn push_build(
        &self,
        token: &str,
        game: &str,
        channel: &str,
        contents: &[u8],
    ) -> Value {
        let (manifest, chunks) = manifest_for(&[("game.exe", contents)]);
        let reply = self
            .post(
                &format!("/v1/games/{game}/builds"),
                Some(token),
                json!({ "channel": channel, "platform": "windows", "version": "1.0", "manifest": manifest }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        for upload in reply.body["uploads"].as_array().unwrap() {
            let hash = upload["hash"].as_str().unwrap();
            self.upload(upload["url"].as_str().unwrap(), &chunks[hash])
                .await;
        }
        let id = reply.body["build"]["id"].as_str().unwrap().to_string();
        let reply = self
            .post(
                &format!("/v1/builds/{id}/finalize"),
                Some(token),
                json!(null),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        assert_eq!(reply.body["state"], "processing");
        self.run_jobs().await;
        let reply = self.get(&format!("/v1/builds/{id}"), Some(token)).await;
        assert_eq!(reply.body["state"], "ready", "{}", reply.body);
        reply.body
    }
}

/// The `token` query parameter of an emailed link.
pub fn token_from_link(link: &str) -> String {
    let url: url::Url = link.parse().unwrap();
    url.query_pairs()
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.into_owned())
        .expect("link without a token")
}

/// A manifest where each non-empty file is a single chunk, plus the chunk bytes by hash.
pub fn manifest_for(files: &[(&str, &[u8])]) -> (Value, HashMap<String, Vec<u8>>) {
    let mut entries = serde_json::Map::new();
    let mut chunks = HashMap::new();
    for (path, bytes) in files {
        let hash = foyer_patch::hash_bytes(bytes);
        let file_chunks = if bytes.is_empty() {
            json!([])
        } else {
            chunks.insert(hash.clone(), bytes.to_vec());
            json!([{ "hash": hash, "size": bytes.len() }])
        };
        entries.insert(
            path.to_string(),
            json!({ "size": bytes.len(), "hash": hash, "chunks": file_chunks }),
        );
    }
    (json!({ "files": entries }), chunks)
}

/// Minimal S3: PUT stores, GET returns, HEAD reports size, DELETE removes,
/// plus multipart uploads. Signatures are not checked.
async fn fake_s3(
    State(objects): State<Objects>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    method: Method,
    body: Bytes,
) -> Response {
    let key = format!("{bucket}/{key}");
    let mut objects = objects.lock().unwrap();
    // Parts are kept as `{upload id}/{part number}` until completed.
    if let Some(upload) = query.get("uploadId") {
        return match (method, query.get("partNumber")) {
            (Method::PUT, Some(part)) => {
                objects.insert(format!("parts/{upload}/{part:0>5}"), body);
                let mut headers = HeaderMap::new();
                headers.insert(header::ETAG, format!("\"etag-{part}\"").parse().unwrap());
                (StatusCode::OK, headers).into_response()
            }
            (Method::POST, None) => {
                let prefix = format!("parts/{upload}/");
                let mut parts: Vec<_> = objects
                    .keys()
                    .filter(|k| k.starts_with(&prefix))
                    .cloned()
                    .collect();
                parts.sort();
                let mut whole = Vec::new();
                for part in parts {
                    whole.extend_from_slice(&objects.remove(&part).unwrap());
                }
                objects.insert(key, Bytes::from(whole));
                "<CompleteMultipartUploadResult></CompleteMultipartUploadResult>".into_response()
            }
            (Method::DELETE, None) => StatusCode::NO_CONTENT.into_response(),
            _ => StatusCode::BAD_REQUEST.into_response(),
        };
    }
    if method == Method::POST && query.contains_key("uploads") {
        return format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><InitiateMultipartUploadResult>\
             <Bucket>{bucket}</Bucket><Key>{key}</Key><UploadId>upload-{}</UploadId>\
             </InitiateMultipartUploadResult>",
            objects.len()
        )
        .into_response();
    }
    match method {
        Method::PUT => {
            objects.insert(key, body);
            StatusCode::OK.into_response()
        }
        Method::HEAD => match objects.get(&key) {
            Some(bytes) => {
                let mut headers = HeaderMap::new();
                headers.insert(header::CONTENT_LENGTH, bytes.len().into());
                (StatusCode::OK, headers).into_response()
            }
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Method::GET => match objects.get(&key) {
            Some(bytes) => bytes.clone().into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Method::DELETE => {
            objects.remove(&key);
            StatusCode::NO_CONTENT.into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}
