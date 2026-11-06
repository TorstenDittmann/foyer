//! Mapping between database rows and the wire types in `foyer-types`, plus
//! the loaders the handlers share.

use anyhow::Context;
use chrono::{DateTime, SecondsFormat, Utc};
use foyer_types::{
    Appeal, AppealStatus, Build, BuildScan, BuildState, Developer, DeveloperSummary, Game,
    GameStatus, LaunchTarget, Media, MediaKind, Platform, PolicyRule, Report, ReportReason,
    ReportStatus, Role, ScanStatus, Strike, Submission, SubmissionKind, SubmissionStatus, User,
    UserSummary,
};
use libsql::{Connection, Row};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::AppState;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult};
use crate::storage::Storage;
use crate::testers::{KeyGrant, Viewer};

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

    /// The developer and reviewers see everything about a game.
    pub fn is_privileged(&self, user: Option<&CurrentUser>) -> bool {
        user.is_some_and(|u| self.is_owner(u) || u.is_staff())
    }

    /// Tester keys stop working once a game is rejected or delisted.
    fn keys_work(&self) -> bool {
        !matches!(
            self.game.status,
            GameStatus::Rejected | GameStatus::Delisted
        )
    }

    fn tester_grant<'a>(&self, viewer: &'a Viewer) -> Option<&'a KeyGrant> {
        self.keys_work()
            .then(|| viewer.grant(self.game.id))
            .flatten()
    }

    /// Like [`Self::can_view`], counting the viewer's tester keys.
    pub fn viewer_can_view(&self, viewer: &Viewer) -> bool {
        self.can_view(viewer.user()) || self.tester_grant(viewer).is_some()
    }

    /// 404 rather than 403 for games the caller can't see, so unpublished
    /// slugs don't leak.
    pub fn require_view(&self, viewer: &Viewer) -> AppResult<()> {
        if self.viewer_can_view(viewer) {
            Ok(())
        } else {
            Err(AppError::NotFound("game"))
        }
    }

    /// Whether the viewer may get builds from `channel`: public channels of
    /// published games, channels a tester key covers, and everything for the
    /// developer and reviewers.
    pub fn can_see_channel(&self, viewer: &Viewer, channel: &str, private: bool) -> bool {
        if self.is_privileged(viewer.user()) {
            return true;
        }
        if !private && self.game.status == GameStatus::Published {
            return true;
        }
        self.keys_work() && viewer.grant_covering(self.game.id, channel).is_some()
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

/// Fills in each game's platforms (from public channels serving a ready
/// build) and media.
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
                 WHERE c.game_id = ?1 AND b.state = 'ready' AND c.private = 0",
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

/// Names of a game's private channels.
pub async fn private_channels(
    conn: &Connection,
    game_id: Uuid,
) -> AppResult<std::collections::HashSet<String>> {
    let mut rows = conn
        .query(
            "SELECT name FROM channels WHERE game_id = ?1 AND private = 1",
            [game_id.to_string()],
        )
        .await?;
    let mut out = std::collections::HashSet::new();
    while let Some(row) = rows.next().await? {
        out.insert(row.get::<String>(0)?);
    }
    Ok(out)
}

/// Whether a game's channel is private. Channels that don't exist count as
/// public.
pub async fn channel_is_private(conn: &Connection, game_id: Uuid, name: &str) -> AppResult<bool> {
    let mut rows = conn
        .query(
            "SELECT private FROM channels WHERE game_id = ?1 AND name = ?2",
            [game_id.to_string(), name.to_string()],
        )
        .await?;
    Ok(match rows.next().await? {
        Some(row) => row.get::<i64>(0)? != 0,
        None => false,
    })
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
     s.reviewer_id, r.username, s.decision_notes, s.rule, s.created_at, s.decided_at, s.game_id, \
     s.kind FROM submissions s JOIN games g ON g.id = s.game_id LEFT JOIN users r ON r.id = s.reviewer_id";

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
            kind: parsed(row, 11, SubmissionKind::parse)?,
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

// ---------------------------------------------------------------- moderation

/// A user named by an optional id column followed by a username column.
fn user_summary(row: &Row, id_idx: i32) -> anyhow::Result<Option<UserSummary>> {
    match row.get::<Option<String>>(id_idx)? {
        Some(id) => Ok(Some(UserSummary {
            id: Uuid::parse_str(&id)?,
            username: row.get::<Option<String>>(id_idx + 1)?.unwrap_or_default(),
        })),
        None => Ok(None),
    }
}

pub const REPORT_SELECT: &str = "SELECT rp.id, g.slug, rp.reason, rp.details, rp.reporter_id, \
     ru.username, rp.status, rp.resolved_by, su.username, rp.resolution_notes, rp.created_at, \
     rp.resolved_at FROM reports rp JOIN games g ON g.id = rp.game_id \
     LEFT JOIN users ru ON ru.id = rp.reporter_id LEFT JOIN users su ON su.id = rp.resolved_by";

pub fn report_from_row(row: &Row) -> anyhow::Result<Report> {
    Ok(Report {
        id: uuid(row, 0)?,
        game: row.get(1)?,
        reason: parsed(row, 2, ReportReason::parse)?,
        details: row.get(3)?,
        reporter: user_summary(row, 4)?,
        status: parsed(row, 6, ReportStatus::parse)?,
        resolved_by: user_summary(row, 7)?,
        resolution_notes: row.get(9)?,
        created_at: ts(row, 10)?,
        resolved_at: opt_ts(row, 11)?,
    })
}

pub const STRIKE_SELECT: &str = "SELECT st.id, d.slug, g.slug, st.rule, st.notes, st.issued_by, \
     iu.username, st.created_at, st.revoked_at, st.revoke_reason FROM strikes st \
     JOIN developers d ON d.id = st.developer_id LEFT JOIN games g ON g.id = st.game_id \
     LEFT JOIN users iu ON iu.id = st.issued_by";

pub fn strike_from_row(row: &Row) -> anyhow::Result<Strike> {
    Ok(Strike {
        id: uuid(row, 0)?,
        developer: row.get(1)?,
        game: row.get(2)?,
        rule: parsed(row, 3, PolicyRule::parse)?,
        notes: row.get(4)?,
        issued_by: user_summary(row, 5)?,
        created_at: ts(row, 7)?,
        revoked_at: opt_ts(row, 8)?,
        revoke_reason: row.get(9)?,
    })
}

pub async fn strike_by_id(conn: &Connection, id: Uuid) -> AppResult<Strike> {
    let mut rows = conn
        .query(
            &format!("{STRIKE_SELECT} WHERE st.id = ?1"),
            [id.to_string()],
        )
        .await?;
    let row = rows.next().await?.ok_or(AppError::NotFound("strike"))?;
    Ok(strike_from_row(&row)?)
}

/// Strikes against the developer with this slug that haven't been revoked.
pub async fn active_strikes(conn: &Connection, developer_slug: &str) -> AppResult<u64> {
    let mut rows = conn
        .query(
            "SELECT count(*) FROM strikes st JOIN developers d ON d.id = st.developer_id
             WHERE d.slug = ?1 AND st.revoked_at IS NULL",
            [developer_slug],
        )
        .await?;
    let row = rows.next().await?.context("count returned no rows")?;
    Ok(u64_col(&row, 0)?)
}

/// Records a strike against the developer of `game_id`. Pass a transaction
/// to tie it to the change it comes with.
pub async fn insert_strike(
    conn: &Connection,
    game_id: Uuid,
    submission_id: Option<Uuid>,
    rule: PolicyRule,
    notes: &str,
    issued_by: Uuid,
) -> AppResult<Uuid> {
    let id = Uuid::now_v7();
    conn.execute(
        "INSERT INTO strikes (id, developer_id, game_id, submission_id, rule, notes, issued_by,
           created_at)
         SELECT ?1, g.developer_id, g.id, ?2, ?3, ?4, ?5, ?6 FROM games g WHERE g.id = ?7",
        libsql::params![
            id.to_string(),
            submission_id.map(|s| s.to_string()),
            rule.as_str(),
            notes.trim(),
            issued_by.to_string(),
            fmt_ts(now()),
            game_id.to_string(),
        ],
    )
    .await?;
    Ok(id)
}

pub const APPEAL_SELECT: &str = "SELECT a.id, g.slug, a.statement, a.status, a.reviewer_id, \
     ar.username, a.decision_notes, a.created_at, a.decided_at, a.submission_id, a.game_id \
     FROM appeals a JOIN games g ON g.id = a.game_id LEFT JOIN users ar ON ar.id = a.reviewer_id";

pub struct AppealRecord {
    pub appeal: Appeal,
    pub game_id: Uuid,
}

/// Runs an [`APPEAL_SELECT`] query and loads the decision each appeals.
pub async fn query_appeals(
    conn: &Connection,
    sql: &str,
    params: impl libsql::params::IntoParams,
) -> AppResult<Vec<AppealRecord>> {
    // Read every row before running more queries: a row is only valid until
    // the next one is fetched.
    let mut rows = conn.query(sql, params).await?;
    let mut raw = Vec::new();
    while let Some(row) = rows.next().await? {
        let appeal = Appeal {
            id: uuid(&row, 0)?,
            game: row.get(1)?,
            submission: Submission {
                id: uuid(&row, 9)?,
                game: String::new(),
                kind: SubmissionKind::Review,
                status: SubmissionStatus::Rejected,
                developer_notes: String::new(),
                reviewer: None,
                decision_notes: None,
                rule: None,
                created_at: ts(&row, 7)?,
                decided_at: None,
            },
            statement: row.get(2)?,
            status: parsed(&row, 3, AppealStatus::parse)?,
            reviewer: user_summary(&row, 4)?,
            decision_notes: row.get(6)?,
            created_at: ts(&row, 7)?,
            decided_at: opt_ts(&row, 8)?,
        };
        raw.push(AppealRecord {
            appeal,
            game_id: uuid(&row, 10)?,
        });
    }
    drop(rows);
    for record in &mut raw {
        record.appeal.submission = submission_by_id(conn, record.appeal.submission.id)
            .await?
            .submission;
    }
    Ok(raw)
}

pub async fn appeal_by_id(conn: &Connection, id: Uuid) -> AppResult<AppealRecord> {
    query_appeals(
        conn,
        &format!("{APPEAL_SELECT} WHERE a.id = ?1"),
        [id.to_string()],
    )
    .await?
    .pop()
    .ok_or(AppError::NotFound("appeal"))
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
