//! Builds: create from a manifest, upload chunks straight to storage with
//! presigned URLs, finalize, and download.
//!
//! The push flow (used by `foyer push`):
//! 1. `POST /v1/games/{slug}/builds` with the manifest. The API answers with
//!    presigned PUT URLs for the chunks storage does not have yet.
//! 2. The client PUTs each chunk. If URLs expire, it asks
//!    `POST /v1/builds/{id}/uploads` for fresh ones.
//! 3. `POST /v1/builds/{id}/finalize`. The API checks every chunk is in
//!    storage with the right size and marks the build `processing`.
//! 4. The worker (`foyer-api worker`) verifies the chunks, scans every file
//!    for malware, packs the zip for browser downloads, marks the build
//!    `ready` and points the channel at it.

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use foyer_types::{
    ApiError, Build, BuildState, ChannelStatus, ChunkDownload, ChunkUpload, CreateBuild,
    CreateBuildResponse, DownloadEvent, DownloadRequest, DownloadUrls, GameStatus, Manifest,
    ManifestSchema, Platform, UploadUrls,
};
use futures::StreamExt;
use libsql::Connection;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::analytics::Source;
use crate::auth::{CurrentUser, MaybeUser};
use crate::error::{AppError, AppResult, JsonBody, PathParam};
use crate::jobs::{self, JobKind};
use crate::model::{self, BUILD_SELECT, BuildRecord, GameRecord, fmt_ts};
use crate::ratelimit::Bucket;
use crate::storage::{ObjectState, PRESIGN_TTL, Storage};
use crate::validate;
use crate::worker::ProcessBuild;

/// Manifests of large games run to several MB of JSON.
const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 200_000;
const MAX_CHUNKS: usize = 500_000;
/// Upper bound for one build. Storage costs money and Foyer earns none.
const MAX_BUILD_SIZE: u64 = 64 * 1024 * 1024 * 1024;
const MAX_DOWNLOADS_PER_REQUEST: usize = 10_000;
/// Parallel HEAD requests when confirming uploads.
const HEAD_CONCURRENCY: usize = 32;

pub fn routes() -> OpenApiRouter<AppState> {
    let uploads = OpenApiRouter::new()
        .routes(routes!(list_builds, create_build))
        .layer(DefaultBodyLimit::max(MAX_MANIFEST_BYTES))
        .routes(routes!(upload_urls))
        .routes(routes!(finalize_build));
    let downloads = OpenApiRouter::new()
        .routes(routes!(get_manifest))
        .routes(routes!(download_urls));
    super::limited(uploads, Bucket::Upload)
        .merge(super::limited(downloads, Bucket::Download))
        .routes(routes!(list_channels))
        .routes(routes!(get_build))
}

pub(crate) fn url_expiry() -> DateTime<Utc> {
    model::now() + Duration::from_std(PRESIGN_TTL).expect("TTL fits")
}

/// Every distinct chunk in a manifest with its size.
fn manifest_chunks(manifest: &Manifest) -> BTreeMap<String, u64> {
    manifest.unique_chunks()
}

/// Builds the developer can push to. Not while the game is in review, so the
/// reviewer approves the builds players will get; not rejected or delisted
/// games.
fn require_pushable(record: &GameRecord) -> AppResult<()> {
    match record.game.status {
        GameStatus::InReview => Err(AppError::conflict(
            "game_in_review",
            "the game is being reviewed; push builds after the decision",
        )),
        GameStatus::Rejected | GameStatus::Delisted => Err(AppError::conflict(
            "game_locked",
            "rejected or delisted games can't receive builds",
        )),
        _ => Ok(()),
    }
}

