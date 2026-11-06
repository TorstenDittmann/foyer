//! Moderation after review: player reports, developer appeals and strikes.
//!
//! - Players report a published game, signed in or not. Reports are rate
//!   limited per address, and each reporter has at most one open report per
//!   game. Nothing happens automatically: reports wait in a staff queue,
//!   where a reviewer dismisses them or delists the game (`review.rs`).
//! - A developer can appeal a rejection or a delisting once. A reviewer other
//!   than the one who made the decision upholds it, overturns it (the game is
//!   published) or reopens the game for changes. Overturning or reopening
//!   revokes the strikes that came with the decision.
//! - Strikes are a per-developer record of violations, issued with a
//!   rejection or delisting or on their own. They inform staff; they don't
//!   ban anyone by themselves.

use std::net::IpAddr;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use foyer_types::{
    ApiError, Appeal, AppealDecision, AppealItem, AppealOutcome, AppealStatus, CreateAppeal,
    CreateReport, CreateStrike, DismissReports, GameStatus, Page, ReasonCount, Report,
    ReportReason, ReportStatus, ReportedGame, ResolvedReports, RevokeStrike, Strike,
    SubmissionStatus,
};
use serde::Deserialize;
use utoipa::IntoParams;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::review::{is_admin, require_not_own_game};
use crate::AppState;
use crate::auth::{self, CurrentUser, MaybeUser};
use crate::error::{AppError, AppResult, JsonBody, PathParam, QueryParams};
use crate::model::{self, APPEAL_SELECT, REPORT_SELECT, STRIKE_SELECT, SUBMISSION_SELECT, fmt_ts};
use crate::ratelimit::{Bucket, ClientIp};
use crate::{mail, validate};

pub fn routes() -> OpenApiRouter<AppState> {
    let reports = OpenApiRouter::new().routes(routes!(create_report));
    OpenApiRouter::new()
        .merge(super::limited(reports, Bucket::Report))
        .routes(routes!(report_queue))
        .routes(routes!(game_reports))
        .routes(routes!(dismiss_reports))
        .routes(routes!(list_appeals, create_appeal))
        .routes(routes!(appeal_queue))
        .routes(routes!(decide_appeal))
        .routes(routes!(developer_strikes))
        .routes(routes!(create_strike))
        .routes(routes!(revoke_strike))
}

// ---------------------------------------------------------------- reports

/// Who filed a report, for de-duplication. Signed-in players by account;
/// anonymous ones by a hash of their address (an IPv6 address by its /64,
/// which one household usually shares). The address itself isn't stored.
fn reporter_key(user: Option<&CurrentUser>, ip: IpAddr) -> String {
    match user {
        Some(user) => format!("user:{}", user.id()),
        None => {
            let network = match ip {
                IpAddr::V4(v4) => v4.to_string(),
                IpAddr::V6(v6) => {
                    let s = v6.segments();
                    format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
                }
            };
            format!("ip:{}", auth::secret_hash(&format!("report:{network}")))
        }
    }
}

