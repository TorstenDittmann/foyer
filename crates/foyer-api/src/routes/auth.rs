//! Accounts: register, log in and out, API tokens, roles.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use foyer_types::{
    ApiError, ApiToken, AuthResponse, CreateApiTokenRequest, CreatedApiToken, LoginRequest,
    RegisterRequest, Role, SetRoleRequest, User,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{self, CurrentUser, TokenKind};
use crate::error::{AppError, AppResult, JsonBody, PathParam};
use crate::model::{self, USER_COLUMNS, fmt_ts};
use crate::ratelimit::Bucket;
use crate::validate;

pub fn routes() -> OpenApiRouter<AppState> {
    let limited = OpenApiRouter::new()
        .routes(routes!(register))
        .routes(routes!(login));
    super::limited(limited, Bucket::Auth)
        .routes(routes!(logout))
        .routes(routes!(me))
        .routes(routes!(list_tokens, create_token))
        .routes(routes!(delete_token))
        .routes(routes!(set_role))
}

/// Create an account and start a session. A link to confirm the email
/// address is sent; until then the account can't submit games.
#[utoipa::path(
    post, path = "/v1/auth/register", tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 201, body = AuthResponse),
        (status = 400, body = ApiError),
        (status = 409, description = "`username_taken` or `email_taken`", body = ApiError),
        (status = 429, description = "`rate_limited`, with `Retry-After`", body = ApiError),
    ),
)]
async fn register(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<RegisterRequest>,
) -> AppResult<(StatusCode, Json<AuthResponse>)> {
    let username = req.username.trim().to_lowercase();
    let email = req.email.trim().to_lowercase();
    validate::username(&username)?;
    validate::email(&email)?;
    validate::password(&req.password)?;

    let conn = state.conn().await?;
    let mut rows = conn
        .query(
            "SELECT username = ?1, email = ?2 FROM users WHERE username = ?1 OR email = ?2",
            [username.as_str(), email.as_str()],
        )
        .await?;
    if let Some(row) = rows.next().await? {
        return Err(if row.get::<bool>(0)? {
            AppError::conflict("username_taken", "that username is taken")
        } else {
            AppError::conflict("email_taken", "an account with that email exists")
        });
    }

    let password = req.password;
    let password_hash =
        tokio::task::spawn_blocking(move || auth::hash_password(&password)).await??;
    // Admin emails are only honoured once confirmed; see
    // `auth::mark_email_verified`.
    let user = User {
        id: Uuid::now_v7(),
        username,
        email,
        role: Role::User,
        email_verified: false,
        created_at: model::now(),
    };
    let inserted = conn
        .execute(
            "INSERT INTO users (id, username, email, password_hash, role, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT DO NOTHING",
            libsql::params![
                user.id.to_string(),
                user.username.clone(),
                user.email.clone(),
                password_hash,
                user.role.as_str(),
                fmt_ts(user.created_at),
            ],
        )
        .await?;
    if inserted == 0 {
        // Lost a race with a concurrent registration.
        return Err(AppError::conflict(
            "username_taken",
            "that username or email is taken",
        ));
    }

    super::account::send_verification(&state, &conn, &user).await?;
    let token = auth::issue_token(&conn, user.id, TokenKind::Session, "").await?;
    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            token: token.secret,
            expires_at: token.expires_at.expect("sessions expire"),
            user,
        }),
    ))
}

/// Log in with email or username and password.
#[utoipa::path(
    post, path = "/v1/auth/login", tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, body = AuthResponse),
        (status = 401, description = "Wrong login or password", body = ApiError),
        (status = 429, description = "`rate_limited`, with `Retry-After`", body = ApiError),
    ),
)]
async fn login(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<LoginRequest>,
) -> AppResult<Json<AuthResponse>> {
    let login = req.login.trim().to_lowercase();
    let conn = state.conn().await?;
    let mut rows = conn
        .query(
            &format!(
                "SELECT {USER_COLUMNS}, u.password_hash FROM users u
                 WHERE u.email = ?1 OR u.username = ?1"
            ),
            [login.as_str()],
        )
        .await?;
    let found = match rows.next().await? {
        Some(row) => Some((
            model::user_from_row(&row, 0)?,
            row.get::<String>(model::USER_COLUMN_COUNT)?,
        )),
        None => None,
    };

    let hash = found
        .as_ref()
        .map_or_else(|| auth::dummy_hash().to_string(), |(_, h)| h.clone());
    let password = req.password;
    let valid =
        tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash)).await?;
    let user = match found {
        Some((user, _)) if valid => user,
        _ => return Err(AppError::Unauthorized),
    };

    let token = auth::issue_token(&conn, user.id, TokenKind::Session, "").await?;
    Ok(Json(AuthResponse {
        token: token.secret,
        expires_at: token.expires_at.expect("sessions expire"),
        user,
    }))
}