/// Builds of a game, newest first. Developer and reviewers only.
#[utoipa::path(
    get, path = "/v1/games/{slug}/builds", tag = "builds",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = Vec<Build>),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn list_builds(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Vec<Build>>> {
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    if !(game.is_owner(&user) || user.is_staff()) {
        game.require_owner(&user)?;
    }
    let mut rows = conn
        .query(
            &format!("{BUILD_SELECT} WHERE b.game_id = ?1 ORDER BY b.created_at DESC LIMIT 100"),
            [game.game.id.to_string()],
        )
        .await?;
    let mut builds = Vec::new();
    while let Some(row) = rows.next().await? {
        builds.push(model::build_from_row(&row)?.build);
    }
    Ok(Json(builds))
}

/// Start a build: send the manifest, get upload URLs for missing chunks.
#[utoipa::path(
    post, path = "/v1/games/{slug}/builds", tag = "builds",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = CreateBuild,
    responses(
        (status = 201, body = CreateBuildResponse),
        (status = 400, description = "Invalid channel or manifest", body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`channel_platform_mismatch` or `game_locked`", body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn create_build(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<CreateBuild>,
) -> AppResult<(StatusCode, Json<CreateBuildResponse>)> {
    let storage = state.storage()?;
    validate::slug("channel", &req.channel, 1, 40)?;
    let version = req.version.filter(|v| !v.trim().is_empty());
    if let Some(version) = &version {
        validate::text("version", version, 64)?;
    }
    let manifest = req.manifest;
    manifest
        .validate()
        .map_err(|e| AppError::validation(format!("manifest: {e}")))?;
    if manifest.files.is_empty() {
        return Err(AppError::validation("manifest: build has no files"));
    }
    if manifest.files.len() > MAX_FILES {
        return Err(AppError::validation(format!(
            "manifest: at most {MAX_FILES} files"
        )));
    }
    let total_size = manifest.total_size();
    if total_size > MAX_BUILD_SIZE {
        return Err(AppError::validation(format!(
            "manifest: builds can be at most {} GiB",
            MAX_BUILD_SIZE >> 30
        )));
    }
    let chunks = manifest_chunks(&manifest);
    if chunks.len() > MAX_CHUNKS {
        return Err(AppError::validation(format!(
            "manifest: at most {MAX_CHUNKS} chunks"
        )));
    }
    let launch = req.launch;
    if let Some(launch) = &launch {
        if !manifest.files.contains_key(&launch.executable) {
            return Err(AppError::validation(format!(
                "launch: {} is not a file in the build",
                launch.executable
            )));
        }
        if launch.args.len() > 32 || launch.args.iter().any(|a| a.len() > 1000) {
            return Err(AppError::validation(
                "launch: at most 32 arguments of up to 1000 characters",
            ));
        }
    }

    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    game.require_owner(&user)?;
    require_pushable(&game)?;
    let game_id = game.game.id;

    // Check storage before writing anything, so a storage outage doesn't
    // leave a build behind that the client never learned the id of.
    let missing = missing_chunks(storage, &conn, game_id, &chunks).await?;

    let build_id = Uuid::now_v7();
    let now = model::now();
    {
        let tx = conn.transaction().await?;
        let mut rows = tx
            .query(
                "SELECT platform FROM channels WHERE game_id = ?1 AND name = ?2",
                [game_id.to_string(), req.channel.clone()],
            )
            .await?;
        match rows.next().await? {
            Some(row) => {
                let platform: String = row.get(0)?;
                if platform != req.platform.as_str() {
                    return Err(AppError::conflict(
                        "channel_platform_mismatch",
                        format!(
                            "channel {} is for {platform}, not {}",
                            req.channel,
                            req.platform.as_str()
                        ),
                    ));
                }
            }
            None => {
                tx.execute(
                    "INSERT INTO channels (game_id, name, platform, updated_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    libsql::params![
                        game_id.to_string(),
                        req.channel.clone(),
                        req.platform.as_str(),
                        fmt_ts(now),
                    ],
                )
                .await?;
            }
        }
        drop(rows);
        tx.execute(
            "INSERT INTO builds (id, game_id, channel, platform, version, state, manifest,
               total_size, chunk_count, created_by, created_at, launch)
             VALUES (?1, ?2, ?3, ?4, ?5, 'uploading', ?6, ?7, ?8, ?9, ?10, ?11)",
            libsql::params![
                build_id.to_string(),
                game_id.to_string(),
                req.channel.clone(),
                req.platform.as_str(),
                version.clone(),
                serde_json::to_string(&manifest)?,
                i64::try_from(total_size).map_err(anyhow::Error::from)?,
                i64::try_from(chunks.len()).map_err(anyhow::Error::from)?,
                user.id().to_string(),
                fmt_ts(now),
                launch.as_ref().map(serde_json::to_string).transpose()?,
            ],
        )
        .await?;
        tx.commit().await?;
    }

    let uploads = presign_uploads(storage, game_id, missing);
    let build = Build {
        id: build_id,
        game: game.game.slug,
        channel: req.channel,
        platform: req.platform,
        version,
        state: BuildState::Uploading,
        total_size,
        chunk_count: chunks.len() as u64,
        created_at: now,
        launch,
        scan: None,
        failure: None,
    };
    Ok((
        StatusCode::CREATED,
        Json(CreateBuildResponse {
            build,
            uploads,
            expires_at: url_expiry(),
        }),
    ))
}