/// Report a published game to staff. Signing in is optional. A second
/// report from the same player while the first is still open is accepted
/// and ignored.
#[utoipa::path(
    post, path = "/v1/games/{slug}/reports", tag = "moderation",
    params(("slug" = String, Path)),
    request_body = CreateReport,
    responses(
        (status = 204, description = "Received"),
        (status = 400, body = ApiError),
        (status = 403, description = "Reporting your own game", body = ApiError),
        (status = 404, body = ApiError),
        (status = 429, description = "`rate_limited`, with `Retry-After`", body = ApiError),
    ),
)]
async fn create_report(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    ClientIp(ip): ClientIp,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<CreateReport>,
) -> AppResult<StatusCode> {
    if req.reason == ReportReason::Other {
        validate::required_text("details", &req.details, 2_000)?;
    } else {
        validate::text("details", &req.details, 2_000)?;
    }
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    if game.game.status != GameStatus::Published {
        return Err(AppError::NotFound("game"));
    }
    if user.as_ref().is_some_and(|u| game.is_owner(u)) {
        return Err(AppError::forbidden("you can't report your own game"));
    }
    conn.execute(
        "INSERT INTO reports (id, game_id, reason, details, reporter_id, reporter_key, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (game_id, reporter_key) WHERE status = 'open' DO NOTHING",
        libsql::params![
            Uuid::now_v7().to_string(),
            game.game.id.to_string(),
            req.reason.as_str(),
            req.details.trim(),
            user.as_ref().map(|u| u.id().to_string()),
            reporter_key(user.as_ref(), ip),
            fmt_ts(model::now()),
        ],
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

/// Games with open reports. Games reported for malware come first, then
/// the most reported, then the longest waiting. Reviewers only.
#[utoipa::path(
    get, path = "/v1/review/reports", tag = "moderation",
    security(("bearer" = [])),
    params(PageQuery),
    responses(
        (status = 200, body = Page<ReportedGame>),
        (status = 403, body = ApiError),
    ),
)]
async fn report_queue(
    State(state): State<AppState>,
    user: CurrentUser,
    QueryParams(query): QueryParams<PageQuery>,
) -> AppResult<Json<Page<ReportedGame>>> {
    user.require_staff()?;
    let offset = model::parse_cursor(query.cursor.as_deref())?;
    let limit = model::clamp_limit(query.limit);
    let conn = state.conn().await?;
    let mut rows = conn
        .query(
            "SELECT game_id, count(*) AS n, min(created_at) AS first, max(created_at)
             FROM reports WHERE status = 'open' GROUP BY game_id
             ORDER BY max(reason = 'malware') DESC, n DESC, first, game_id
             LIMIT ?1 OFFSET ?2",
            [limit + 1, offset],
        )
        .await?;
    let mut groups = Vec::new();
    while let Some(row) = rows.next().await? {
        groups.push((
            model::uuid(&row, 0)?,
            u64::try_from(row.get::<i64>(1)?).unwrap_or(0),
            model::ts(&row, 2)?,
            model::ts(&row, 3)?,
        ));
    }
    drop(rows);
    let mut items = Vec::with_capacity(groups.len());
    for (game_id, open_reports, first_reported_at, last_reported_at) in groups {
        let game = model::game_by_id(&state, &conn, game_id).await?.game;
        let mut rows = conn
            .query(
                "SELECT reason, count(*) AS n FROM reports
                 WHERE game_id = ?1 AND status = 'open'
                 GROUP BY reason ORDER BY n DESC, reason",
                [game_id.to_string()],
            )
            .await?;
        let mut reasons = Vec::new();
        while let Some(row) = rows.next().await? {
            let reason: String = row.get(0)?;
            reasons.push(ReasonCount {
                reason: ReportReason::parse(&reason)
                    .ok_or_else(|| anyhow::anyhow!("unknown report reason {reason:?}"))?,
                count: u64::try_from(row.get::<i64>(1)?).unwrap_or(0),
            });
        }
        drop(rows);
        let developer_strikes = model::active_strikes(&conn, &game.developer.slug).await?;
        items.push(ReportedGame {
            game,
            open_reports,
            reasons,
            first_reported_at,
            last_reported_at,
            developer_strikes,
        });
    }
    Ok(Json(model::page(items, offset, limit)))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct GameReportsQuery {
    /// Only reports with this status. Default: all.
    pub status: Option<ReportStatus>,
}

/// A game's reports, newest first (at most 200). Reviewers only.
#[utoipa::path(
    get, path = "/v1/review/games/{slug}/reports", tag = "moderation",
    security(("bearer" = [])),
    params(("slug" = String, Path), GameReportsQuery),
    responses(
        (status = 200, body = Vec<Report>),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn game_reports(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    QueryParams(query): QueryParams<GameReportsQuery>,
) -> AppResult<Json<Vec<Report>>> {
    user.require_staff()?;
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    let mut rows = conn
        .query(
            &format!(
                "{REPORT_SELECT} WHERE rp.game_id = ?1 AND (?2 IS NULL OR rp.status = ?2)
                 ORDER BY rp.created_at DESC, rp.id DESC LIMIT 200"
            ),
            libsql::params![game.game.id.to_string(), query.status.map(|s| s.as_str())],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(model::report_from_row(&row)?);
    }
    Ok(Json(out))
}

/// Close a game's open reports without acting on them. To act, delist the
/// game instead, which closes them as actioned. Reviewers only.
#[utoipa::path(
    post, path = "/v1/review/games/{slug}/reports/dismiss", tag = "moderation",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = DismissReports,
    responses(
        (status = 200, body = ResolvedReports),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn dismiss_reports(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<DismissReports>,
) -> AppResult<Json<ResolvedReports>> {
    user.require_staff()?;
    validate::text("notes", &req.notes, 10_000)?;
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    require_not_own_game(game.owner_id, &user)?;
    let notes = Some(req.notes.trim().to_string()).filter(|n| !n.is_empty());
    let resolved = conn
        .execute(
            "UPDATE reports SET status = 'dismissed', resolved_by = ?1, resolution_notes = ?2,
               resolved_at = ?3
             WHERE game_id = ?4 AND status = 'open'",
            libsql::params![
                user.id().to_string(),
                notes,
                fmt_ts(model::now()),
                game.game.id.to_string(),
            ],
        )
        .await?;
    Ok(Json(ResolvedReports { resolved }))
}

// ---------------------------------------------------------------- appeals

/// A game's appeals, newest first. Developer and reviewers only.
#[utoipa::path(
    get, path = "/v1/games/{slug}/appeals", tag = "moderation",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = Vec<Appeal>),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn list_appeals(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Vec<Appeal>>> {
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    if !user.is_staff() {
        game.require_owner(&user)?;
    }
    let appeals = model::query_appeals(
        &conn,
        &format!("{APPEAL_SELECT} WHERE a.game_id = ?1 ORDER BY a.created_at DESC, a.id DESC"),
        [game.game.id.to_string()],
    )
    .await?;
    Ok(Json(appeals.into_iter().map(|a| a.appeal).collect()))
}

/// Appeal the game's latest decision, if it was a rejection or a delisting.
/// Each decision can be appealed once.
#[utoipa::path(
    post, path = "/v1/games/{slug}/appeals", tag = "moderation",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = CreateAppeal,
    responses(
        (status = 201, body = Appeal),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`not_appealable`, `already_appealed` or `email_unverified`", body = ApiError),
    ),
)]
async fn create_appeal(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<CreateAppeal>,
) -> AppResult<(StatusCode, Json<Appeal>)> {
    validate::required_text("statement", &req.statement, 5_000)?;
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    game.require_owner(&user)?;
    user.require_verified()?;
    if !matches!(
        game.game.status,
        GameStatus::Rejected | GameStatus::Delisted
    ) {
        return Err(AppError::conflict(
            "not_appealable",
            "only a rejected or delisted game can be appealed",
        ));
    }
    let mut rows = conn
        .query(
            &format!(
                "{SUBMISSION_SELECT} WHERE s.game_id = ?1 AND s.decided_at IS NOT NULL
                 ORDER BY s.decided_at DESC, s.created_at DESC LIMIT 1"
            ),
            [game.game.id.to_string()],
        )
        .await?;
    let decision = match rows.next().await? {
        Some(row) => model::submission_from_row(&row)?,
        None => {
            return Err(AppError::conflict(
                "not_appealable",
                "there is no decision to appeal",
            ));
        }
    };
    drop(rows);
    if decision.submission.status != SubmissionStatus::Rejected {
        return Err(AppError::conflict(
            "not_appealable",
            "only a rejection or a delisting can be appealed",
        ));
    }
    let id = Uuid::now_v7();
    let inserted = conn
        .execute(
            "INSERT INTO appeals (id, submission_id, game_id, statement, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (submission_id) DO NOTHING",
            [
                id.to_string(),
                decision.submission.id.to_string(),
                game.game.id.to_string(),
                req.statement.trim().to_string(),
                fmt_ts(model::now()),
            ],
        )
        .await?;
    if inserted == 0 {
        return Err(AppError::conflict(
            "already_appealed",
            "this decision was appealed already",
        ));
    }
    Ok((
        StatusCode::CREATED,
        Json(model::appeal_by_id(&conn, id).await?.appeal),
    ))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AppealQueueQuery {
    /// Default: `pending`.
    pub status: Option<AppealStatus>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

/// Appeals, oldest first. Reviewers only.
#[utoipa::path(
    get, path = "/v1/review/appeals", tag = "moderation",
    security(("bearer" = [])),
    params(AppealQueueQuery),
    responses(
        (status = 200, body = Page<AppealItem>),
        (status = 403, body = ApiError),
    ),
)]
async fn appeal_queue(
    State(state): State<AppState>,
    user: CurrentUser,
    QueryParams(query): QueryParams<AppealQueueQuery>,
) -> AppResult<Json<Page<AppealItem>>> {
    user.require_staff()?;
    let status = query.status.unwrap_or(AppealStatus::Pending);
    let offset = model::parse_cursor(query.cursor.as_deref())?;
    let limit = model::clamp_limit(query.limit);
    let conn = state.conn().await?;
    let appeals = model::query_appeals(
        &conn,
        &format!(
            "{APPEAL_SELECT} WHERE a.status = ?1 ORDER BY a.created_at, a.id LIMIT ?2 OFFSET ?3"
        ),
        libsql::params![status.as_str(), limit + 1, offset],
    )
    .await?;
    let mut items = Vec::with_capacity(appeals.len());
    for record in appeals {
        let game = model::game_by_id(&state, &conn, record.game_id).await?.game;
        let developer_strikes = model::active_strikes(&conn, &game.developer.slug).await?;
        items.push(AppealItem {
            appeal: record.appeal,
            game,
            developer_strikes,
        });
    }
    Ok(Json(model::page(items, offset, limit)))
}

/// Decide an appeal. The reviewer who made the original decision can't,
/// unless they are an admin. Overturning publishes the game; reopening lets
/// the developer change it and resubmit. Both revoke the decision's strikes.
/// The developer gets an email either way.
#[utoipa::path(
    post, path = "/v1/review/appeals/{id}/decision", tag = "moderation",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = AppealDecision,
    responses(
        (status = 200, body = Appeal),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`already_decided`", body = ApiError),
    ),
)]
async fn decide_appeal(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
    JsonBody(req): JsonBody<AppealDecision>,
) -> AppResult<Json<Appeal>> {
    user.require_staff()?;
    validate::required_text("notes", &req.notes, 10_000)?;
    let conn = state.conn().await?;
    let record = model::appeal_by_id(&conn, id).await?;
    let game = model::game_by_id(&state, &conn, record.game_id).await?;
    require_not_own_game(game.owner_id, &user)?;
    if record.appeal.status != AppealStatus::Pending {
        return Err(AppError::conflict(
            "already_decided",
            "this appeal has a decision",
        ));
    }
    let original = record.appeal.submission.reviewer.as_ref().map(|r| r.id);
    if original == Some(user.id()) && !is_admin(&user) {
        return Err(AppError::forbidden(
            "another reviewer has to decide an appeal of your own decision",
        ));
    }
    let owner = model::user_by_id(&conn, game.owner_id).await?;
    let notes = req.notes.trim().to_string();
    let now = fmt_ts(model::now());
    let tx = conn.transaction().await?;
    let updated = tx
        .execute(
            "UPDATE appeals SET status = ?1, reviewer_id = ?2, decision_notes = ?3, decided_at = ?4
             WHERE id = ?5 AND status = 'pending'",
            [
                req.outcome.status().as_str().to_string(),
                user.id().to_string(),
                notes.clone(),
                now.clone(),
                id.to_string(),
            ],
        )
        .await?;
    if updated == 0 {
        return Err(AppError::conflict(
            "already_decided",
            "this appeal has a decision",
        ));
    }
    let game_status = match req.outcome {
        AppealOutcome::Upheld => None,
        AppealOutcome::Overturned => Some(GameStatus::Published),
        AppealOutcome::Reopened => Some(GameStatus::ChangesRequested),
    };
    if let Some(status) = game_status {
        tx.execute(
            "UPDATE games SET status = ?1, updated_at = ?2,
               published_at = CASE WHEN ?1 = 'published' THEN coalesce(published_at, ?2)
                                   ELSE published_at END
             WHERE id = ?3 AND status IN ('rejected', 'delisted')",
            [
                status.as_str().to_string(),
                now.clone(),
                record.game_id.to_string(),
            ],
        )
        .await?;
        tx.execute(
            "UPDATE strikes SET revoked_at = ?1, revoked_by = ?2, revoke_reason = ?3
             WHERE submission_id = ?4 AND revoked_at IS NULL",
            [
                now,
                user.id().to_string(),
                format!("Appeal {}", req.outcome.status().as_str()),
                record.appeal.submission.id.to_string(),
            ],
        )
        .await?;
    }
    let link = state.web_link(&format!("/dashboard/games/{}", game.game.slug), &[]);
    mail::queue(
        &tx,
        &mail::appeal_decided(
            &owner.email,
            &owner.username,
            &game.game.title,
            req.outcome,
            &notes,
            &link,
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(model::appeal_by_id(&conn, id).await?.appeal))
}

// ---------------------------------------------------------------- strikes

/// A developer's strikes, newest first, revoked ones included. The
/// developer and reviewers only.
#[utoipa::path(
    get, path = "/v1/developers/{slug}/strikes", tag = "moderation",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = Vec<Strike>),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn developer_strikes(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Vec<Strike>>> {
    let conn = state.conn().await?;
    let developer = model::developer_by_slug(&conn, &slug).await?;
    if developer.owner_id != user.id() && !user.is_staff() {
        return Err(AppError::forbidden(
            "only the developer and reviewers can see strikes",
        ));
    }
    let mut rows = conn
        .query(
            &format!(
                "{STRIKE_SELECT} WHERE st.developer_id = ?1 ORDER BY st.created_at DESC, st.id DESC"
            ),
            [developer.developer.id.to_string()],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(model::strike_from_row(&row)?);
    }
    Ok(Json(out))
}

/// Record a strike against a developer, optionally for one of their games.
/// Reviewers only.
#[utoipa::path(
    post, path = "/v1/review/developers/{slug}/strikes", tag = "moderation",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = CreateStrike,
    responses(
        (status = 201, body = Strike),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn create_strike(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<CreateStrike>,
) -> AppResult<(StatusCode, Json<Strike>)> {
    user.require_staff()?;
    validate::required_text("notes", &req.notes, 10_000)?;
    let conn = state.conn().await?;
    let developer = model::developer_by_slug(&conn, &slug).await?;
    if developer.owner_id == user.id() {
        return Err(AppError::forbidden("you can't give yourself a strike"));
    }
    let game_id = match req.game.as_deref().filter(|g| !g.is_empty()) {
        Some(game) => {
            let game = model::game_by_slug(&state, &conn, game).await?;
            if game.game.developer.slug != developer.developer.slug {
                return Err(AppError::validation(
                    "that game belongs to another developer",
                ));
            }
            Some(game.game.id.to_string())
        }
        None => None,
    };
    let id = Uuid::now_v7();
    conn.execute(
        "INSERT INTO strikes (id, developer_id, game_id, rule, notes, issued_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        libsql::params![
            id.to_string(),
            developer.developer.id.to_string(),
            game_id,
            req.rule.as_str(),
            req.notes.trim(),
            user.id().to_string(),
            fmt_ts(model::now()),
        ],
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(model::strike_by_id(&conn, id).await?),
    ))
}

