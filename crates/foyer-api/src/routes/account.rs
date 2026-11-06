//! Email verification, password resets and Discord sign-in.
//!
//! Discord flow, driven by the website:
//! 1. `GET /v1/auth/discord/authorize` returns Discord's consent URL and a
//!    `state`. The website keeps the state in a cookie and redirects.
//! 2. Discord sends the browser to `{FOYER_PUBLIC_WEB_URL}/auth/discord/callback`
//!    with `code` and `state`. The website checks the state against its
//!    cookie and calls `POST /v1/auth/discord/callback`.
//! 3. The API checks the state is one it issued, exchanges the code, and
//!    signs the user in: by Discord id, else by linking the account with the
//!    same (Discord-verified) email, else by creating an account.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Duration;
use foyer_types::{
    ApiError, AuthResponse, DiscordAuthorize, DiscordCallback, PasswordReset, PasswordResetRequest,
    Role, User, VerifyEmailRequest,
};
use serde::Deserialize;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{self, CurrentUser, EmailPurpose, TokenKind};
use crate::error::{AppError, AppResult, JsonBody};
use crate::mail;
use crate::model::{self, USER_COLUMNS, fmt_ts};
use crate::ratelimit::Bucket;
use crate::validate;

/// How long a Discord sign-in may take from start to callback.
const OAUTH_STATE_TTL: Duration = Duration::minutes(15);

pub fn routes() -> OpenApiRouter<AppState> {
    let limited = OpenApiRouter::new()
        .routes(routes!(resend_verification))
        .routes(routes!(verify_email))
        .routes(routes!(request_password_reset))
        .routes(routes!(reset_password))
        .routes(routes!(discord_authorize))
        .routes(routes!(discord_callback));
    super::limited(limited, Bucket::Auth)
}

/// Sends `user` a link to confirm their address.
pub(crate) async fn send_verification(
    state: &AppState,
    conn: &libsql::Connection,
    user: &User,
) -> AppResult<()> {
    let token =
        auth::issue_email_token(conn, user.id, EmailPurpose::VerifyEmail, &user.email).await?;
    let link = state.web_link("/verify-email", &[("token", &token)]);
    mail::queue(
        conn,
        &mail::verification(&user.email, &user.username, &link),
    )
    .await?;
    Ok(())
}

fn invalid_token() -> AppError {
    AppError::coded(
        StatusCode::BAD_REQUEST,
        "invalid_token",
        "this link is invalid or has expired; ask for a new one",
    )
}

/// Send the confirmation link again.
#[utoipa::path(
    post, path = "/v1/auth/verify-email/resend", tag = "auth",
    security(("bearer" = [])),
    responses(
        (status = 204),
        (status = 401, body = ApiError),
        (status = 409, description = "`already_verified`", body = ApiError),
        (status = 429, body = ApiError),
    ),
)]
async fn resend_verification(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<StatusCode> {
    if user.user.email_verified {
        return Err(AppError::conflict(
            "already_verified",
            "your email address is already confirmed",
        ));
    }
    let conn = state.conn().await?;
    send_verification(&state, &conn, &user.user).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Confirm an email address with the token from the link.
#[utoipa::path(
    post, path = "/v1/auth/verify-email", tag = "auth",
    request_body = VerifyEmailRequest,
    responses(
        (status = 200, body = User),
        (status = 400, description = "`invalid_token`", body = ApiError),
        (status = 429, body = ApiError),
    ),
)]
async fn verify_email(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<VerifyEmailRequest>,
) -> AppResult<Json<User>> {
    let conn = state.conn().await?;
    let (user_id, email) = auth::take_email_token(&conn, &req.token, EmailPurpose::VerifyEmail)
        .await?
        .ok_or_else(invalid_token)?;
    // The user may have changed their address since; then the link is void.
    if !auth::mark_email_verified(&state, &conn, user_id, &email).await? {
        return Err(invalid_token());
    }
    Ok(Json(model::user_by_id(&conn, user_id).await?))
}

