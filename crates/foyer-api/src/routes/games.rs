//! Game pages and their images.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Duration;
use foyer_types::{
    ApiError, CreateGame, CreateMedia, Game, GameStatus, Media, MediaKind, MediaUpload, Page,
    UpdateGame,
};
use serde::Deserialize;
use utoipa::IntoParams;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{CurrentUser, MaybeUser};
use crate::error::{AppError, AppResult, JsonBody, PathParam, QueryParams};
use crate::model::{self, GAME_SELECT, GameRecord, fmt_ts};
use crate::ratelimit::Bucket;
use crate::storage::{PRESIGN_TTL, Storage};
use crate::validate;

pub fn routes() -> OpenApiRouter<AppState> {
    super::limited(
        OpenApiRouter::new().routes(routes!(create_media)),
        Bucket::Upload,
    )
    .routes(routes!(list_games, create_game))
    .routes(routes!(my_games))
    .routes(routes!(get_game, update_game))
    .routes(routes!(delete_media))
}

const MAX_SCREENSHOTS: i64 = 12;
const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/avif"];

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct GameQuery {
    /// Only games with this tag.
    pub tag: Option<String>,
    /// Full-text search over title, tagline, tags, developer and
    /// description. Every word must match; the last word may be a prefix.
    pub q: Option<String>,
    /// From the previous page's `next_cursor`.
    pub cursor: Option<String>,
    /// 1 to 100, default 24.
    pub limit: Option<i64>,
}

/// Turns what a visitor typed into an FTS5 query: every word must match,
/// and the last one may be the start of a word (search as you type). Words
/// are quoted, so FTS5 operators in the input are plain text. `None` when
/// nothing searchable is left.
fn fts_query(input: &str) -> Option<String> {
    let words: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(12)
        .map(|w| format!("\"{}\"", w.to_lowercase()))
        .collect();
    let (last, rest) = words.split_last()?;
    let mut query = rest.join(" ");
    if !query.is_empty() {
        query.push(' ');
    }
    query.push_str(last);
    query.push('*');
    Some(query)
}

/// Published games, newest first, or best match first when searching.
#[utoipa::path(
    get, path = "/v1/games", tag = "games",
    params(GameQuery),
    responses((status = 200, body = Page<Game>), (status = 400, body = ApiError)),
)]
async fn list_games(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<GameQuery>,
) -> AppResult<Json<Page<Game>>> {
    let offset = model::parse_cursor(query.cursor.as_deref())?;
    let limit = model::clamp_limit(query.limit);
    let tag = query
        .tag
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty());
    if let Some(q) = &query.q {
        validate::text("q", q, 100)?;
    }
    let search = query.q.as_deref().and_then(fts_query);
    let conn = state.conn().await?;
    // `?1 IS NULL OR …` keeps one statement per shape of query.
    let games = match search {
        None => {
            model::query_games(
                &state,
                &conn,
                &format!(
                    "{GAME_SELECT} WHERE g.status = 'published'
                       AND (?1 IS NULL OR EXISTS (SELECT 1 FROM json_each(g.tags) WHERE value = ?1))
                     ORDER BY g.published_at DESC, g.id LIMIT ?2 OFFSET ?3"
                ),
                libsql::params![tag, limit + 1, offset],
            )
            .await?
        }
        Some(search) => {
            // Best match first: title hits weigh most, description least.
            model::query_games(
                &state,
                &conn,
                &format!(
                    "{GAME_SELECT} JOIN games_fts ON games_fts.game_id = g.id
                     WHERE g.status = 'published' AND games_fts MATCH ?2
                       AND (?1 IS NULL OR EXISTS (SELECT 1 FROM json_each(g.tags) WHERE value = ?1))
                     ORDER BY bm25(games_fts, 0.0, 10.0, 5.0, 4.0, 3.0, 1.0), g.published_at DESC, g.id
                     LIMIT ?3 OFFSET ?4"
                ),
                libsql::params![tag, search, limit + 1, offset],
            )
            .await?
        }
    };
    let games = games.into_iter().map(|g| g.game).collect();
    Ok(Json(model::page(games, offset, limit)))
}

/// The caller's games in every status, for the developer dashboard.
#[utoipa::path(
    get, path = "/v1/me/games", tag = "games",
    security(("bearer" = [])),
    responses((status = 200, body = Vec<Game>), (status = 401, body = ApiError)),
)]
async fn my_games(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<Game>>> {
    let conn = state.conn().await?;
    let games = model::query_games(
        &state,
        &conn,
        &format!("{GAME_SELECT} WHERE d.owner_id = ?1 ORDER BY g.created_at DESC"),
        [user.id().to_string()],
    )
    .await?;
    Ok(Json(games.into_iter().map(|g| g.game).collect()))
}

struct GameFields<'a> {
    title: &'a str,
    tagline: &'a str,
    description: &'a str,
    ai_disclosure: Option<&'a str>,
}