/// Take a strike back. Reviewers only.
#[utoipa::path(
    post, path = "/v1/review/strikes/{id}/revoke", tag = "moderation",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = RevokeStrike,
    responses(
        (status = 200, body = Strike),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`already_revoked`", body = ApiError),
    ),
)]
async fn revoke_strike(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
    JsonBody(req): JsonBody<RevokeStrike>,
) -> AppResult<Json<Strike>> {
    user.require_staff()?;
    validate::required_text("reason", &req.reason, 2_000)?;
    let conn = state.conn().await?;
    let strike = model::strike_by_id(&conn, id).await?;
    let developer = model::developer_by_slug(&conn, &strike.developer).await?;
    if developer.owner_id == user.id() {
        return Err(AppError::forbidden("you can't revoke your own strikes"));
    }
    let updated = conn
        .execute(
            "UPDATE strikes SET revoked_at = ?1, revoked_by = ?2, revoke_reason = ?3
             WHERE id = ?4 AND revoked_at IS NULL",
            [
                fmt_ts(model::now()),
                user.id().to_string(),
                req.reason.trim().to_string(),
                id.to_string(),
            ],
        )
        .await?;
    if updated == 0 {
        return Err(AppError::conflict(
            "already_revoked",
            "this strike was revoked already",
        ));
    }
    Ok(Json(model::strike_by_id(&conn, id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_reporters_are_keyed_by_network() {
        let a = reporter_key(None, "2001:db8:1:2:aaaa::1".parse().unwrap());
        let b = reporter_key(None, "2001:db8:1:2:bbbb::9".parse().unwrap());
        let c = reporter_key(None, "2001:db8:1:3::1".parse().unwrap());
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("ip:") && !a.contains("2001"));
        let v4 = reporter_key(None, "203.0.113.7".parse().unwrap());
        assert_ne!(v4, reporter_key(None, "203.0.113.8".parse().unwrap()));
    }
}