/// Email a password reset link. Answers 204 whether or not the address has
/// an account, so it can't be used to find out who is registered.
#[utoipa::path(
    post, path = "/v1/auth/password-reset/request", tag = "auth",
    request_body = PasswordResetRequest,
    responses((status = 204), (status = 429, body = ApiError)),
)]
async fn request_password_reset(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<PasswordResetRequest>,
) -> AppResult<StatusCode> {
    let email = req.email.trim().to_lowercase();
    let conn = state.conn().await?;
    let mut rows = conn
        .query(
            &format!("SELECT {USER_COLUMNS} FROM users u WHERE u.email = ?1"),
            [email.as_str()],
        )
        .await?;
    let user = match rows.next().await? {
        Some(row) => Some(model::user_from_row(&row, 0)?),
        None => None,
    };
    drop(rows);
    if let Some(user) = user {
        let token =
            auth::issue_email_token(&conn, user.id, EmailPurpose::ResetPassword, &user.email)
                .await?;
        let link = state.web_link("/reset-password", &[("token", &token)]);
        mail::queue(
            &conn,
            &mail::password_reset(&user.email, &user.username, &link),
        )
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Set a new password with the token from the reset link. Every session is
/// signed out; API tokens keep working. Also confirms the email address,
/// since the link proves the user can read it.
#[utoipa::path(
    post, path = "/v1/auth/password-reset", tag = "auth",
    request_body = PasswordReset,
    responses(
        (status = 204),
        (status = 400, description = "`invalid_token` or a weak password", body = ApiError),
        (status = 429, body = ApiError),
    ),
)]
async fn reset_password(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<PasswordReset>,
) -> AppResult<StatusCode> {
    validate::password(&req.password)?;
    let conn = state.conn().await?;
    let (user_id, email) = auth::take_email_token(&conn, &req.token, EmailPurpose::ResetPassword)
        .await?
        .ok_or_else(invalid_token)?;
    let password = req.password;
    let hash = tokio::task::spawn_blocking(move || auth::hash_password(&password)).await??;
    let updated = conn
        .execute(
            "UPDATE users SET password_hash = ?1 WHERE id = ?2 AND email = ?3",
            [hash, user_id.to_string(), email.clone()],
        )
        .await?;
    if updated == 0 {
        return Err(invalid_token());
    }
    conn.execute(
        "DELETE FROM tokens WHERE user_id = ?1 AND kind = 'session'",
        [user_id.to_string()],
    )
    .await?;
    auth::mark_email_verified(&state, &conn, user_id, &email).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn discord(state: &AppState) -> AppResult<&crate::DiscordConfig> {
    state.discord.as_deref().ok_or_else(|| {
        AppError::coded(
            StatusCode::SERVICE_UNAVAILABLE,
            "discord_unavailable",
            "Discord sign-in is not set up",
        )
    })
}

fn discord_redirect_uri(state: &AppState) -> String {
    state.web_link("/auth/discord/callback", &[])
}

/// Start a Discord sign-in: where to send the browser.
#[utoipa::path(
    get, path = "/v1/auth/discord/authorize", tag = "auth",
    responses(
        (status = 200, body = DiscordAuthorize),
        (status = 429, body = ApiError),
        (status = 503, description = "`discord_unavailable`", body = ApiError),
    ),
)]
async fn discord_authorize(State(state): State<AppState>) -> AppResult<Json<DiscordAuthorize>> {
    let discord = discord(&state)?;
    let oauth_state = auth::random_secret("")?;
    let now = model::now();
    let conn = state.conn().await?;
    conn.execute(
        "DELETE FROM oauth_states WHERE expires_at < ?1",
        [fmt_ts(now)],
    )
    .await?;
    conn.execute(
        "INSERT INTO oauth_states (state_hash, created_at, expires_at) VALUES (?1, ?2, ?3)",
        [
            auth::secret_hash(&oauth_state),
            fmt_ts(now),
            fmt_ts(now + OAUTH_STATE_TTL),
        ],
    )
    .await?;
    let mut url = discord.authorize_url.clone();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &discord.client_id)
        .append_pair("scope", "identify email")
        .append_pair("redirect_uri", &discord_redirect_uri(&state))
        .append_pair("state", &oauth_state);
    Ok(Json(DiscordAuthorize {
        url: url.to_string(),
        state: oauth_state,
    }))
}

#[derive(Deserialize)]
struct DiscordToken {
    access_token: String,
}

#[derive(Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    email: Option<String>,
    #[serde(default)]
    verified: bool,
}

fn discord_failed() -> AppError {
    AppError::coded(
        StatusCode::BAD_REQUEST,
        "discord_failed",
        "Discord sign-in didn't work; please try again",
    )
}

/// Finish a Discord sign-in and start a session.
#[utoipa::path(
    post, path = "/v1/auth/discord/callback", tag = "auth",
    request_body = DiscordCallback,
    responses(
        (status = 200, body = AuthResponse),
        (status = 400, description = "`discord_failed`, or `discord_email_unverified` when \
            the Discord account has no verified email", body = ApiError),
        (status = 409, description = "`discord_email_taken`: the email's account is linked \
            to another Discord account", body = ApiError),
        (status = 429, body = ApiError),
        (status = 503, description = "`discord_unavailable`", body = ApiError),
    ),
)]
async fn discord_callback(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<DiscordCallback>,
) -> AppResult<Json<AuthResponse>> {
    let discord = discord(&state)?;
    let conn = state.conn().await?;
    let known = conn
        .execute(
            "DELETE FROM oauth_states WHERE state_hash = ?1 AND expires_at > ?2",
            [auth::secret_hash(&req.state), fmt_ts(model::now())],
        )
        .await?;
    if known == 0 {
        return Err(discord_failed());
    }

    let api = discord.api_url.as_str().trim_end_matches('/');
    let response = state
        .http
        .post(format!("{api}/oauth2/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", req.code.as_str()),
            ("redirect_uri", discord_redirect_uri(&state).as_str()),
            ("client_id", discord.client_id.as_str()),
            ("client_secret", discord.client_secret.as_str()),
        ])
        .send()
        .await?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Discord token exchange failed");
        return Err(discord_failed());
    }
    let token: DiscordToken = response.json().await.map_err(|_| discord_failed())?;
    let response = state
        .http
        .get(format!("{api}/users/@me"))
        .bearer_auth(&token.access_token)
        .send()
        .await?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Discord user lookup failed");
        return Err(discord_failed());
    }
    let profile: DiscordUser = response.json().await.map_err(|_| discord_failed())?;
    let email = match profile.email.as_deref() {
        Some(email) if profile.verified => email.trim().to_lowercase(),
        _ => {
            return Err(AppError::coded(
                StatusCode::BAD_REQUEST,
                "discord_email_unverified",
                "your Discord account needs a verified email address",
            ));
        }
    };

    let user_id = discord_account(&state, &conn, &profile, &email).await?;
    let user = model::user_by_id(&conn, user_id).await?;
    let token = auth::issue_token(&conn, user.id, TokenKind::Session, "").await?;
    Ok(Json(AuthResponse {
        token: token.secret,
        expires_at: token.expires_at.expect("sessions expire"),
        user,
    }))
}