fn check_game(fields: GameFields<'_>) -> AppResult<()> {
    validate::required_text("title", fields.title, 100)?;
    validate::text("tagline", fields.tagline, 160)?;
    validate::text("description", fields.description, 20_000)?;
    if let Some(ai) = fields.ai_disclosure {
        validate::text("ai_disclosure", ai, 2000)?;
    }
    Ok(())
}

/// Create a game page as a draft. Needs a developer profile.
#[utoipa::path(
    post, path = "/v1/games", tag = "games",
    security(("bearer" = [])),
    request_body = CreateGame,
    responses(
        (status = 201, body = Game),
        (status = 400, body = ApiError),
        (status = 403, description = "No developer profile", body = ApiError),
        (status = 409, description = "`slug_taken`", body = ApiError),
    ),
)]
async fn create_game(
    State(state): State<AppState>,
    user: CurrentUser,
    JsonBody(req): JsonBody<CreateGame>,
) -> AppResult<(StatusCode, Json<Game>)> {
    validate::slug("slug", &req.slug, 2, 40)?;
    let ai_disclosure = req.ai_disclosure.filter(|a| !a.trim().is_empty());
    check_game(GameFields {
        title: &req.title,
        tagline: &req.tagline,
        description: &req.description,
        ai_disclosure: ai_disclosure.as_deref(),
    })?;
    let tags = validate::tags(&req.tags)?;
    validate::support_links(&req.support_links)?;
    let mut descriptors = req.content_descriptors;
    descriptors.sort_by_key(|d| *d as u8);
    descriptors.dedup();

    let conn = state.conn().await?;
    let developer = model::developer_by_owner(&conn, user.id())
        .await?
        .ok_or_else(|| AppError::forbidden("create a developer profile first"))?
        .developer;
    let id = Uuid::now_v7();
    let now = fmt_ts(model::now());
    let inserted = conn
        .execute(
            "INSERT INTO games (id, developer_id, slug, title, tagline, description, tags,
               content_descriptors, ai_disclosure, support_links, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'draft', ?11, ?11)
             ON CONFLICT DO NOTHING",
            libsql::params![
                id.to_string(),
                developer.id.to_string(),
                req.slug,
                req.title.trim(),
                req.tagline.trim(),
                req.description,
                serde_json::to_string(&tags)?,
                serde_json::to_string(&descriptors)?,
                ai_disclosure,
                serde_json::to_string(&req.support_links)?,
                now,
            ],
        )
        .await?;
    if inserted == 0 {
        return Err(AppError::conflict("slug_taken", "that game slug is taken"));
    }
    let game = model::game_by_id(&state, &conn, id).await?.game;
    Ok((StatusCode::CREATED, Json(game)))
}

/// A game page. Unpublished games are visible only to their developer and
/// reviewers.
#[utoipa::path(
    get, path = "/v1/games/{slug}", tag = "games",
    params(("slug" = String, Path)),
    responses((status = 200, body = Game), (status = 404, body = ApiError)),
)]
async fn get_game(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Game>> {
    let conn = state.conn().await?;
    let record = model::game_by_slug(&state, &conn, &slug).await?;
    record.require_view(user.as_ref())?;
    Ok(Json(record.game))
}

/// The developer can't edit a page while it is being reviewed, or after it
/// was rejected or delisted.
fn require_editable(record: &GameRecord) -> AppResult<()> {
    match record.game.status {
        GameStatus::InReview => Err(AppError::conflict(
            "game_in_review",
            "the game is being reviewed; wait for the decision",
        )),
        GameStatus::Rejected | GameStatus::Delisted => Err(AppError::conflict(
            "game_locked",
            "rejected or delisted games can't be edited",
        )),
        _ => Ok(()),
    }
}

/// Edit a game page.
#[utoipa::path(
    patch, path = "/v1/games/{slug}", tag = "games",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = UpdateGame,
    responses(
        (status = 200, body = Game),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`game_in_review` or `game_locked`", body = ApiError),
    ),
)]
async fn update_game(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<UpdateGame>,
) -> AppResult<Json<Game>> {
    let conn = state.conn().await?;
    let record = model::game_by_slug(&state, &conn, &slug).await?;
    record.require_owner(&user)?;
    require_editable(&record)?;

    let mut game = record.game;
    if let Some(title) = req.title {
        game.title = title.trim().to_string();
    }
    if let Some(tagline) = req.tagline {
        game.tagline = tagline.trim().to_string();
    }
    if let Some(description) = req.description {
        game.description = description;
    }
    if let Some(tags) = req.tags {
        game.tags = validate::tags(&tags)?;
    }
    if let Some(mut descriptors) = req.content_descriptors {
        descriptors.sort_by_key(|d| *d as u8);
        descriptors.dedup();
        game.content_descriptors = descriptors;
    }
    if let Some(ai) = req.ai_disclosure {
        game.ai_disclosure = Some(ai).filter(|a| !a.trim().is_empty());
    }
    if let Some(links) = req.support_links {
        validate::support_links(&links)?;
        game.support_links = links;
    }
    check_game(GameFields {
        title: &game.title,
        tagline: &game.tagline,
        description: &game.description,
        ai_disclosure: game.ai_disclosure.as_deref(),
    })?;

    conn.execute(
        "UPDATE games SET title = ?1, tagline = ?2, description = ?3, tags = ?4,
           content_descriptors = ?5, ai_disclosure = ?6, support_links = ?7, updated_at = ?8
         WHERE id = ?9",
        libsql::params![
            game.title.clone(),
            game.tagline.clone(),
            game.description.clone(),
            serde_json::to_string(&game.tags)?,
            serde_json::to_string(&game.content_descriptors)?,
            game.ai_disclosure.clone(),
            serde_json::to_string(&game.support_links)?,
            fmt_ts(model::now()),
            game.id.to_string(),
        ],
    )
    .await?;
    Ok(Json(model::game_by_id(&state, &conn, game.id).await?.game))
}