fn presign_uploads(
    storage: &Storage,
    game_id: Uuid,
    chunks: Vec<(String, u64)>,
) -> Vec<ChunkUpload> {
    chunks
        .into_iter()
        .map(|(hash, size)| ChunkUpload {
            url: storage.presign_put(&Storage::chunk_key(game_id, &hash)),
            hash,
            size,
        })
        .collect()
}

/// Returns the chunks that are not in storage yet. Chunks recorded in the
/// `chunks` table are trusted; the rest are checked with HEAD, and the ones
/// found with the right size are recorded so later pushes skip them.
async fn missing_chunks(
    storage: &Storage,
    conn: &Connection,
    game_id: Uuid,
    chunks: &BTreeMap<String, u64>,
) -> AppResult<Vec<(String, u64)>> {
    let mut known = HashMap::new();
    let mut rows = conn
        .query(
            "SELECT hash, size FROM chunks WHERE game_id = ?1",
            [game_id.to_string()],
        )
        .await?;
    while let Some(row) = rows.next().await? {
        known.insert(row.get::<String>(0)?, row.get::<i64>(1)?);
    }
    drop(rows);

    let unknown: Vec<(String, u64)> = chunks
        .iter()
        .filter(|(hash, size)| known.get(*hash).is_none_or(|k| *k as u64 != **size))
        .map(|(hash, size)| (hash.clone(), *size))
        .collect();
    if unknown.is_empty() {
        return Ok(Vec::new());
    }

    let checks: Vec<_> = futures::stream::iter(unknown)
        .map(|(hash, size)| async move {
            let state = storage.head(&Storage::chunk_key(game_id, &hash)).await;
            (hash, size, state)
        })
        .buffer_unordered(HEAD_CONCURRENCY)
        .collect()
        .await;

    let mut present = Vec::new();
    let mut missing = Vec::new();
    for (hash, size, state) in checks {
        match state? {
            ObjectState::Present { size: stored } if stored == size => present.push((hash, size)),
            _ => missing.push((hash, size)),
        }
    }

    if !present.is_empty() {
        let now = fmt_ts(model::now());
        let tx = conn.transaction().await?;
        for (hash, size) in &present {
            tx.execute(
                "INSERT INTO chunks (game_id, hash, size, created_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (game_id, hash) DO UPDATE SET size = excluded.size",
                libsql::params![game_id.to_string(), hash.clone(), *size as i64, now.clone()],
            )
            .await?;
        }
        tx.commit().await?;
    }
    missing.sort();
    Ok(missing)
}

/// Loads a build and checks the caller may see it: the game's developer and
/// reviewers always, everyone else only ready builds of published games.
async fn visible_build(
    state: &AppState,
    conn: &Connection,
    id: Uuid,
    user: Option<&CurrentUser>,
) -> AppResult<(BuildRecord, GameRecord)> {
    let build = model::build_by_id(conn, id).await?;
    let game = model::game_by_id(state, conn, build.game_id).await?;
    let privileged = user.is_some_and(|u| game.is_owner(u) || u.is_staff());
    if !privileged && !(game.can_view(user) && build.build.state == BuildState::Ready) {
        return Err(AppError::NotFound("build"));
    }
    Ok((build, game))
}

async fn owned_build(
    state: &AppState,
    conn: &Connection,
    id: Uuid,
    user: &CurrentUser,
) -> AppResult<(BuildRecord, GameRecord)> {
    let (build, game) = visible_build(state, conn, id, Some(user)).await?;
    game.require_owner(user)?;
    Ok((build, game))
}

