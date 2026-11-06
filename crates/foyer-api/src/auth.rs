//! Passwords, tokens and the extractors that authenticate a request.
//!
//! Clients send `Authorization: Bearer <token>`. Tokens are random 256-bit
//! secrets; the database stores only their SHA-256, so a leaked database does
//! not leak working tokens. Session tokens (`fys_…`) come from login and
//! expire; API tokens (`fyp_…`) are created for the CLI and CI and last until
//! revoked.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use foyer_types::{Role, User};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::AppState;
use crate::error::{AppError, AppResult};
use crate::model::{self, fmt_ts};

/// How long a login session lasts.
pub const SESSION_TTL: Duration = Duration::days(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Session,
    Api,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TokenKind::Session => "session",
            TokenKind::Api => "api",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            TokenKind::Session => "fys_",
            TokenKind::Api => "fyp_",
        }
    }
}

/// Hashes a password with Argon2id. CPU-heavy: call from `spawn_blocking`.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| anyhow::anyhow!("salt: {e}"))?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2: {e}"))?;
    Ok(hash.to_string())
}

/// Checks a password against a stored hash. CPU-heavy: call from
/// `spawn_blocking`.
pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

/// A valid Argon2 hash of a random password, verified against when a login
/// names an unknown user so that response times don't reveal which accounts
/// exist.
pub fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password(&Uuid::new_v4().to_string()).expect("hashing works"))
}

fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A newly issued token. `secret` is shown to the client once.
pub struct IssuedToken {
    pub id: Uuid,
    pub secret: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Creates a token for `user_id` and stores its hash.
pub async fn issue_token(
    conn: &libsql::Connection,
    user_id: Uuid,
    kind: TokenKind,
    name: &str,
) -> AppResult<IssuedToken> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    let secret = format!("{}{}", kind.prefix(), URL_SAFE_NO_PAD.encode(bytes));
    let id = Uuid::now_v7();
    let created_at = model::now();
    let expires_at = match kind {
        TokenKind::Session => Some(created_at + SESSION_TTL),
        TokenKind::Api => None,
    };
    conn.execute(
        "INSERT INTO tokens (id, user_id, kind, name, token_hash, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        libsql::params![
            id.to_string(),
            user_id.to_string(),
            kind.as_str(),
            name,
            token_hash(&secret),
            fmt_ts(created_at),
            expires_at.map(fmt_ts),
        ],
    )
    .await?;
    Ok(IssuedToken {
        id,
        secret,
        created_at,
        expires_at,
    })
}

/// The authenticated caller. Extracting it fails with 401 when the request
/// has no valid token.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user: User,
    /// The token used for this request, so logout can revoke it.
    pub token_id: Uuid,
}

impl CurrentUser {
    pub fn id(&self) -> Uuid {
        self.user.id
    }

    pub fn is_staff(&self) -> bool {
        self.user.role.is_staff()
    }

    pub fn require_staff(&self) -> AppResult<()> {
        if self.is_staff() {
            Ok(())
        } else {
            Err(AppError::forbidden("only reviewers can do this"))
        }
    }

    pub fn require_admin(&self) -> AppResult<()> {
        if self.user.role == Role::Admin {
            Ok(())
        } else {
            Err(AppError::forbidden("only admins can do this"))
        }
    }
}

/// Like [`CurrentUser`], but anonymous requests are fine. An invalid token is
/// still a 401, so clients notice expired sessions.
#[derive(Debug, Clone)]
pub struct MaybeUser(pub Option<CurrentUser>);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> AppResult<Self> {
        MaybeUser::from_request_parts(parts, state)
            .await?
            .0
            .ok_or(AppError::Unauthorized)
    }
}

impl FromRequestParts<AppState> for MaybeUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> AppResult<Self> {
        let Some(header) = parts.headers.get(axum::http::header::AUTHORIZATION) else {
            return Ok(MaybeUser(None));
        };
        let token = header
            .to_str()
            .ok()
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(str::trim)
            .ok_or(AppError::Unauthorized)?;
        authenticate(state, token).await.map(|u| MaybeUser(Some(u)))
    }
}

async fn authenticate(state: &AppState, token: &str) -> AppResult<CurrentUser> {
    let conn = state.conn().await?;
    let now = fmt_ts(model::now());
    let mut rows = conn
        .query(
            &format!(
                "SELECT t.id, t.kind, {cols} FROM tokens t JOIN users u ON u.id = t.user_id
                 WHERE t.token_hash = ?1 AND (t.expires_at IS NULL OR t.expires_at > ?2)",
                cols = model::USER_COLUMNS
            ),
            libsql::params![token_hash(token), now.clone()],
        )
        .await?;
    let row = rows.next().await?.ok_or(AppError::Unauthorized)?;
    let token_id: Uuid = model::uuid(&row, 0)?;
    let kind: String = row.get(1)?;
    let user = model::user_from_row(&row, 2)?;
    if kind == TokenKind::Api.as_str() {
        conn.execute(
            "UPDATE tokens SET last_used_at = ?1 WHERE id = ?2",
            libsql::params![now, token_id.to_string()],
        )
        .await?;
    }
    Ok(CurrentUser { user, token_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_round_trip() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong password!", &hash));
        assert!(!verify_password("anything", "not a hash"));
    }

    #[test]
    fn token_hash_is_hex_sha256() {
        assert_eq!(token_hash("abc").len(), 64);
        assert_ne!(token_hash("abc"), token_hash("abd"));
    }
}
