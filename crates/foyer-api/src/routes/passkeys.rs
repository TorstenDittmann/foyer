//! Passkeys: add them to an account, and sign in with them.
//!
//! Each ceremony has two steps. `…/start` returns WebAuthn options for the
//! browser and keeps the challenge state on the server under a ceremony id;
//! `…/finish` takes the browser's answer with that id. A ceremony works once
//! and for five minutes.
//!
//! Sign-in uses discoverable credentials: the browser offers the passkeys it
//! has for the site, so nobody types a username. Registration therefore asks
//! for a discoverable (resident) credential.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Duration;
use foyer_types::{
    ApiError, AuthResponse, FinishPasskeyLogin, FinishPasskeyRegistration, PasskeyChallenge,
    PasskeyInfo,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;
use webauthn_rs::prelude::{
    DiscoverableAuthentication, Passkey, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential,
};

use crate::AppState;
use crate::auth::{self, CurrentUser, TokenKind};
use crate::error::{AppError, AppResult, JsonBody, PathParam};
use crate::model::{self, fmt_ts};
use crate::ratelimit::Bucket;
use crate::validate;

/// How long the browser has to answer.
const CEREMONY_TTL: Duration = Duration::minutes(5);
/// The most passkeys one account may have.
const MAX_PASSKEYS: i64 = 20;

pub fn routes() -> OpenApiRouter<AppState> {
    let limited = OpenApiRouter::new()
        .routes(routes!(login_start))
        .routes(routes!(login_finish))
        .routes(routes!(register_start))
        .routes(routes!(register_finish));
    super::limited(limited, Bucket::Auth)
        .routes(routes!(list_passkeys))
        .routes(routes!(delete_passkey))
}

fn passkey_failed() -> AppError {
    AppError::coded(
        StatusCode::BAD_REQUEST,
        "passkey_failed",
        "that passkey didn't work; try again or sign in another way",
    )
}

fn credential_key(id: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(id)
}

#[derive(Clone, Copy)]
enum Ceremony {
    Register,
    Login,
}

impl Ceremony {
    fn as_str(self) -> &'static str {
        match self {
            Ceremony::Register => "register",
            Ceremony::Login => "login",
        }
    }
}

async fn save_ceremony(
    conn: &libsql::Connection,
    kind: Ceremony,
    user: Option<Uuid>,
    state: &impl serde::Serialize,
) -> AppResult<Uuid> {
    let now = model::now();
    conn.execute(
        "DELETE FROM passkey_ceremonies WHERE expires_at <= ?1",
        [fmt_ts(now)],
    )
    .await?;
    let id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO passkey_ceremonies (id, kind, user_id, state, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        libsql::params![
            id.to_string(),
            kind.as_str(),
            user.map(|u| u.to_string()),
            serde_json::to_string(state)?,
            fmt_ts(now + CEREMONY_TTL),
        ],
    )
    .await?;
    Ok(id)
}

/// Uses up a ceremony. `None` when it is unknown, expired, used, of another
/// kind, or (for registrations) started by another user.
async fn take_ceremony<T: serde::de::DeserializeOwned>(
    conn: &libsql::Connection,
    id: Uuid,
    kind: Ceremony,
    user: Option<Uuid>,
) -> AppResult<Option<T>> {
    let mut rows = conn
        .query(
            "DELETE FROM passkey_ceremonies
             WHERE id = ?1 AND kind = ?2 AND expires_at > ?3 AND user_id IS ?4
             RETURNING state",
            libsql::params![
                id.to_string(),
                kind.as_str(),
                fmt_ts(model::now()),
                user.map(|u| u.to_string()),
            ],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some(serde_json::from_str(&row.get::<String>(0)?)?)),
        None => Ok(None),
    }
}

async fn user_passkeys(
    conn: &libsql::Connection,
    user: Uuid,
) -> AppResult<Vec<(PasskeyInfo, Passkey)>> {
    let mut rows = conn
        .query(
            "SELECT id, name, created_at, last_used_at, passkey FROM passkeys
             WHERE user_id = ?1 ORDER BY created_at",
            [user.to_string()],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push((
            PasskeyInfo {
                id: model::uuid(&row, 0)?,
                name: row.get(1)?,
                created_at: model::ts(&row, 2)?,
                last_used_at: model::opt_ts(&row, 3)?,
            },
            serde_json::from_str(&row.get::<String>(4)?)?,
        ));
    }
    Ok(out)
}

