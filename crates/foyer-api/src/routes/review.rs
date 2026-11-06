//! Submissions and the staff review queue.
//!
//! A developer submits a game; it waits in the queue as `pending`. A reviewer
//! claims it (`in_review`), plays it, and records a decision citing a content
//! rule. Approval publishes the game; a change request lets the developer
//! edit and resubmit; a rejection is final (appeals come later).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use foyer_types::{
    ApiError, CreateSubmission, Decision, DecisionOutcome, Delist, Game, GameStatus, Page,
    QueueItem, Role, Submission, SubmissionStatus,
};
use serde::Deserialize;
use utoipa::IntoParams;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, JsonBody, PathParam, QueryParams};
use crate::model::{self, SUBMISSION_SELECT, fmt_ts};
use crate::validate;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_submissions, create_submission))
        .routes(routes!(queue))
        .routes(routes!(claim))
        .routes(routes!(release))
        .routes(routes!(decide))
        .routes(routes!(delist))
}

/// A game's submissions and their decisions, newest first. Developer and
/// reviewers only.
#[utoipa::path(
    get, path = "/v1/games/{slug}/submissions", tag = "review",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    responses(
        (status = 200, body = Vec<Submission>),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn list_submissions(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
) -> AppResult<Json<Vec<Submission>>> {
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    if !user.is_staff() {
        game.require_owner(&user)?;
    }
    let mut rows = conn
        .query(
            &format!("{SUBMISSION_SELECT} WHERE s.game_id = ?1 ORDER BY s.created_at DESC"),
            [game.game.id.to_string()],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(model::submission_from_row(&row)?.submission);
    }
    Ok(Json(out))
}

/// Submit a draft (or a game with requested changes) for review. Needs a
/// description and at least one ready build.
#[utoipa::path(
    post, path = "/v1/games/{slug}/submissions", tag = "review",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = CreateSubmission,
    responses(
        (status = 201, body = Submission),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`not_submittable`, `no_ready_build` or `email_unverified`", body = ApiError),
    ),
)]
async fn create_submission(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<CreateSubmission>,
) -> AppResult<(StatusCode, Json<Submission>)> {
    validate::text("developer_notes", &req.developer_notes, 10_000)?;
    let conn = state.conn().await?;
    let record = model::game_by_slug(&state, &conn, &slug).await?;
    record.require_owner(&user)?;
    user.require_verified()?;
    let game = &record.game;
    if !matches!(
        game.status,
        GameStatus::Draft | GameStatus::ChangesRequested
    ) {
        return Err(AppError::conflict(
            "not_submittable",
            format!(
                "a game that is {} can't be submitted",
                game.status.as_str().replace('_', " ")
            ),
        ));
    }
    if game.description.trim().is_empty() {
        return Err(AppError::validation("add a description before submitting"));
    }
    if game.platforms.is_empty() {
        return Err(AppError::conflict(
            "no_ready_build",
            "push at least one build before submitting",
        ));
    }

    let id = Uuid::now_v7();
    let now = fmt_ts(model::now());
    let tx = conn.transaction().await?;
    // Guard on the status we checked, so two concurrent submits can't both
    // pass.
    let updated = tx
        .execute(
            "UPDATE games SET status = 'in_review', updated_at = ?1
             WHERE id = ?2 AND status IN ('draft', 'changes_requested')",
            [now.clone(), game.id.to_string()],
        )
        .await?;
    if updated == 0 {
        return Err(AppError::conflict(
            "not_submittable",
            "the game was submitted already",
        ));
    }
    tx.execute(
        "INSERT INTO submissions (id, game_id, status, developer_notes, created_at)
         VALUES (?1, ?2, 'pending', ?3, ?4)",
        [
            id.to_string(),
            game.id.to_string(),
            req.developer_notes,
            now,
        ],
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(model::submission_by_id(&conn, id).await?.submission),
    ))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct QueueQuery {
    /// `pending` or `in_review`. Default: both.
    pub status: Option<SubmissionStatus>,
    /// Only submissions claimed by the caller.
    #[serde(default)]
    pub mine: bool,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

/// Open submissions, oldest first. Reviewers only.
#[utoipa::path(
    get, path = "/v1/review/queue", tag = "review",
    security(("bearer" = [])),
    params(QueueQuery),
    responses(
        (status = 200, body = Page<QueueItem>),
        (status = 403, body = ApiError),
    ),
)]
async fn queue(
    State(state): State<AppState>,
    user: CurrentUser,
    QueryParams(query): QueryParams<QueueQuery>,
) -> AppResult<Json<Page<QueueItem>>> {
    user.require_staff()?;
    let statuses = match query.status {
        None => vec!["pending", "in_review"],
        Some(s @ (SubmissionStatus::Pending | SubmissionStatus::InReview)) => vec![s.as_str()],
        Some(_) => {
            return Err(AppError::validation("status must be pending or in_review"));
        }
    };
    let offset = model::parse_cursor(query.cursor.as_deref())?;
    let limit = model::clamp_limit(query.limit);
    let conn = state.conn().await?;
    let mut rows = conn
        .query(
            &format!(
                "{SUBMISSION_SELECT}
                 WHERE s.status IN (?1, ?2) AND (?3 IS NULL OR s.reviewer_id = ?3)
                 ORDER BY s.created_at, s.id LIMIT ?4 OFFSET ?5"
            ),
            libsql::params![
                statuses[0],
                statuses.last().copied().unwrap_or(statuses[0]),
                query.mine.then(|| user.id().to_string()),
                limit + 1,
                offset,
            ],
        )
        .await?;
    let mut submissions = Vec::new();
    while let Some(row) = rows.next().await? {
        submissions.push(model::submission_from_row(&row)?);
    }
    drop(rows);
    let mut items = Vec::with_capacity(submissions.len());
    for record in submissions {
        let game = model::game_by_id(&state, &conn, record.game_id).await?.game;
        let flagged_builds = model::flagged_builds(&conn, record.game_id).await?;
        items.push(QueueItem {
            submission: record.submission,
            game,
            flagged_builds,
        });
    }
    Ok(Json(model::page(items, offset, limit)))
}