async fn load_manifest(conn: &Connection, id: Uuid) -> AppResult<Manifest> {
    let mut rows = conn
        .query(
            "SELECT manifest FROM builds WHERE id = ?1",
            [id.to_string()],
        )
        .await?;
    let row = rows.next().await?.ok_or(AppError::NotFound("build"))?;
    Ok(model::json(&row, 0)?)
}

/// A game's channels and the build each one serves. Players see only
/// channels with a ready build.
#[utoipa::path(
    get, path = "/v1/games/{slug}/channels", tag = "builds",
    params(("slug" = String, Path)),
    responses((status = 200, body = Vec<ChannelStatus>), (status = 404, body = ApiError)),
)]
async fn list_channels(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Vec<ChannelStatus>>> {
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    game.require_view(user.as_ref())?;
    let privileged = user
        .as_ref()
        .is_some_and(|u| game.is_owner(u) || u.is_staff());

    let mut rows = conn
        .query(
            "SELECT name, platform, current_build_id FROM channels WHERE game_id = ?1 ORDER BY name",
            [game.game.id.to_string()],
        )
        .await?;
    let mut channels = Vec::new();
    while let Some(row) = rows.next().await? {
        let name: String = row.get(0)?;
        let platform: String = row.get(1)?;
        let current: Option<String> = row.get(2)?;
        channels.push((name, platform, current));
    }
    drop(rows);

    let mut out = Vec::new();
    for (name, platform, current) in channels {
        let latest = match current {
            Some(id) => Some(
                model::build_by_id(&conn, Uuid::parse_str(&id).map_err(anyhow::Error::from)?)
                    .await?
                    .build,
            ),
            None => None,
        };
        if latest.is_none() && !privileged {
            continue;
        }
        out.push(ChannelStatus {
            channel: name,
            platform: Platform::parse(&platform)
                .ok_or_else(|| anyhow::anyhow!("unknown platform {platform:?}"))?,
            latest,
        });
    }
    Ok(Json(out))
}

/// One build.
#[utoipa::path(
    get, path = "/v1/builds/{id}", tag = "builds",
    params(("id" = Uuid, Path)),
    responses((status = 200, body = Build), (status = 404, body = ApiError)),
)]
async fn get_build(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    PathParam(id): PathParam<Uuid>,
) -> AppResult<Json<Build>> {
    let conn = state.conn().await?;
    let (build, _) = visible_build(&state, &conn, id, user.as_ref()).await?;
    Ok(Json(build.build))
}

/// A build's manifest, for clients planning an install or update.
#[utoipa::path(
    get, path = "/v1/builds/{id}/manifest", tag = "builds",
    params(("id" = Uuid, Path)),
    responses((status = 200, body = ManifestSchema), (status = 404, body = ApiError)),
)]
async fn get_manifest(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    PathParam(id): PathParam<Uuid>,
) -> AppResult<Json<Manifest>> {
    let conn = state.conn().await?;
    visible_build(&state, &conn, id, user.as_ref()).await?;
    Ok(Json(load_manifest(&conn, id).await?))
}

/// Fresh upload URLs for the chunks a build is still missing.
#[utoipa::path(
    post, path = "/v1/builds/{id}/uploads", tag = "builds",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = UploadUrls),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`build_not_uploading`", body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn upload_urls(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
) -> AppResult<Json<UploadUrls>> {
    let storage = state.storage()?;
    let conn = state.conn().await?;
    let (build, _) = owned_build(&state, &conn, id, &user).await?;
    if build.build.state != BuildState::Uploading {
        return Err(AppError::conflict(
            "build_not_uploading",
            "this build is already finalized",
        ));
    }
    let chunks = manifest_chunks(&load_manifest(&conn, id).await?);
    let missing = missing_chunks(storage, &conn, build.game_id, &chunks).await?;
    Ok(Json(UploadUrls {
        uploads: presign_uploads(storage, build.game_id, missing),
        expires_at: url_expiry(),
    }))
}