/// Start adding a passkey to the caller's account.
#[utoipa::path(
    post, path = "/v1/auth/passkeys/register/start", tag = "auth",
    security(("bearer" = [])),
    responses(
        (status = 200, body = PasskeyChallenge),
        (status = 401, body = ApiError),
        (status = 409, description = "`too_many_passkeys`", body = ApiError),
        (status = 429, body = ApiError),
        (status = 503, description = "`passkeys_unavailable`", body = ApiError),
    ),
)]
async fn register_start(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<PasskeyChallenge>> {
    let webauthn = state.webauthn()?;
    let conn = state.conn().await?;
    let existing = user_passkeys(&conn, user.id()).await?;
    if existing.len() as i64 >= MAX_PASSKEYS {
        return Err(AppError::conflict(
            "too_many_passkeys",
            format!("an account can have at most {MAX_PASSKEYS} passkeys; remove one first"),
        ));
    }
    // So the browser won't register the same authenticator twice.
    let exclude = existing
        .iter()
        .map(|(_, key)| key.cred_id().clone())
        .collect();
    let name = &user.user.username;
    let (mut options, registration) = webauthn
        .start_passkey_registration(user.id(), name, name, Some(exclude))
        .map_err(|e| anyhow::anyhow!("starting passkey registration: {e}"))?;
    // Sign-in is usernameless, which needs a discoverable credential.
    if let Some(selection) = options.public_key.authenticator_selection.as_mut() {
        selection.resident_key = Some(webauthn_rs_proto::ResidentKeyRequirement::Required);
        selection.require_resident_key = true;
    }
    let ceremony = save_ceremony(&conn, Ceremony::Register, Some(user.id()), &registration).await?;
    Ok(Json(PasskeyChallenge {
        ceremony,
        options: serde_json::to_value(options)?,
    }))
}

/// Finish adding a passkey with the browser's answer.
#[utoipa::path(
    post, path = "/v1/auth/passkeys/register/finish", tag = "auth",
    security(("bearer" = [])),
    request_body = FinishPasskeyRegistration,
    responses(
        (status = 201, body = PasskeyInfo),
        (status = 400, description = "`passkey_failed`, or a bad name", body = ApiError),
        (status = 401, body = ApiError),
        (status = 409, description = "`passkey_exists`", body = ApiError),
        (status = 429, body = ApiError),
        (status = 503, description = "`passkeys_unavailable`", body = ApiError),
    ),
)]
async fn register_finish(
    State(state): State<AppState>,
    user: CurrentUser,
    JsonBody(req): JsonBody<FinishPasskeyRegistration>,
) -> AppResult<(StatusCode, Json<PasskeyInfo>)> {
    let webauthn = state.webauthn()?;
    let name = match req.name.trim() {
        "" => "Passkey",
        name => name,
    };
    validate::required_text("name", name, 60)?;
    let conn = state.conn().await?;
    let registration: PasskeyRegistration =
        take_ceremony(&conn, req.ceremony, Ceremony::Register, Some(user.id()))
            .await?
            .ok_or_else(passkey_failed)?;
    let credential: RegisterPublicKeyCredential =
        serde_json::from_value(req.credential).map_err(|_| passkey_failed())?;
    let passkey = webauthn
        .finish_passkey_registration(&credential, &registration)
        .map_err(|e| {
            tracing::info!(error = %e, "passkey registration rejected");
            passkey_failed()
        })?;

    let info = PasskeyInfo {
        id: Uuid::now_v7(),
        name: name.to_string(),
        created_at: model::now(),
        last_used_at: None,
    };
    let inserted = conn
        .execute(
            "INSERT INTO passkeys (id, user_id, credential_id, name, passkey, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT DO NOTHING",
            libsql::params![
                info.id.to_string(),
                user.id().to_string(),
                credential_key(passkey.cred_id()),
                info.name.clone(),
                serde_json::to_string(&passkey)?,
                fmt_ts(info.created_at),
            ],
        )
        .await?;
    if inserted == 0 {
        return Err(AppError::conflict(
            "passkey_exists",
            "this passkey is already registered",
        ));
    }
    Ok((StatusCode::CREATED, Json(info)))
}

