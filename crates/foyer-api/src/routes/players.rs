//! Player accounts: wishlist, following developers, account settings and
//! unsubscribing from release emails.
//!
//! None of this is needed to browse or download; it is what an account adds.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use foyer_types::{
    AccountSettings, ApiError, DeveloperSummary, Follow, GameStatus, Page, UnsubscribeRequest,
    UpdateAccountSettings, WishlistEntry,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::developers::PageQuery;
use crate::AppState;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, JsonBody, PathParam, QueryParams};
use crate::model::{self, fmt_ts};
use crate::ratelimit::Bucket;

pub fn routes() -> OpenApiRouter<AppState> {
    let limited = OpenApiRouter::new().routes(routes!(unsubscribe));
    super::limited(limited, Bucket::Auth)
        .routes(routes!(wishlist))
        .routes(routes!(
            wishlist_entry,
            add_to_wishlist,
            remove_from_wishlist
        ))
        .routes(routes!(following))
        .routes(routes!(follow_entry, follow, unfollow))
        .routes(routes!(settings, update_settings))
}

/// The most entries a wishlist or follow list may hold.
const MAX_ENTRIES: i64 = 1000;

// ---------------------------------------------------------------- wishlist

/// A published game by slug; anything else is a 404, like on the game page.
async fn published_game(
    state: &AppState,
    conn: &libsql::Connection,
    slug: &str,
) -> AppResult<model::GameRecord> {
    let record = model::game_by_slug(state, conn, slug).await?;
    if record.game.status != GameStatus::Published {
        return Err(AppError::NotFound("game"));
    }
    Ok(record)
}

async fn count(conn: &libsql::Connection, sql: &str, user: Uuid) -> AppResult<i64> {
    let mut rows = conn.query(sql, [user.to_string()]).await?;
    Ok(match rows.next().await? {
        Some(row) => row.get(0)?,
        None => 0,
    })
}

/// The caller's wishlist, most recently added first. Games that are no
/// longer public are left out.
#[utoipa::path(
    get, path = "/v1/me/wishlist", tag = "players",
    security(("bearer" = [])),
    params(PageQuery),
    responses((status = 200, body = Page<WishlistEntry>), (status = 401, body = ApiError)),
)]
async fn wishlist(
    State(state): State<AppState>,
    user: CurrentUser,
    QueryParams(query): QueryParams<PageQuery>,
) -> AppResult<Json<Page<WishlistEntry>>> {
    let offset = model::parse_cursor(query.cursor.as_deref())?;
    let limit = model::clamp_limit(query.limit);
    let conn = state.conn().await?;
    let mut rows = conn
        .query(
            "SELECT w.game_id, w.created_at FROM wishlist w JOIN games g ON g.id = w.game_id
             WHERE w.user_id = ?1 AND g.status = 'published'
             ORDER BY w.created_at DESC, w.game_id LIMIT ?2 OFFSET ?3",
            libsql::params![user.id().to_string(), limit + 1, offset],
        )
        .await?;
    let mut found = Vec::new();
    while let Some(row) = rows.next().await? {
        found.push((model::uuid(&row, 0)?, model::ts(&row, 1)?));
    }
    drop(rows);
    let mut entries = Vec::with_capacity(found.len());
    for (game_id, added_at) in found {
        let game = model::game_by_id(&state, &conn, game_id).await?.game;
        entries.push(WishlistEntry { game, added_at });
    }
    Ok(Json(model::page(entries, offset, limit)))
}

async fn wishlist_row(
    state: &AppState,
    conn: &libsql::Connection,
    user: &CurrentUser,
    slug: &str,
) -> AppResult<Option<WishlistEntry>> {
    let record = published_game(state, conn, slug).await?;
    let mut rows = conn
        .query(
            "SELECT created_at FROM wishlist WHERE user_id = ?1 AND game_id = ?2",
            [user.id().to_string(), record.game.id.to_string()],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some(WishlistEntry {
            added_at: model::ts(&row, 0)?,
            game: record.game,
        })),
        None => Ok(None),
    }
}

/// Whether a game is on the caller's wishlist: 200 if it is, 404 if not.
#[utoipa::path(
    get, path = "/v1/me/wishlist/{slug}", tag = "players",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = WishlistEntry),
        (status = 401, body = ApiError),
        (status = 404, description = "Not on the wishlist, or no such public game", body = ApiError),
    ),
)]
async fn wishlist_entry(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<WishlistEntry>> {
    let conn = state.conn().await?;
    wishlist_row(&state, &conn, &user, &slug)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound("wishlist entry"))
}