/// The account for a Discord user: found by Discord id, linked by email, or
/// created.
async fn discord_account(
    state: &AppState,
    conn: &libsql::Connection,
    profile: &DiscordUser,
    email: &str,
) -> AppResult<Uuid> {
    let mut rows = conn
        .query(
            "SELECT id FROM users WHERE discord_id = ?1",
            [profile.id.as_str()],
        )
        .await?;
    if let Some(row) = rows.next().await? {
        return Ok(model::uuid(&row, 0)?);
    }
    drop(rows);

    let mut rows = conn
        .query(
            "SELECT id, discord_id, email_verified_at FROM users WHERE email = ?1",
            [email],
        )
        .await?;
    if let Some(row) = rows.next().await? {
        let id = model::uuid(&row, 0)?;
        let linked: Option<String> = row.get(1)?;
        let verified: Option<String> = row.get(2)?;
        drop(rows);
        if linked.is_some() {
            return Err(AppError::conflict(
                "discord_email_taken",
                "an account with this email is linked to another Discord account",
            ));
        }
        let tx = conn.transaction().await?;
        if verified.is_none() {
            // Whoever registered this address never proved they own it; the
            // Discord user has. Drop their password and sessions so a
            // squatter can't keep access.
            tx.execute(
                "UPDATE users SET password_hash = '' WHERE id = ?1",
                [id.to_string()],
            )
            .await?;
            tx.execute("DELETE FROM tokens WHERE user_id = ?1", [id.to_string()])
                .await?;
        }
        tx.execute(
            "UPDATE users SET discord_id = ?1 WHERE id = ?2",
            [profile.id.clone(), id.to_string()],
        )
        .await?;
        auth::mark_email_verified(state, &tx, id, email).await?;
        tx.commit().await?;
        return Ok(id);
    }
    drop(rows);

    let id = Uuid::now_v7();
    let now = fmt_ts(model::now());
    for username in username_candidates(&profile.username) {
        let inserted = conn
            .execute(
                "INSERT INTO users (id, username, email, password_hash, role, created_at,
                   discord_id)
                 VALUES (?1, ?2, ?3, '', ?4, ?5, ?6) ON CONFLICT DO NOTHING",
                [
                    id.to_string(),
                    username,
                    email.to_string(),
                    Role::User.as_str().to_string(),
                    now.clone(),
                    profile.id.clone(),
                ],
            )
            .await?;
        if inserted == 1 {
            auth::mark_email_verified(state, conn, id, email).await?;
            return Ok(id);
        }
    }
    // Every candidate taken, or a concurrent sign-in created the account.
    Err(discord_failed())
}

/// Usernames to try for a new Discord account: the Discord name made valid,
/// then with a number, then with random digits.
fn username_candidates(discord: &str) -> Vec<String> {
    let mut base: String = discord
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            'a'..='z' | '0'..='9' | '-' | '_' => Some(c),
            '.' | ' ' => Some('_'),
            _ => None,
        })
        .take(26)
        .collect();
    if base.len() < 3 {
        base = format!("player{base}");
    }
    let mut out = vec![base.clone()];
    out.extend((2..10).map(|n| format!("{base}-{n}")));
    let mut bytes = [0u8; 2];
    if getrandom::fill(&mut bytes).is_ok() {
        out.push(format!("{base}-{}", u16::from_le_bytes(bytes)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::username_candidates;

    #[test]
    fn discord_names_become_valid_usernames() {
        assert_eq!(username_candidates("Ada.Lovelace")[0], "ada_lovelace");
        assert_eq!(username_candidates("✨")[0], "player");
        assert_eq!(username_candidates("ab")[1], "playerab-2");
        for name in username_candidates("Some Very Long Discord Name That Goes On") {
            crate::validate::username(&name).unwrap();
        }
    }
}
