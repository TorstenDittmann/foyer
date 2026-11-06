//! Anonymous downloads for players: zips for the browser, update checks for
//! the desktop client, and the download counts developers see.
//!
//! Players never need an account. Everything here works without a token;
//! with one, the game's developer and reviewers also see unpublished games.

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::State;
use chrono::{Duration, NaiveDate};
use foyer_types::{
    ApiError, ArchiveDownload, AvailableUpdate, DailyDownloads, DownloadCounts, GameDownload,
    GameStats, GameStatus, Platform, PlatformDownloads, UpdateCheck, UpdateCheckResponse,
};
use libsql::Connection;
use serde::Deserialize;
use utoipa::IntoParams;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::analytics::{self, Source};
use crate::auth::{CurrentUser, MaybeUser};
use crate::error::{AppError, AppResult, JsonBody, PathParam, QueryParams};
use crate::model::{self, BUILD_SELECT, BuildRecord, GameRecord};
use crate::ratelimit::Bucket;
use crate::storage::Storage;

/// Most installed builds one update check may list.
const MAX_UPDATE_ENTRIES: usize = 500;

pub fn routes() -> OpenApiRouter<AppState> {
    let limited = OpenApiRouter::new()
        .routes(routes!(archive))
        .routes(routes!(check_updates));
    super::limited(limited, Bucket::Download)
        .routes(routes!(list_downloads))
        .routes(routes!(stats))
}

/// Each channel's current build, if it is ready.
async fn current_builds(conn: &Connection, game_id: Uuid) -> AppResult<Vec<(BuildRecord, String)>> {
    let mut rows = conn
        .query(
            &format!(
                "{BUILD_SELECT} JOIN channels c ON c.current_build_id = b.id
                 WHERE c.game_id = ?1 AND b.state = 'ready'
                 ORDER BY c.updated_at DESC"
            ),
            [game_id.to_string()],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        let record = model::build_from_row(&row)?;
        let channel = record.build.channel.clone();
        out.push((record, channel));
    }
    Ok(out)
}