/// Reviewers can't review their own games.
fn require_not_own_game(owner_id: Uuid, user: &CurrentUser) -> AppResult<()> {
    if owner_id == user.id() {
        Err(AppError::forbidden("you can't review your own game"))
    } else {
        Ok(())
    }
}

/// Admins can take over or release a submission another reviewer claimed.
fn is_admin(user: &CurrentUser) -> bool {
    user.user.role == Role::Admin
}

/// Put a claimed submission back in the queue. The claiming reviewer or an
/// admin can do this, so an abandoned claim never blocks a game.
#[utoipa::path(
    post, path = "/v1/review/submissions/{id}/release", tag = "review",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Submission),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`not_claimed`", body = ApiError),
    ),
)]
async fn release(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
) -> AppResult<Json<Submission>> {
    user.require_staff()?;
    let conn = state.conn().await?;
    let record = model::submission_by_id(&conn, id).await?;
    if record.submission.status != SubmissionStatus::InReview {
        return Err(AppError::conflict(
            "not_claimed",
            "this submission is not claimed",
        ));
    }
    let claimant = record.submission.reviewer.as_ref().map(|r| r.id);
    if claimant != Some(user.id()) && !is_admin(&user) {
        return Err(AppError::forbidden(
            "only the claiming reviewer or an admin can release this",
        ));
    }
    conn.execute(
        "UPDATE submissions SET status = 'pending', reviewer_id = NULL, claimed_at = NULL
         WHERE id = ?1 AND status = 'in_review'",
        [id.to_string()],
    )
    .await?;
    Ok(Json(model::submission_by_id(&conn, id).await?.submission))
}

/// Claim a submission, so other reviewers know it is taken. Admins can take
/// over another reviewer's claim.
#[utoipa::path(
    post, path = "/v1/review/submissions/{id}/claim", tag = "review",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Submission),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`already_claimed` or `already_decided`", body = ApiError),
    ),
)]
async fn claim(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
) -> AppResult<Json<Submission>> {
    user.require_staff()?;
    let conn = state.conn().await?;
    let record = model::submission_by_id(&conn, id).await?;
    let game = model::game_by_id(&state, &conn, record.game_id).await?;
    require_not_own_game(game.owner_id, &user)?;
    if record.submission.status.is_decided() {
        return Err(AppError::conflict(
            "already_decided",
            "this submission has a decision",
        ));
    }
    let updated = conn
        .execute(
            "UPDATE submissions SET status = 'in_review', reviewer_id = ?1, claimed_at = ?2
             WHERE id = ?3 AND (status = 'pending' OR (status = 'in_review' AND (reviewer_id = ?1 OR ?4)))",
            libsql::params![user.id().to_string(), fmt_ts(model::now()), id.to_string(), is_admin(&user)],
        )
        .await?;
    if updated == 0 {
        return Err(AppError::conflict(
            "already_claimed",
            "another reviewer claimed this submission",
        ));
    }
    Ok(Json(model::submission_by_id(&conn, id).await?.submission))
}