/// Check every chunk is uploaded and hand the build to the worker, which
/// scans it, packs its zip, marks it `ready` and points its channel at it.
/// Calling it again on a `processing` or `ready` build returns the build
/// unchanged.
#[utoipa::path(
    post, path = "/v1/builds/{id}/finalize", tag = "builds",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Build),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`missing_chunks` (with `missing`) or `build_not_uploading`", body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn finalize_build(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
) -> AppResult<Json<Build>> {
    let storage = state.storage()?;
    let conn = state.conn().await?;
    let (build, game) = owned_build(&state, &conn, id, &user).await?;
    let build_game_id = build.game_id;
    match build.build.state {
        BuildState::Uploading => {}
        BuildState::Processing | BuildState::Ready => return Ok(Json(build.build)),
        _ => {
            return Err(AppError::conflict(
                "build_not_uploading",
                "this build can't be finalized",
            ));
        }
    }
    require_pushable(&game)?;

    let chunks = manifest_chunks(&load_manifest(&conn, id).await?);
    let missing = missing_chunks(storage, &conn, build.game_id, &chunks).await?;
    if !missing.is_empty() {
        return Err(AppError::MissingChunks(
            missing.into_iter().map(|(hash, _)| hash).collect(),
        ));
    }

    let now = fmt_ts(model::now());
    let tx = conn.transaction().await?;
    // Re-check the game's status in the same statement: the developer may have
    // submitted the game for review while the HEAD sweep ran.
    let updated = tx
        .execute(
            "UPDATE builds SET state = 'processing', finalized_at = ?1
             WHERE id = ?2 AND state = 'uploading'
               AND (SELECT status FROM games WHERE id = builds.game_id)
                   NOT IN ('in_review', 'rejected', 'delisted')",
            [now, id.to_string()],
        )
        .await?;
    if updated == 1 {
        jobs::enqueue(&tx, JobKind::ProcessBuild, &ProcessBuild { build_id: id }).await?;
    }
    tx.commit().await?;

    let build = model::build_by_id(&conn, id).await?.build;
    if updated == 0 && build.state == BuildState::Uploading {
        // Either the game became unpushable or another request changed the
        // build; report which.
        require_pushable(&model::game_by_id(&state, &conn, build_game_id).await?)?;
        return Err(AppError::conflict(
            "build_not_uploading",
            "this build can't be finalized",
        ));
    }
    Ok(Json(build))
}

/// Presigned download URLs for a ready build's chunks.
#[utoipa::path(
    post, path = "/v1/builds/{id}/downloads", tag = "builds",
    params(("id" = Uuid, Path)),
    request_body = DownloadRequest,
    responses(
        (status = 200, body = DownloadUrls),
        (status = 400, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`build_not_ready`", body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn download_urls(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    PathParam(id): PathParam<Uuid>,
    JsonBody(req): JsonBody<DownloadRequest>,
) -> AppResult<Json<DownloadUrls>> {
    let storage = state.storage()?;
    let conn = state.conn().await?;
    let (build, game) = visible_build(&state, &conn, id, user.as_ref()).await?;
    if build.build.state != BuildState::Ready {
        return Err(AppError::conflict(
            "build_not_ready",
            "this build is not ready for download",
        ));
    }
    let chunks = manifest_chunks(&load_manifest(&conn, id).await?);
    let wanted: Vec<(String, u64)> = if req.hashes.is_empty() {
        chunks.into_iter().collect()
    } else {
        let mut wanted = Vec::with_capacity(req.hashes.len());
        for hash in req.hashes {
            let size = *chunks.get(&hash).ok_or_else(|| {
                AppError::validation(format!("chunk {hash} is not in this build"))
            })?;
            wanted.push((hash, size));
        }
        wanted.sort();
        wanted.dedup();
        wanted
    };
    if wanted.len() > MAX_DOWNLOADS_PER_REQUEST {
        return Err(AppError::validation(format!(
            "at most {MAX_DOWNLOADS_PER_REQUEST} chunks per request; pass `hashes` in batches"
        )));
    }
    let downloads = wanted
        .into_iter()
        .map(|(hash, size)| ChunkDownload {
            url: storage.presign_get(&Storage::chunk_key(build.game_id, &hash)),
            hash,
            size,
        })
        .collect();
    if let Some(event) = req.event {
        let source = match event {
            DownloadEvent::Install => Source::ClientInstall,
            DownloadEvent::Update => Source::ClientUpdate,
        };
        super::downloads::count(&conn, &game, build.build.platform, source).await;
    }
    Ok(Json(DownloadUrls {
        downloads,
        expires_at: url_expiry(),
    }))
}