/// The caller's passkeys.
#[utoipa::path(
    get, path = "/v1/auth/passkeys", tag = "auth",
    security(("bearer" = [])),
    responses((status = 200, body = Vec<PasskeyInfo>), (status = 401, body = ApiError)),
)]
async fn list_passkeys(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Vec<PasskeyInfo>>> {
    let conn = state.conn().await?;
    let keys = user_passkeys(&conn, user.id()).await?;
    Ok(Json(keys.into_iter().map(|(info, _)| info).collect()))
}

/// Remove one of the caller's passkeys. It stops working for sign-in right
/// away; the copy on the device stays until the user deletes it there.
#[utoipa::path(
    delete, path = "/v1/auth/passkeys/{id}", tag = "auth",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses((status = 204), (status = 401, body = ApiError), (status = 404, body = ApiError)),
)]
async fn delete_passkey(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
) -> AppResult<StatusCode> {
    let deleted = state
        .conn()
        .await?
        .execute(
            "DELETE FROM passkeys WHERE id = ?1 AND user_id = ?2",
            [id.to_string(), user.id().to_string()],
        )
        .await?;
    if deleted == 0 {
        return Err(AppError::NotFound("passkey"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Start a passkey sign-in. No username: the browser offers the passkeys it
/// has for this site.
#[utoipa::path(
    post, path = "/v1/auth/passkeys/login/start", tag = "auth",
    responses(
        (status = 200, body = PasskeyChallenge),
        (status = 429, body = ApiError),
        (status = 503, description = "`passkeys_unavailable`", body = ApiError),
    ),
)]
async fn login_start(State(state): State<AppState>) -> AppResult<Json<PasskeyChallenge>> {
    let webauthn = state.webauthn()?;
    let (mut options, authentication) = webauthn
        .start_discoverable_authentication()
        .map_err(|e| anyhow::anyhow!("starting passkey sign-in: {e}"))?;
    // webauthn-rs asks for conditional mediation (autofill). The website
    // starts sign-in from a button instead, which needs the default modal
    // prompt; the browser picks the mediation, so drop the hint.
    options.mediation = None;
    let conn = state.conn().await?;
    let ceremony = save_ceremony(&conn, Ceremony::Login, None, &authentication).await?;
    Ok(Json(PasskeyChallenge {
        ceremony,
        options: serde_json::to_value(options)?,
    }))
}

/// Finish a passkey sign-in and start a session.
#[utoipa::path(
    post, path = "/v1/auth/passkeys/login/finish", tag = "auth",
    request_body = FinishPasskeyLogin,
    responses(
        (status = 200, body = AuthResponse),
        (status = 400, description = "`passkey_failed`", body = ApiError),
        (status = 429, body = ApiError),
        (status = 503, description = "`passkeys_unavailable`", body = ApiError),
    ),
)]
async fn login_finish(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<FinishPasskeyLogin>,
) -> AppResult<Json<AuthResponse>> {
    let webauthn = state.webauthn()?;
    let conn = state.conn().await?;
    let authentication: DiscoverableAuthentication =
        take_ceremony(&conn, req.ceremony, Ceremony::Login, None)
            .await?
            .ok_or_else(passkey_failed)?;
    let credential: PublicKeyCredential =
        serde_json::from_value(req.credential).map_err(|_| passkey_failed())?;
    let (user_id, credential_id) = webauthn
        .identify_discoverable_authentication(&credential)
        .map_err(|_| passkey_failed())?;

    let mut rows = conn
        .query(
            "SELECT id, passkey FROM passkeys WHERE credential_id = ?1 AND user_id = ?2",
            [credential_key(credential_id), user_id.to_string()],
        )
        .await?;
    let row = rows.next().await?.ok_or_else(passkey_failed)?;
    let passkey_id = model::uuid(&row, 0)?;
    let mut passkey: Passkey = serde_json::from_str(&row.get::<String>(1)?)?;
    drop(rows);

    let result = webauthn
        .finish_discoverable_authentication_v2(&credential, authentication, &passkey)
        .map_err(|e| {
            tracing::info!(error = %e, "passkey sign-in rejected");
            passkey_failed()
        })?;
    // Keeps the signature counter current, which catches cloned keys.
    let stored = if passkey.update_credential(&result) == Some(true) {
        Some(serde_json::to_string(&passkey)?)
    } else {
        None
    };
    conn.execute(
        "UPDATE passkeys SET last_used_at = ?1, passkey = coalesce(?2, passkey) WHERE id = ?3",
        libsql::params![fmt_ts(model::now()), stored, passkey_id.to_string()],
    )
    .await?;

    let user = model::user_by_id(&conn, user_id).await?;
    let token = auth::issue_token(&conn, user.id, TokenKind::Session, "").await?;
    Ok(Json(AuthResponse {
        token: token.secret,
        expires_at: token.expires_at.expect("sessions expire"),
        user,
    }))
}