/// Add a published game to the caller's wishlist. Adding it again is fine.
#[utoipa::path(
    put, path = "/v1/me/wishlist/{slug}", tag = "players",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = WishlistEntry),
        (status = 401, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`wishlist_full`", body = ApiError),
    ),
)]
async fn add_to_wishlist(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<WishlistEntry>> {
    let conn = state.conn().await?;
    let record = published_game(&state, &conn, &slug).await?;
    let held = count(
        &conn,
        "SELECT count(*) FROM wishlist WHERE user_id = ?1",
        user.id(),
    )
    .await?;
    if held >= MAX_ENTRIES {
        return Err(AppError::conflict(
            "wishlist_full",
            format!("a wishlist holds at most {MAX_ENTRIES} games"),
        ));
    }
    conn.execute(
        "INSERT INTO wishlist (user_id, game_id, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT DO NOTHING",
        [
            user.id().to_string(),
            record.game.id.to_string(),
            fmt_ts(model::now()),
        ],
    )
    .await?;
    wishlist_row(&state, &conn, &user, &slug)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound("wishlist entry"))
}

/// Remove a game from the caller's wishlist. Removing one that isn't there
/// is fine.
#[utoipa::path(
    delete, path = "/v1/me/wishlist/{slug}", tag = "players",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses((status = 204), (status = 401, body = ApiError)),
)]
async fn remove_from_wishlist(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<StatusCode> {
    // By slug, so a game that was delisted can still be removed.
    state
        .conn()
        .await?
        .execute(
            "DELETE FROM wishlist WHERE user_id = ?1
               AND game_id = (SELECT id FROM games WHERE slug = ?2)",
            [user.id().to_string(), slug],
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- follows

/// Developers the caller follows, most recently followed first.
#[utoipa::path(
    get, path = "/v1/me/following", tag = "players",
    security(("bearer" = [])),
    params(PageQuery),
    responses((status = 200, body = Page<Follow>), (status = 401, body = ApiError)),
)]
async fn following(
    State(state): State<AppState>,
    user: CurrentUser,
    QueryParams(query): QueryParams<PageQuery>,
) -> AppResult<Json<Page<Follow>>> {
    let offset = model::parse_cursor(query.cursor.as_deref())?;
    let limit = model::clamp_limit(query.limit);
    let conn = state.conn().await?;
    let mut rows = conn
        .query(
            "SELECT d.slug, d.display_name, f.created_at FROM follows f
             JOIN developers d ON d.id = f.developer_id
             WHERE f.user_id = ?1
             ORDER BY f.created_at DESC, d.slug LIMIT ?2 OFFSET ?3",
            libsql::params![user.id().to_string(), limit + 1, offset],
        )
        .await?;
    let mut follows = Vec::new();
    while let Some(row) = rows.next().await? {
        follows.push(follow_from_row(&row)?);
    }
    Ok(Json(model::page(follows, offset, limit)))
}

fn follow_from_row(row: &libsql::Row) -> anyhow::Result<Follow> {
    Ok(Follow {
        developer: DeveloperSummary {
            slug: row.get(0)?,
            display_name: row.get(1)?,
        },
        followed_at: model::ts(row, 2)?,
    })
}

async fn follow_row(
    conn: &libsql::Connection,
    user: &CurrentUser,
    slug: &str,
) -> AppResult<Option<Follow>> {
    let mut rows = conn
        .query(
            "SELECT d.slug, d.display_name, f.created_at FROM follows f
             JOIN developers d ON d.id = f.developer_id
             WHERE f.user_id = ?1 AND d.slug = ?2",
            [user.id().to_string(), slug.to_string()],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some(follow_from_row(&row)?)),
        None => Ok(None),
    }
}

/// Whether the caller follows a developer: 200 if so, 404 if not.
#[utoipa::path(
    get, path = "/v1/me/following/{slug}", tag = "players",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = Follow),
        (status = 401, body = ApiError),
        (status = 404, description = "Not followed, or no such developer", body = ApiError),
    ),
)]
async fn follow_entry(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Follow>> {
    let conn = state.conn().await?;
    follow_row(&conn, &user, &slug)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound("follow"))
}