/// Zips of each channel's current build, for the download buttons on the
/// game page. Builds whose zip is still being packed are left out.
#[utoipa::path(
    get, path = "/v1/games/{slug}/downloads", tag = "downloads",
    params(("slug" = String, Path)),
    responses((status = 200, body = Vec<GameDownload>), (status = 404, body = ApiError)),
)]
async fn list_downloads(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Vec<GameDownload>>> {
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    game.require_view(user.as_ref())?;

    let builds = current_builds(&conn, game.game.id).await?;
    // Newest channel first, so the first one per platform is the fallback
    // default.
    let mut defaults: HashMap<Platform, String> = HashMap::new();
    for (record, channel) in &builds {
        if record.archive_size.is_none() {
            continue;
        }
        let platform = record.build.platform;
        let named = channel == platform.as_str();
        if named || !defaults.contains_key(&platform) {
            defaults.insert(platform, channel.clone());
        }
    }
    let mut out: Vec<GameDownload> = builds
        .into_iter()
        .filter_map(|(record, channel)| {
            let size = record.archive_size?;
            Some(GameDownload {
                platform: record.build.platform,
                default: defaults.get(&record.build.platform) == Some(&channel),
                channel,
                archive_size: size,
                filename: format!("{}.zip", model::archive_stem(&record.build)),
                build: record.build,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        (a.platform, !a.default, &a.channel).cmp(&(b.platform, !b.default, &b.channel))
    });
    Ok(Json(out))
}

/// A short-lived link to the zip of a channel's current build. Counts one
/// browser download.
#[utoipa::path(
    post, path = "/v1/games/{slug}/channels/{channel}/archive", tag = "downloads",
    params(("slug" = String, Path), ("channel" = String, Path)),
    responses(
        (status = 200, body = ArchiveDownload),
        (status = 404, description = "No such game or channel, or no zip yet", body = ApiError),
        (status = 429, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn archive(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    PathParam((slug, channel)): PathParam<(String, String)>,
) -> AppResult<Json<ArchiveDownload>> {
    let storage = state.storage()?;
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    game.require_view(user.as_ref())?;
    let record = current_builds(&conn, game.game.id)
        .await?
        .into_iter()
        .find(|(_, c)| *c == channel)
        .map(|(record, _)| record)
        .ok_or(AppError::NotFound("download"))?;
    let size = record.archive_size.ok_or(AppError::NotFound("download"))?;
    let filename = format!("{}.zip", model::archive_stem(&record.build));
    let url = storage.presign_download(
        &Storage::archive_key(game.game.id, record.build.id),
        &filename,
    );
    count(&conn, &game, record.build.platform, Source::Web).await;
    Ok(Json(ArchiveDownload {
        url,
        filename,
        size,
        expires_at: super::builds::url_expiry(),
    }))
}

/// Counts a download of a published game. Counting never fails a download.
pub(crate) async fn count(
    conn: &Connection,
    game: &GameRecord,
    platform: Platform,
    source: Source,
) {
    if game.game.status != GameStatus::Published {
        return;
    }
    if let Err(err) = analytics::count_download(conn, game.game.id, platform, source).await {
        tracing::warn!(error = %err, "could not count download");
    }
}

/// Which installed builds have updates. For the desktop client; send every
/// installed game in one call.
#[utoipa::path(
    post, path = "/v1/updates", tag = "downloads",
    request_body = UpdateCheck,
    responses(
        (status = 200, body = UpdateCheckResponse),
        (status = 400, body = ApiError),
        (status = 429, body = ApiError),
    ),
)]
async fn check_updates(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    JsonBody(req): JsonBody<UpdateCheck>,
) -> AppResult<Json<UpdateCheckResponse>> {
    if req.installed.len() > MAX_UPDATE_ENTRIES {
        return Err(AppError::validation(format!(
            "at most {MAX_UPDATE_ENTRIES} installed builds per check"
        )));
    }
    let conn = state.conn().await?;
    // Per game slug: each channel's current build, or None when the game
    // is not visible.
    let mut games: HashMap<String, Option<BTreeMap<String, BuildRecord>>> = HashMap::new();
    let mut response = UpdateCheckResponse::default();
    for entry in req.installed {
        if !games.contains_key(&entry.game) {
            let current = match model::game_by_slug(&state, &conn, &entry.game).await {
                Ok(game) if game.can_view(user.as_ref()) => {
                    let builds = current_builds(&conn, game.game.id)
                        .await?
                        .into_iter()
                        .map(|(record, channel)| (channel, record))
                        .collect();
                    Some(builds)
                }
                Ok(_) | Err(AppError::NotFound(_)) => None,
                Err(err) => return Err(err),
            };
            games.insert(entry.game.clone(), current);
        }
        let latest = games[&entry.game]
            .as_ref()
            .and_then(|builds| builds.get(&entry.channel));
        match latest {
            None => response.unavailable.push(entry),
            Some(record) if record.build.id == entry.build_id => {}
            Some(record) => response.updates.push(AvailableUpdate {
                installed_build_id: entry.build_id,
                game: entry.game,
                channel: entry.channel,
                latest: record.build.clone(),
            }),
        }
    }
    Ok(Json(response))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StatsQuery {
    /// Days of daily counts, ending today. 1 to 365, default 30.
    pub days: Option<i64>,
}

/// Download counts of a game. Developer and reviewers only.
#[utoipa::path(
    get, path = "/v1/games/{slug}/stats", tag = "downloads",
    security(("bearer" = [])),
    params(("slug" = String, Path), StatsQuery),
    responses(
        (status = 200, body = GameStats),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn stats(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    QueryParams(query): QueryParams<StatsQuery>,
) -> AppResult<Json<GameStats>> {
    let days = query.days.unwrap_or(30).clamp(1, 365);
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    if !user.is_staff() {
        game.require_owner(&user)?;
    }
    let today = model::now().date_naive();
    let first = today - Duration::days(days - 1);

    let mut total = DownloadCounts::default();
    let mut platforms: BTreeMap<Platform, DownloadCounts> = BTreeMap::new();
    let mut daily: BTreeMap<NaiveDate, DownloadCounts> = (0..days)
        .map(|i| (first + Duration::days(i), DownloadCounts::default()))
        .collect();

    let mut rows = conn
        .query(
            "SELECT platform, source, day, count FROM download_counts WHERE game_id = ?1",
            [game.game.id.to_string()],
        )
        .await?;
    while let Some(row) = rows.next().await? {
        let platform: String = row.get(0)?;
        let source: String = row.get(1)?;
        let day: String = row.get(2)?;
        let count = u64::try_from(row.get::<i64>(3)?).map_err(anyhow::Error::from)?;
        let Some(platform) = Platform::parse(&platform) else {
            continue;
        };
        add(&mut total, &source, count);
        add(platforms.entry(platform).or_default(), &source, count);
        if let Ok(day) = day.parse::<NaiveDate>()
            && let Some(counts) = daily.get_mut(&day)
        {
            add(counts, &source, count);
        }
    }
    Ok(Json(GameStats {
        total,
        platforms: platforms
            .into_iter()
            .map(|(platform, counts)| PlatformDownloads { platform, counts })
            .collect(),
        daily: daily
            .into_iter()
            .map(|(day, counts)| DailyDownloads { day, counts })
            .collect(),
    }))
}

fn add(counts: &mut DownloadCounts, source: &str, n: u64) {
    match source {
        "web" => counts.web += n,
        "client_install" => counts.client_installs += n,
        "client_update" => counts.client_updates += n,
        _ => {}
    }
}