/// Add an image to a game page and get a presigned URL to upload it to. A new
/// capsule replaces the old one.
#[utoipa::path(
    post, path = "/v1/games/{slug}/media", tag = "games",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = CreateMedia,
    responses(
        (status = 201, body = MediaUpload),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 409, description = "`too_many_screenshots`, `game_in_review` or `game_locked`", body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn create_media(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<CreateMedia>,
) -> AppResult<(StatusCode, Json<MediaUpload>)> {
    let storage = state.storage()?;
    if !IMAGE_TYPES.contains(&req.content_type.as_str()) {
        return Err(AppError::validation(format!(
            "content_type must be one of {}",
            IMAGE_TYPES.join(", ")
        )));
    }
    let conn = state.conn().await?;
    let record = model::game_by_slug(&state, &conn, &slug).await?;
    record.require_owner(&user)?;
    require_editable(&record)?;
    let game_id = record.game.id;

    let tx = conn.transaction().await?;
    let position = match req.kind {
        MediaKind::Capsule => {
            tx.execute(
                "DELETE FROM media WHERE game_id = ?1 AND kind = 'capsule'",
                [game_id.to_string()],
            )
            .await?;
            0
        }
        MediaKind::Screenshot => {
            let mut rows = tx
                .query(
                    "SELECT count(*), coalesce(max(position) + 1, 0) FROM media
                     WHERE game_id = ?1 AND kind = 'screenshot'",
                    [game_id.to_string()],
                )
                .await?;
            let row = rows.next().await?.expect("aggregate returns a row");
            if row.get::<i64>(0)? >= MAX_SCREENSHOTS {
                return Err(AppError::conflict(
                    "too_many_screenshots",
                    format!("a game can have at most {MAX_SCREENSHOTS} screenshots"),
                ));
            }
            row.get::<i64>(1)?
        }
    };
    let media_id = Uuid::now_v7();
    let now = model::now();
    tx.execute(
        "INSERT INTO media (id, game_id, kind, content_type, position, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        libsql::params![
            media_id.to_string(),
            game_id.to_string(),
            req.kind.as_str(),
            req.content_type,
            position,
            fmt_ts(now),
        ],
    )
    .await?;
    tx.execute(
        "UPDATE games SET updated_at = ?1 WHERE id = ?2",
        [fmt_ts(now), game_id.to_string()],
    )
    .await?;
    tx.commit().await?;

    let key = Storage::media_key(game_id, media_id);
    Ok((
        StatusCode::CREATED,
        Json(MediaUpload {
            media: Media {
                id: media_id,
                kind: req.kind,
                url: storage.public_url(&key),
                position,
            },
            upload_url: storage.presign_put(&key),
            expires_at: now + Duration::from_std(PRESIGN_TTL).expect("TTL fits"),
        }),
    ))
}

/// Remove an image from a game page.
#[utoipa::path(
    delete, path = "/v1/games/{slug}/media/{id}", tag = "games",
    security(("bearer" = [])),
    params(("slug" = String, Path), ("id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, body = ApiError),
    ),
)]
async fn delete_media(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam((slug, id)): PathParam<(String, Uuid)>,
) -> AppResult<StatusCode> {
    let conn = state.conn().await?;
    let record = model::game_by_slug(&state, &conn, &slug).await?;
    record.require_owner(&user)?;
    require_editable(&record)?;
    let deleted = conn
        .execute(
            "DELETE FROM media WHERE id = ?1 AND game_id = ?2",
            [id.to_string(), record.game.id.to_string()],
        )
        .await?;
    if deleted == 0 {
        return Err(AppError::NotFound("media"));
    }
    // The object stays in storage; a cleanup job can sweep unreferenced keys.
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::fts_query;

    #[test]
    fn search_input_becomes_a_safe_fts_query() {
        assert_eq!(
            fts_query("Lantern keep").as_deref(),
            Some("\"lantern\" \"keep\"*")
        );
        assert_eq!(fts_query("  ").as_deref(), None);
        // Operators and quotes are just separators.
        assert_eq!(
            fts_query("a\" OR title:x NEAR(").as_deref(),
            Some("\"a\" \"or\" \"title\" \"x\" \"near\"*")
        );
        assert_eq!(fts_query("Café").as_deref(), Some("\"café\"*"));
    }
}
