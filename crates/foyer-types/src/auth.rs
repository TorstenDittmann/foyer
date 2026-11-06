use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a user is allowed to do beyond managing their own content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Players and developers.
    User,
    /// Can work the review queue.
    Reviewer,
    /// Reviewer who can also change other users' roles.
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Reviewer => "reviewer",
            Role::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Role::User),
            "reviewer" => Some(Role::Reviewer),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }

    /// Reviewers and admins can review submissions.
    pub fn is_staff(self) -> bool {
        matches!(self, Role::Reviewer | Role::Admin)
    }
}

/// The signed-in user (`GET /v1/me`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}

/// Public view of a user, e.g. the reviewer on a submission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserSummary {
    pub id: Uuid,
    pub username: String,
}

/// `POST /v1/auth/register`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RegisterRequest {
    /// 3 to 32 characters: lowercase letters, digits, `-` and `_`.
    pub username: String,
    pub email: String,
    /// At least 10 characters.
    pub password: String,
}

/// `POST /v1/auth/login`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LoginRequest {
    /// Email address or username.
    pub login: String,
    pub password: String,
}

/// A new session. Send `token` as `Authorization: Bearer <token>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AuthResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub user: User,
}

/// `POST /v1/auth/tokens`: a long-lived API token for the CLI or CI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateApiTokenRequest {
    /// Shown in the token list, e.g. "GitHub Actions".
    pub name: String,
}

/// An API token, without its secret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ApiToken {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Returned once, when a token is created. The secret cannot be read again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreatedApiToken {
    pub token: String,
    pub info: ApiToken,
}

/// `PUT /v1/admin/users/{id}/role`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetRoleRequest {
    pub role: Role,
}
