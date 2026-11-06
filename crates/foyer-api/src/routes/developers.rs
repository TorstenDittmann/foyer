//! Developer profiles.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use foyer_types::{ApiError, CreateDeveloper, Developer, Game, Page, UpdateDeveloper};
use serde::Deserialize;
use utoipa::IntoParams;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, JsonBody, PathParam, QueryParams};
use crate::model::{self, GAME_SELECT, fmt_ts};
use crate::validate;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_developer))
        .routes(routes!(my_developer))
        .routes(routes!(get_developer, update_developer))
        .routes(routes!(developer_games))
}

fn check_profile(display_name: &str, bio: &str, website_url: Option<&str>) -> AppResult<()> {
    validate::required_text("display_name", display_name, 60)?;
    validate::text("bio", bio, 5000)?;
    if let Some(url) = website_url {
        validate::https_url("website_url", url)?;
    }
    Ok(())
}

/// Create the caller's developer profile. Each account can have one.
#[utoipa::path(
    post, path = "/v1/developers", tag = "developers",
    security(("bearer" = [])),
    request_body = CreateDeveloper,
    responses(
        (status = 201, body = Developer),
        (status = 400, body = ApiError),
        (status = 409, description = "`developer_exists` or `slug_taken`", body = ApiError),
    ),
)]
async fn create_developer(
    State(state): State<AppState>,
    user: CurrentUser,
    JsonBody(req): JsonBody<CreateDeveloper>,
) -> AppResult<(StatusCode, Json<Developer>)> {
    validate::slug("slug", &req.slug, 2, 40)?;
    let website_url = req.website_url.filter(|u| !u.is_empty());
    check_profile(&req.display_name, &req.bio, website_url.as_deref())?;
    validate::support_links(&req.support_links)?;

    let conn = state.conn().await?;
    if model::developer_by_owner(&conn, user.id()).await?.is_some() {
        return Err(AppError::conflict(
            "developer_exists",
            "this account already has a developer profile",
        ));
    }
    let now = model::now();
    let developer = Developer {
        id: Uuid::now_v7(),
        slug: req.slug,
        display_name: req.display_name.trim().to_string(),
        bio: req.bio,
        website_url,
        support_links: req.support_links,
        created_at: now,
    };
    let inserted = conn
        .execute(
            "INSERT INTO developers
               (id, owner_id, slug, display_name, bio, website_url, support_links, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8) ON CONFLICT DO NOTHING",
            libsql::params![
                developer.id.to_string(),
                user.id().to_string(),
                developer.slug.clone(),
                developer.display_name.clone(),
                developer.bio.clone(),
                developer.website_url.clone(),
                serde_json::to_string(&developer.support_links)?,
                fmt_ts(now),
            ],
        )
        .await?;
    if inserted == 0 {
        return Err(AppError::conflict(
            "slug_taken",
            "that developer slug is taken",
        ));
    }
    Ok((StatusCode::CREATED, Json(developer)))
}

/// The caller's developer profile.
#[utoipa::path(
    get, path = "/v1/me/developer", tag = "developers",
    security(("bearer" = [])),
    responses(
        (status = 200, body = Developer),
        (status = 404, description = "No profile yet", body = ApiError),
    ),
)]
async fn my_developer(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Developer>> {
    let conn = state.conn().await?;
    model::developer_by_owner(&conn, user.id())
        .await?
        .map(|d| Json(d.developer))
        .ok_or(AppError::NotFound("developer"))
}

/// A developer's public profile.
#[utoipa::path(
    get, path = "/v1/developers/{slug}", tag = "developers",
    params(("slug" = String, Path)),
    responses((status = 200, body = Developer), (status = 404, body = ApiError)),
)]
async fn get_developer(
    State(state): State<AppState>,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Developer>> {
    let conn = state.conn().await?;
    Ok(Json(
        model::developer_by_slug(&conn, &slug).await?.developer,
    ))
}

/// Edit the caller's developer profile.
#[utoipa::path(
    patch, path = "/v1/developers/{slug}", tag = "developers",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = UpdateDeveloper,
    responses(
        (status = 200, body = Developer),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn update_developer(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<UpdateDeveloper>,
) -> AppResult<Json<Developer>> {
    let conn = state.conn().await?;
    let record = model::developer_by_slug(&conn, &slug).await?;
    if record.owner_id != user.id() {
        return Err(AppError::forbidden("this is not your developer profile"));
    }
    let mut dev = record.developer;
    if let Some(name) = req.display_name {
        dev.display_name = name.trim().to_string();
    }
    if let Some(bio) = req.bio {
        dev.bio = bio;
    }
    if let Some(url) = req.website_url {
        dev.website_url = Some(url).filter(|u| !u.is_empty());
    }
    if let Some(links) = req.support_links {
        validate::support_links(&links)?;
        dev.support_links = links;
    }
    check_profile(&dev.display_name, &dev.bio, dev.website_url.as_deref())?;

    conn.execute(
        "UPDATE developers SET display_name = ?1, bio = ?2, website_url = ?3,
           support_links = ?4, updated_at = ?5 WHERE id = ?6",
        libsql::params![
            dev.display_name.clone(),
            dev.bio.clone(),
            dev.website_url.clone(),
            serde_json::to_string(&dev.support_links)?,
            fmt_ts(model::now()),
            dev.id.to_string(),
        ],
    )
    .await?;
    Ok(Json(dev))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// From the previous page's `next_cursor`.
    pub cursor: Option<String>,
    /// 1 to 100, default 24.
    pub limit: Option<i64>,
}

/// A developer's published games, newest first.
#[utoipa::path(
    get, path = "/v1/developers/{slug}/games", tag = "developers",
    params(("slug" = String, Path), PageQuery),
    responses((status = 200, body = Page<Game>), (status = 404, body = ApiError)),
)]
async fn developer_games(
    State(state): State<AppState>,
    PathParam(slug): PathParam<String>,
    QueryParams(query): QueryParams<PageQuery>,
) -> AppResult<Json<Page<Game>>> {
    let conn = state.conn().await?;
    let developer = model::developer_by_slug(&conn, &slug).await?.developer;
    let offset = model::parse_cursor(query.cursor.as_deref())?;
    let limit = model::clamp_limit(query.limit);
    let games = model::query_games(
        &state,
        &conn,
        &format!(
            "{GAME_SELECT} WHERE g.developer_id = ?1 AND g.status = 'published'
             ORDER BY g.published_at DESC, g.id LIMIT ?2 OFFSET ?3"
        ),
        libsql::params![developer.id.to_string(), limit + 1, offset],
    )
    .await?;
    let games = games.into_iter().map(|g| g.game).collect();
    Ok(Json(model::page(games, offset, limit)))
}
