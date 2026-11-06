//! Tester keys: the developer creates, lists and revokes them; clients
//! redeem one to learn which game it unlocks. How keys grant access is in
//! `crate::testers`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use foyer_types::{
    ApiError, CreateTesterKey, CreatedTesterKey, GameStatus, RedeemTesterKey, TesterAccess,
    TesterKey,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{self, CurrentUser};
use crate::error::{AppError, AppResult, JsonBody, PathParam};
use crate::model::{self, fmt_ts};
use crate::ratelimit::Bucket;
use crate::testers::{self, KEY_PREFIX, MAX_KEYS_PER_GAME};
use crate::validate;

const MAX_CHANNELS_PER_KEY: usize = 20;

pub fn routes() -> OpenApiRouter<AppState> {
    let redeem = OpenApiRouter::new().routes(routes!(redeem));
    super::limited(redeem, Bucket::Auth)
        .routes(routes!(list_keys, create_key))
        .routes(routes!(delete_key))
}

fn key_from_row(row: &libsql::Row) -> anyhow::Result<TesterKey> {
    Ok(TesterKey {
        id: model::uuid(row, 0)?,
        label: row.get(1)?,
        channels: model::json(row, 2)?,
        prefix: row.get(3)?,
        created_at: model::ts(row, 4)?,
        expires_at: model::opt_ts(row, 5)?,
        last_used_at: model::opt_ts(row, 6)?,
    })
}

/// A game's tester keys, newest first. Developer only.
#[utoipa::path(
    get, path = "/v1/games/{slug}/tester-keys", tag = "testers",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = Vec<TesterKey>),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn list_keys(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Vec<TesterKey>>> {
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    game.require_owner(&user)?;
    let mut rows = conn
        .query(
            "SELECT id, label, channels, prefix, created_at, expires_at, last_used_at
             FROM tester_keys WHERE game_id = ?1 ORDER BY created_at DESC",
            [game.game.id.to_string()],
        )
        .await?;
    let mut keys = Vec::new();
    while let Some(row) = rows.next().await? {
        keys.push(key_from_row(&row)?);
    }
    Ok(Json(keys))
}

/// Create a tester key. It unlocks the game before it is published, and
/// the listed channels (all when empty), private or not. The key is
/// returned only once, with a website link that unlocks the game.
#[utoipa::path(
    post, path = "/v1/games/{slug}/tester-keys", tag = "testers",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = CreateTesterKey,
    responses(
        (status = 201, body = CreatedTesterKey),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`game_locked` or `too_many_keys`", body = ApiError),
    ),
)]
async fn create_key(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<CreateTesterKey>,
) -> AppResult<(StatusCode, Json<CreatedTesterKey>)> {
    let label = req.label.trim().to_string();
    validate::text("label", &label, 80)?;
    let mut channels = req.channels;
    for channel in &channels {
        validate::slug("channel", channel, 1, 40)?;
    }
    channels.sort();
    channels.dedup();
    if channels.len() > MAX_CHANNELS_PER_KEY {
        return Err(AppError::validation(format!(
            "at most {MAX_CHANNELS_PER_KEY} channels per key"
        )));
    }
    let ttl = testers::ttl(req.expires_in_days)?;

    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    game.require_owner(&user)?;
    if matches!(
        game.game.status,
        GameStatus::Rejected | GameStatus::Delisted
    ) {
        return Err(AppError::conflict(
            "game_locked",
            "rejected or delisted games can't have tester keys",
        ));
    }
    let mut rows = conn
        .query(
            "SELECT count(*) FROM tester_keys WHERE game_id = ?1",
            [game.game.id.to_string()],
        )
        .await?;
    let count: i64 = match rows.next().await? {
        Some(row) => row.get(0)?,
        None => 0,
    };
    drop(rows);
    if count >= MAX_KEYS_PER_GAME {
        return Err(AppError::conflict(
            "too_many_keys",
            format!("a game can have at most {MAX_KEYS_PER_GAME} tester keys; revoke some first"),
        ));
    }

    let secret = auth::random_secret(KEY_PREFIX)?;
    let prefix: String = secret.chars().take(KEY_PREFIX.len() + 6).collect();
    let id = Uuid::now_v7();
    let created_at = model::now();
    let expires_at = created_at + ttl;
    conn.execute(
        "INSERT INTO tester_keys (id, game_id, label, channels, key_hash, prefix, created_by,
           created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        libsql::params![
            id.to_string(),
            game.game.id.to_string(),
            label.clone(),
            serde_json::to_string(&channels)?,
            auth::secret_hash(&secret),
            prefix.clone(),
            user.id().to_string(),
            fmt_ts(created_at),
            fmt_ts(expires_at),
        ],
    )
    .await?;
    let link = state.web_link("/tester", &[("key", &secret)]);
    Ok((
        StatusCode::CREATED,
        Json(CreatedTesterKey {
            key: TesterKey {
                id,
                label,
                channels,
                prefix,
                created_at,
                expires_at: Some(expires_at),
                last_used_at: None,
            },
            secret,
            link,
        }),
    ))
}

/// Revoke a tester key. Clients holding it lose access at once; games they
/// already installed keep working but get no more updates.
#[utoipa::path(
    delete, path = "/v1/games/{slug}/tester-keys/{id}", tag = "testers",
    security(("bearer" = [])),
    params(("slug" = String, Path), ("id" = Uuid, Path)),
    responses((status = 204), (status = 403, body = ApiError), (status = 404, body = ApiError)),
)]
async fn delete_key(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam((slug, id)): PathParam<(String, Uuid)>,
) -> AppResult<StatusCode> {
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    game.require_owner(&user)?;
    let deleted = conn
        .execute(
            "DELETE FROM tester_keys WHERE id = ?1 AND game_id = ?2",
            [id.to_string(), game.game.id.to_string()],
        )
        .await?;
    if deleted == 0 {
        return Err(AppError::NotFound("tester key"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Which game a tester key unlocks. Needs no account. Afterwards send the
/// key in `X-Foyer-Key` with every request for that game.
#[utoipa::path(
    post, path = "/v1/tester-keys/redeem", tag = "testers",
    request_body = RedeemTesterKey,
    responses(
        (status = 200, body = TesterAccess),
        (status = 404, description = "Unknown, expired or revoked key", body = ApiError),
        (status = 429, body = ApiError),
    ),
)]
async fn redeem(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<RedeemTesterKey>,
) -> AppResult<Json<TesterAccess>> {
    let key = req.key.trim().to_string();
    if !key.starts_with(KEY_PREFIX) {
        return Err(AppError::NotFound("tester key"));
    }
    let conn = state.conn().await?;
    let grant = testers::lookup(&conn, std::slice::from_ref(&key))
        .await?
        .pop()
        .ok_or(AppError::NotFound("tester key"))?;
    let game = model::game_by_id(&state, &conn, grant.game_id).await?;
    if matches!(
        game.game.status,
        GameStatus::Rejected | GameStatus::Delisted
    ) {
        return Err(AppError::NotFound("tester key"));
    }
    let mut rows = conn
        .query(
            "SELECT expires_at FROM tester_keys WHERE id = ?1",
            [grant.key_id.to_string()],
        )
        .await?;
    let expires_at = match rows.next().await? {
        Some(row) => model::opt_ts(&row, 0)?,
        None => None,
    };
    Ok(Json(TesterAccess {
        game: game.game.slug,
        title: game.game.title,
        channels: grant.channels,
        expires_at,
    }))
}