/// Follow a developer. With release emails on (see `/v1/me/settings`), the
/// caller gets an email when the developer releases a game. Following again
/// is fine.
#[utoipa::path(
    put, path = "/v1/me/following/{slug}", tag = "players",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = Follow),
        (status = 400, description = "Following yourself", body = ApiError),
        (status = 401, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`following_full`", body = ApiError),
    ),
)]
async fn follow(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Follow>> {
    let conn = state.conn().await?;
    let record = model::developer_by_slug(&conn, &slug).await?;
    if record.owner_id == user.id() {
        return Err(AppError::validation("you can't follow your own profile"));
    }
    let held = count(
        &conn,
        "SELECT count(*) FROM follows WHERE user_id = ?1",
        user.id(),
    )
    .await?;
    if held >= MAX_ENTRIES {
        return Err(AppError::conflict(
            "following_full",
            format!("you can follow at most {MAX_ENTRIES} developers"),
        ));
    }
    conn.execute(
        "INSERT INTO follows (user_id, developer_id, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT DO NOTHING",
        [
            user.id().to_string(),
            record.developer.id.to_string(),
            fmt_ts(model::now()),
        ],
    )
    .await?;
    follow_row(&conn, &user, &slug)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound("follow"))
}

/// Stop following a developer. Unfollowing one you don't follow is fine.
#[utoipa::path(
    delete, path = "/v1/me/following/{slug}", tag = "players",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses((status = 204), (status = 401, body = ApiError)),
)]
async fn unfollow(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<StatusCode> {
    state
        .conn()
        .await?
        .execute(
            "DELETE FROM follows WHERE user_id = ?1
               AND developer_id = (SELECT id FROM developers WHERE slug = ?2)",
            [user.id().to_string(), slug],
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- settings

async fn load_settings(conn: &libsql::Connection, user: Uuid) -> AppResult<AccountSettings> {
    let mut rows = conn
        .query(
            "SELECT release_emails FROM users WHERE id = ?1",
            [user.to_string()],
        )
        .await?;
    let row = rows.next().await?.ok_or(AppError::NotFound("user"))?;
    Ok(AccountSettings {
        release_emails: row.get::<i64>(0)? != 0,
    })
}

/// The caller's account settings.
#[utoipa::path(
    get, path = "/v1/me/settings", tag = "players",
    security(("bearer" = [])),
    responses((status = 200, body = AccountSettings), (status = 401, body = ApiError)),
)]
async fn settings(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<AccountSettings>> {
    let conn = state.conn().await?;
    Ok(Json(load_settings(&conn, user.id()).await?))
}

/// Change the caller's account settings.
#[utoipa::path(
    patch, path = "/v1/me/settings", tag = "players",
    security(("bearer" = [])),
    request_body = UpdateAccountSettings,
    responses(
        (status = 200, body = AccountSettings),
        (status = 400, body = ApiError),
        (status = 401, body = ApiError),
    ),
)]
async fn update_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    JsonBody(req): JsonBody<UpdateAccountSettings>,
) -> AppResult<Json<AccountSettings>> {
    let conn = state.conn().await?;
    if let Some(on) = req.release_emails {
        conn.execute(
            "UPDATE users SET release_emails = ?1 WHERE id = ?2",
            libsql::params![i64::from(on), user.id().to_string()],
        )
        .await?;
    }
    Ok(Json(load_settings(&conn, user.id()).await?))
}

/// Turn release emails off with the token from an email's unsubscribe link.
/// No sign-in needed.
#[utoipa::path(
    post, path = "/v1/unsubscribe", tag = "players",
    request_body = UnsubscribeRequest,
    responses(
        (status = 204),
        (status = 400, description = "`invalid_token`", body = ApiError),
        (status = 429, body = ApiError),
    ),
)]
async fn unsubscribe(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<UnsubscribeRequest>,
) -> AppResult<StatusCode> {
    let token = req.token.trim();
    if token.is_empty() {
        return Err(invalid_unsubscribe());
    }
    let updated = state
        .conn()
        .await?
        .execute(
            "UPDATE users SET release_emails = 0 WHERE unsubscribe_token = ?1",
            [token],
        )
        .await?;
    if updated == 0 {
        return Err(invalid_unsubscribe());
    }
    Ok(StatusCode::NO_CONTENT)
}

fn invalid_unsubscribe() -> AppError {
    AppError::coded(
        StatusCode::BAD_REQUEST,
        "invalid_token",
        "this unsubscribe link is not valid; turn release emails off in your account settings",
    )
}