/// Revoke the token used for this request.
#[utoipa::path(
    post, path = "/v1/auth/logout", tag = "auth",
    security(("bearer" = [])),
    responses((status = 204), (status = 401, body = ApiError)),
)]
async fn logout(State(state): State<AppState>, user: CurrentUser) -> AppResult<StatusCode> {
    state
        .conn()
        .await?
        .execute(
            "DELETE FROM tokens WHERE id = ?1",
            [user.token_id.to_string()],
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The signed-in user.
#[utoipa::path(
    get, path = "/v1/me", tag = "auth",
    security(("bearer" = [])),
    responses((status = 200, body = User), (status = 401, body = ApiError)),
)]
async fn me(user: CurrentUser) -> Json<User> {
    Json(user.user)
}

/// List the caller's API tokens.
#[utoipa::path(
    get, path = "/v1/auth/tokens", tag = "auth",
    security(("bearer" = [])),
    responses((status = 200, body = Vec<ApiToken>), (status = 401, body = ApiError)),
)]
async fn list_tokens(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Vec<ApiToken>>> {
    let conn = state.conn().await?;
    let mut rows = conn
        .query(
            "SELECT id, name, created_at, last_used_at FROM tokens
             WHERE user_id = ?1 AND kind = 'api' ORDER BY created_at",
            [user.id().to_string()],
        )
        .await?;
    let mut tokens = Vec::new();
    while let Some(row) = rows.next().await? {
        tokens.push(ApiToken {
            id: model::uuid(&row, 0)?,
            name: row.get(1)?,
            created_at: model::ts(&row, 2)?,
            last_used_at: model::opt_ts(&row, 3)?,
        });
    }
    Ok(Json(tokens))
}

/// Create an API token for the CLI or CI. The secret is returned only once.
#[utoipa::path(
    post, path = "/v1/auth/tokens", tag = "auth",
    security(("bearer" = [])),
    request_body = CreateApiTokenRequest,
    responses(
        (status = 201, body = CreatedApiToken),
        (status = 400, body = ApiError),
        (status = 401, body = ApiError),
    ),
)]
async fn create_token(
    State(state): State<AppState>,
    user: CurrentUser,
    JsonBody(req): JsonBody<CreateApiTokenRequest>,
) -> AppResult<(StatusCode, Json<CreatedApiToken>)> {
    let name = req.name.trim();
    validate::required_text("name", name, 60)?;
    let conn = state.conn().await?;
    let token = auth::issue_token(&conn, user.id(), TokenKind::Api, name).await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedApiToken {
            token: token.secret,
            info: ApiToken {
                id: token.id,
                name: name.to_string(),
                created_at: token.created_at,
                last_used_at: None,
            },
        }),
    ))
}

/// Revoke one of the caller's API tokens.
#[utoipa::path(
    delete, path = "/v1/auth/tokens/{id}", tag = "auth",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses((status = 204), (status = 404, body = ApiError)),
)]
async fn delete_token(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
) -> AppResult<StatusCode> {
    let deleted = state
        .conn()
        .await?
        .execute(
            "DELETE FROM tokens WHERE id = ?1 AND user_id = ?2 AND kind = 'api'",
            [id.to_string(), user.id().to_string()],
        )
        .await?;
    if deleted == 0 {
        return Err(AppError::NotFound("token"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Change a user's role. Admins only.
#[utoipa::path(
    put, path = "/v1/admin/users/{id}/role", tag = "admin",
    security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = SetRoleRequest,
    responses(
        (status = 200, body = User),
        (status = 403, body = ApiError),
        (status = 404, body = ApiError),
    ),
)]
async fn set_role(
    State(state): State<AppState>,
    user: CurrentUser,
    PathParam(id): PathParam<Uuid>,
    JsonBody(req): JsonBody<SetRoleRequest>,
) -> AppResult<Json<User>> {
    user.require_admin()?;
    if id == user.id() && req.role != Role::Admin {
        return Err(AppError::validation("admins can't demote themselves"));
    }
    let conn = state.conn().await?;
    let updated = conn
        .execute(
            "UPDATE users SET role = ?1 WHERE id = ?2",
            [req.role.as_str(), &id.to_string()],
        )
        .await?;
    if updated == 0 {
        return Err(AppError::NotFound("user"));
    }
    Ok(Json(model::user_by_id(&conn, id).await?))
}