/// Record a decision. Claims the submission if nobody has; admins can decide
/// on another reviewer's claim.
#[utoipa::path(
    post, path = "/v1/review/submissions/{id}/decision", tag = "review",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = Decision,
    responses(
        (status = 200, body = Submission),
        (status = 400, description = "Missing notes or rule", body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`already_claimed` or `already_decided`", body = ApiError),
    ),
)]
async fn decide(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
    JsonBody(req): JsonBody<Decision>,
) -> AppResult<Json<Submission>> {
    user.require_staff()?;
    validate::text("notes", &req.notes, 10_000)?;
    match req.outcome {
        DecisionOutcome::Approved => {}
        DecisionOutcome::ChangesRequested => {
            validate::required_text("notes", &req.notes, 10_000)?;
        }
        DecisionOutcome::Rejected => {
            validate::required_text("notes", &req.notes, 10_000)?;
            if req.rule.is_none() {
                return Err(AppError::validation(
                    "a rejection must cite the rule it is based on",
                ));
            }
        }
    }

    let conn = state.conn().await?;
    let record = model::submission_by_id(&conn, id).await?;
    let game = model::game_by_id(&state, &conn, record.game_id).await?;
    require_not_own_game(game.owner_id, &user)?;
    if record.submission.status.is_decided() {
        return Err(AppError::conflict(
            "already_decided",
            "this submission has a decision",
        ));
    }

    let now = fmt_ts(model::now());
    let notes = Some(req.notes.trim().to_string()).filter(|n| !n.is_empty());
    let tx = conn.transaction().await?;
    let updated = tx
        .execute(
            "UPDATE submissions SET status = ?1, reviewer_id = ?2, decision_notes = ?3, rule = ?4,
               claimed_at = coalesce(claimed_at, ?5), decided_at = ?5
             WHERE id = ?6 AND (status = 'pending' OR (status = 'in_review' AND (reviewer_id = ?2 OR ?7)))",
            libsql::params![
                req.outcome.submission_status().as_str(),
                user.id().to_string(),
                notes,
                req.rule.map(|r| r.as_str()),
                now.clone(),
                id.to_string(),
                is_admin(&user),
            ],
        )
        .await?;
    if updated == 0 {
        return Err(AppError::conflict(
            "already_claimed",
            "another reviewer claimed this submission",
        ));
    }
    tx.execute(
        "UPDATE games SET status = ?1, updated_at = ?2,
           published_at = CASE WHEN ?1 = 'published' THEN coalesce(published_at, ?2)
                               ELSE published_at END
         WHERE id = ?3",
        [
            req.outcome.game_status().as_str(),
            &now,
            &record.game_id.to_string(),
        ],
    )
    .await?;
    // First release: tell the developer's followers. Re-approvals after an
    // update don't announce the game again.
    if req.outcome == DecisionOutcome::Approved && game.game.published_at.is_none() {
        crate::releases::queue(&tx, record.game_id).await?;
    }
    tx.commit().await?;
    Ok(Json(model::submission_by_id(&conn, id).await?.submission))
}

/// Take a published game down for a policy violation. The reason is recorded
/// as a rejected submission so it shows up in the game's review history.
#[utoipa::path(
    post, path = "/v1/review/games/{slug}/delist", tag = "review",
    security(("bearer" = [])),
    params(("slug" = String, Path)),
    request_body = Delist,
    responses(
        (status = 200, body = Game),
        (status = 400, body = ApiError),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
        (status = 409, description = "`not_published`", body = ApiError),
    ),
)]
async fn delist(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(slug): PathParam<String>,
    JsonBody(req): JsonBody<Delist>,
) -> AppResult<Json<Game>> {
    user.require_staff()?;
    validate::required_text("notes", &req.notes, 10_000)?;
    let conn = state.conn().await?;
    let game = model::game_by_slug(&state, &conn, &slug).await?;
    if game.game.status != GameStatus::Published {
        return Err(AppError::conflict(
            "not_published",
            "only published games can be delisted",
        ));
    }
    let now = fmt_ts(model::now());
    let tx = conn.transaction().await?;
    tx.execute(
        "UPDATE games SET status = 'delisted', updated_at = ?1 WHERE id = ?2",
        [now.clone(), game.game.id.to_string()],
    )
    .await?;
    tx.execute(
        "INSERT INTO submissions (id, game_id, status, developer_notes, reviewer_id,
           decision_notes, rule, created_at, claimed_at, decided_at)
         VALUES (?1, ?2, 'rejected', '', ?3, ?4, ?5, ?6, ?6, ?6)",
        [
            Uuid::now_v7().to_string(),
            game.game.id.to_string(),
            user.id().to_string(),
            format!("Delisted: {}", req.notes.trim()),
            req.rule.as_str().to_string(),
            now,
        ],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        model::game_by_id(&state, &conn, game.game.id).await?.game,
    ))
}
