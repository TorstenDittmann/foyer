//! Mapping between database rows and the wire types in `foyer-types`, plus
//! the loaders the handlers share.

use anyhow::Context;
use chrono::{DateTime, SecondsFormat, Utc};
use foyer_types::{
    Build, BuildScan, BuildState, Developer, DeveloperSummary, Game, GameStatus, LaunchTarget,
    Media, MediaKind, Platform, PolicyRule, Role, ScanStatus, Submission, SubmissionStatus, User,
    UserSummary,
};
use libsql::{Connection, Row};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::AppState;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult};
use crate::storage::Storage;

pub fn now() -> DateTime<Utc> {
    // Millisecond precision keeps stored strings fixed-width, so they sort
    // correctly as text.
    let now = Utc::now();
    DateTime::from_timestamp_millis(now.timestamp_millis()).unwrap_or(now)
}

pub fn fmt_ts(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_ts(value: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("bad timestamp {value:?}"))?
        .with_timezone(&Utc))
}

pub fn ts(row: &Row, idx: i32) -> anyhow::Result<DateTime<Utc>> {
    parse_ts(&row.get::<String>(idx)?)
}

pub fn opt_ts(row: &Row, idx: i32) -> anyhow::Result<Option<DateTime<Utc>>> {
    row.get::<Option<String>>(idx)?
        .map(|v| parse_ts(&v))
        .transpose()
}

pub fn uuid(row: &Row, idx: i32) -> anyhow::Result<Uuid> {
    let value: String = row.get(idx)?;
    Uuid::parse_str(&value).with_context(|| format!("bad uuid {value:?}"))
}

pub fn json<T: DeserializeOwned>(row: &Row, idx: i32) -> anyhow::Result<T> {
    let value: String = row.get(idx)?;
    serde_json::from_str(&value).with_context(|| format!("bad JSON in column {idx}"))
}

fn parsed<T>(row: &Row, idx: i32, parse: fn(&str) -> Option<T>) -> anyhow::Result<T> {
    let value: String = row.get(idx)?;
    parse(&value).with_context(|| format!("unknown value {value:?} in column {idx}"))
}

fn u64_col(row: &Row, idx: i32) -> anyhow::Result<u64> {
    Ok(u64::try_from(row.get::<i64>(idx)?)?)
}

// ---------------------------------------------------------------- users

/// Columns for [`user_from_row`], with the users table aliased as `u`.
pub const USER_COLUMNS: &str =
    "u.id, u.username, u.email, u.role, u.created_at, u.email_verified_at";

/// How many columns [`USER_COLUMNS`] selects.
pub const USER_COLUMN_COUNT: i32 = 6;

pub fn user_from_row(row: &Row, start: i32) -> anyhow::Result<User> {
    Ok(User {
        id: uuid(row, start)?,
        username: row.get(start + 1)?,
        email: row.get(start + 2)?,
        role: parsed(row, start + 3, Role::parse)?,
        created_at: ts(row, start + 4)?,
        email_verified: row.get::<Option<String>>(start + 5)?.is_some(),
    })
}

pub async fn user_by_id(conn: &Connection, id: Uuid) -> AppResult<User> {
    let mut rows = conn
        .query(
            &format!("SELECT {USER_COLUMNS} FROM users u WHERE u.id = ?1"),
            [id.to_string()],
        )
        .await?;
    let row = rows.next().await?.ok_or(AppError::NotFound("user"))?;
    Ok(user_from_row(&row, 0)?)
}

// ---------------------------------------------------------------- developers

const DEVELOPER_SELECT: &str = "SELECT d.id, d.slug, d.display_name, d.bio, d.website_url, \
     d.support_links, d.created_at, d.owner_id FROM developers d";

pub struct DeveloperRecord {
    pub developer: Developer,
    pub owner_id: Uuid,
}

fn developer_from_row(row: &Row) -> anyhow::Result<DeveloperRecord> {
    Ok(DeveloperRecord {
        developer: Developer {
            id: uuid(row, 0)?,
            slug: row.get(1)?,
            display_name: row.get(2)?,
            bio: row.get(3)?,
            website_url: row.get(4)?,
            support_links: json(row, 5)?,
            created_at: ts(row, 6)?,
        },
        owner_id: uuid(row, 7)?,
    })
}

pub async fn developer_by_slug(conn: &Connection, slug: &str) -> AppResult<DeveloperRecord> {
    let mut rows = conn
        .query(&format!("{DEVELOPER_SELECT} WHERE d.slug = ?1"), [slug])
        .await?;
    let row = rows.next().await?.ok_or(AppError::NotFound("developer"))?;
    Ok(developer_from_row(&row)?)
}

pub async fn developer_by_owner(
    conn: &Connection,
    owner: Uuid,
) -> AppResult<Option<DeveloperRecord>> {
    let mut rows = conn
        .query(
            &format!("{DEVELOPER_SELECT} WHERE d.owner_id = ?1"),
            [owner.to_string()],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some(developer_from_row(&row)?)),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------- games

/// Selects everything [`game_from_row`] needs. Filter with `WHERE` on `g.`.
pub const GAME_SELECT: &str = "SELECT g.id, g.slug, g.title, g.tagline, g.description, g.tags, \
     g.content_descriptors, g.ai_disclosure, g.support_links, g.status, g.published_at, \
     g.created_at, g.updated_at, d.slug, d.display_name, d.owner_id \
     FROM games g JOIN developers d ON d.id = g.developer_id";

/// A game with the facts needed for access checks.
pub struct GameRecord {
    pub game: Game,
    pub owner_id: Uuid,
}

impl GameRecord {
    pub fn is_owner(&self, user: &CurrentUser) -> bool {
        self.owner_id == user.id()
    }

    /// Published games are public; everything else only to the owner and
    /// staff.
    pub fn can_view(&self, user: Option<&CurrentUser>) -> bool {
        self.game.status == GameStatus::Published
            || user.is_some_and(|u| self.is_owner(u) || u.is_staff())
    }

    /// 404 rather than 403 for games the caller can't see, so unpublished
    /// slugs don't leak.
    pub fn require_view(&self, user: Option<&CurrentUser>) -> AppResult<()> {
        if self.can_view(user) {
            Ok(())
        } else {
            Err(AppError::NotFound("game"))
        }
    }

    pub fn require_owner(&self, user: &CurrentUser) -> AppResult<()> {
        if self.is_owner(user) {
            Ok(())
        } else if self.can_view(Some(user)) {
            Err(AppError::forbidden("only the game's developer can do this"))
        } else {
            Err(AppError::NotFound("game"))
        }
    }
}

/// Maps a [`GAME_SELECT`] row. `platforms` and `media` are filled in by
/// [`hydrate_games`].
pub fn game_from_row(row: &Row) -> anyhow::Result<GameRecord> {
    Ok(GameRecord {
        game: Game {
            id: uuid(row, 0)?,
            slug: row.get(1)?,
            title: row.get(2)?,
            tagline: row.get(3)?,
            description: row.get(4)?,
            tags: json(row, 5)?,
            content_descriptors: json(row, 6)?,
            ai_disclosure: row.get(7)?,
            support_links: json(row, 8)?,
            status: parsed(row, 9, GameStatus::parse)?,
            published_at: opt_ts(row, 10)?,
            created_at: ts(row, 11)?,
            updated_at: ts(row, 12)?,
            developer: DeveloperSummary {
                slug: row.get(13)?,
                display_name: row.get(14)?,
            },
            platforms: Vec::new(),
            media: Vec::new(),
        },
        owner_id: uuid(row, 15)?,
    })
}

/// Runs a [`GAME_SELECT`] query and returns hydrated games.
pub async fn query_games(
    state: &AppState,
    conn: &Connection,
    sql: &str,
    params: impl libsql::params::IntoParams,
) -> AppResult<Vec<GameRecord>> {
    let mut rows = conn.query(sql, params).await?;
    let mut games = Vec::new();
    while let Some(row) = rows.next().await? {
        games.push(game_from_row(&row)?);
    }
    hydrate_games(state, conn, &mut games).await?;
    Ok(games)
}

pub async fn game_by_slug(
    state: &AppState,
    conn: &Connection,
    slug: &str,
) -> AppResult<GameRecord> {
    query_games(
        state,
        conn,
        &format!("{GAME_SELECT} WHERE g.slug = ?1"),
        [slug],
    )
    .await?
    .pop()
    .ok_or(AppError::NotFound("game"))
}

pub async fn game_by_id(state: &AppState, conn: &Connection, id: Uuid) -> AppResult<GameRecord> {
    query_games(
        state,
        conn,
        &format!("{GAME_SELECT} WHERE g.id = ?1"),
        [id.to_string()],
    )
    .await?
    .pop()
    .ok_or(AppError::NotFound("game"))
}

/// Fills in each game's platforms (from channels serving a ready build) and
/// media.
pub async fn hydrate_games(
    state: &AppState,
    conn: &Connection,
    games: &mut [GameRecord],
) -> AppResult<()> {
    for record in games.iter_mut() {
        let game = &mut record.game;
        let id = game.id.to_string();

        let mut rows = conn
            .query(
                "SELECT DISTINCT c.platform FROM channels c
                 JOIN builds b ON b.id = c.current_build_id
                 WHERE c.game_id = ?1 AND b.state = 'ready'",
                [id.as_str()],
            )
            .await?;
        let mut platforms = Vec::new();
        while let Some(row) = rows.next().await? {
            platforms.push(parsed(&row, 0, Platform::parse)?);
        }
        platforms.sort();
        game.platforms = platforms;

        let mut rows = conn
            .query(
                "SELECT id, kind, position FROM media WHERE game_id = ?1
                 ORDER BY kind, position, created_at",
                [id.as_str()],
            )
            .await?;
        let mut media = Vec::new();
        while let Some(row) = rows.next().await? {
            let media_id = uuid(&row, 0)?;
            let key = Storage::media_key(game.id, media_id);
            media.push(Media {
                id: media_id,
                kind: parsed(&row, 1, MediaKind::parse)?,
                url: match &state.storage {
                    Some(storage) => storage.public_url(&key),
                    None => key,
                },
                position: row.get(2)?,
            });
        }
        game.media = media;
    }
    Ok(())
}

// ---------------------------------------------------------------- builds

pub const BUILD_SELECT: &str = "SELECT b.id, g.slug, b.channel, b.platform, b.version, b.state, \
     b.total_size, b.chunk_count, b.created_at, b.game_id, b.launch, b.scan_status, \
     b.scan_findings, b.scanned_at, b.failure, b.archive_size \
     FROM builds b JOIN games g ON g.id = b.game_id";

pub struct BuildRecord {
    pub build: Build,
    pub game_id: Uuid,
    /// Size of the build's zip in storage, if it has one.
    pub archive_size: Option<u64>,
}

pub fn build_from_row(row: &Row) -> anyhow::Result<BuildRecord> {
    let scan = match row.get::<Option<String>>(11)? {
        Some(status) => Some(BuildScan {
            status: ScanStatus::parse(&status)
                .with_context(|| format!("unknown scan status {status:?}"))?,
            findings: json(row, 12)?,
            scanned_at: opt_ts(row, 13)?.context("scanned build without scanned_at")?,
        }),
        None => None,
    };
    let launch = row
        .get::<Option<String>>(10)?
        .map(|l| serde_json::from_str::<LaunchTarget>(&l))
        .transpose()?;
    Ok(BuildRecord {
        build: Build {
            id: uuid(row, 0)?,
            game: row.get(1)?,
            channel: row.get(2)?,
            platform: parsed(row, 3, Platform::parse)?,
            version: row.get(4)?,
            state: parsed(row, 5, BuildState::parse)?,
            total_size: u64_col(row, 6)?,
            chunk_count: u64_col(row, 7)?,
            created_at: ts(row, 8)?,
            launch,
            scan,
            failure: row.get(14)?,
        },
        game_id: uuid(row, 9)?,
        archive_size: row.get::<Option<i64>>(15)?.map(u64::try_from).transpose()?,
    })
}

/// Name of a build's zip without `.zip`, e.g. `lantern-1.0.2-windows`. Also
/// the folder inside the zip. Falls back to the build id's first block when
/// there is no version.
pub fn archive_stem(build: &Build) -> String {
    let version = build
        .version
        .as_deref()
        .map(|v| {
            v.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        })
        .map(|v| v.trim_matches(['-', '.']).to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| build.id.simple().to_string()[..8].to_string());
    format!("{}-{version}-{}", build.game, build.platform.as_str())
}

pub async fn build_by_id(conn: &Connection, id: Uuid) -> AppResult<BuildRecord> {
    let mut rows = conn
        .query(&format!("{BUILD_SELECT} WHERE b.id = ?1"), [id.to_string()])
        .await?;
    let row = rows.next().await?.ok_or(AppError::NotFound("build"))?;
    Ok(build_from_row(&row)?)
}

/// Builds of a game whose malware scan found something or could not finish,
/// newest first, for the review queue.
pub async fn flagged_builds(conn: &Connection, game_id: Uuid) -> AppResult<Vec<Build>> {
    let mut rows = conn
        .query(
            &format!(
                "{BUILD_SELECT} WHERE b.game_id = ?1 AND b.scan_status IN ('infected', 'incomplete')
                 ORDER BY b.created_at DESC LIMIT 20"
            ),
            [game_id.to_string()],
        )
        .await?;
    let mut builds = Vec::new();
    while let Some(row) = rows.next().await? {
        builds.push(build_from_row(&row)?.build);
    }
    Ok(builds)
}

// ---------------------------------------------------------------- submissions

pub const SUBMISSION_SELECT: &str = "SELECT s.id, g.slug, s.status, s.developer_notes, \
     s.reviewer_id, r.username, s.decision_notes, s.rule, s.created_at, s.decided_at, s.game_id \
     FROM submissions s JOIN games g ON g.id = s.game_id LEFT JOIN users r ON r.id = s.reviewer_id";

pub struct SubmissionRecord {
    pub submission: Submission,
    pub game_id: Uuid,
}

pub fn submission_from_row(row: &Row) -> anyhow::Result<SubmissionRecord> {
    let reviewer = match row.get::<Option<String>>(4)? {
        Some(id) => Some(UserSummary {
            id: Uuid::parse_str(&id)?,
            username: row.get(5)?,
        }),
        None => None,
    };
    let rule = row
        .get::<Option<String>>(7)?
        .map(|r| PolicyRule::parse(&r).with_context(|| format!("unknown rule {r:?}")))
        .transpose()?;
    Ok(SubmissionRecord {
        submission: Submission {
            id: uuid(row, 0)?,
            game: row.get(1)?,
            status: parsed(row, 2, SubmissionStatus::parse)?,
            developer_notes: row.get(3)?,
            reviewer,
            decision_notes: row.get(6)?,
            rule,
            created_at: ts(row, 8)?,
            decided_at: opt_ts(row, 9)?,
        },
        game_id: uuid(row, 10)?,
    })
}

pub async fn submission_by_id(conn: &Connection, id: Uuid) -> AppResult<SubmissionRecord> {
    let mut rows = conn
        .query(
            &format!("{SUBMISSION_SELECT} WHERE s.id = ?1"),
            [id.to_string()],
        )
        .await?;
    let row = rows.next().await?.ok_or(AppError::NotFound("submission"))?;
    Ok(submission_from_row(&row)?)
}

// ---------------------------------------------------------------- paging

/// Opaque cursors are plain offsets for now; clients must not rely on that.
pub fn parse_cursor(cursor: Option<&str>) -> AppResult<i64> {
    match cursor {
        None | Some("") => Ok(0),
        Some(c) => c
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| AppError::validation("invalid cursor")),
    }
}

pub fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(24).clamp(1, 100)
}

/// Builds a page from `limit + 1` fetched rows.
pub fn page<T>(mut items: Vec<T>, offset: i64, limit: i64) -> foyer_types::Page<T> {
    let more = items.len() as i64 > limit;
    items.truncate(limit as usize);
    foyer_types::Page {
        next_cursor: more.then(|| (offset + limit).to_string()),
        items,
    }
}
