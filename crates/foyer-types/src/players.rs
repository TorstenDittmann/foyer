//! Player accounts: wishlists, following developers, release emails and
//! passkeys.
//!
//! Accounts are optional for players: browsing and downloading never need
//! one. An account adds a wishlist, follows, and passkey sign-in.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{DeveloperSummary, Game};

/// A game on the caller's wishlist (`GET /v1/me/wishlist`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WishlistEntry {
    pub game: Game,
    pub added_at: DateTime<Utc>,
}

/// A developer the caller follows (`GET /v1/me/following`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Follow {
    pub developer: DeveloperSummary,
    pub followed_at: DateTime<Utc>,
}

/// The caller's account settings (`GET /v1/me/settings`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountSettings {
    /// Email me when a developer I follow releases a game. Off by default.
    /// Only confirmed addresses get these emails.
    pub release_emails: bool,
}

/// `PATCH /v1/me/settings`. Omitted fields stay unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateAccountSettings {
    pub release_emails: Option<bool>,
}

/// `POST /v1/unsubscribe`: the token from the link in a release email.
/// Turns release emails off without signing in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UnsubscribeRequest {
    pub token: String,
}

/// A passkey on the caller's account, without its key material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PasskeyInfo {
    pub id: Uuid,
    /// Chosen when it was added, e.g. "MacBook" or "Phone".
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// The start of a passkey registration or sign-in. Pass `options` to the
/// browser's WebAuthn API (`navigator.credentials.create` or `.get`, after
/// decoding the base64url fields), then send its answer back with
/// `ceremony` within five minutes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PasskeyChallenge {
    pub ceremony: Uuid,
    /// WebAuthn `CredentialCreationOptions` or `CredentialRequestOptions`
    /// as JSON, binary fields base64url-encoded.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub options: serde_json::Value,
}

/// `POST /v1/auth/passkeys/register/finish`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FinishPasskeyRegistration {
    pub ceremony: Uuid,
    /// Up to 60 characters; "Passkey" when empty.
    #[serde(default)]
    pub name: String,
    /// The browser's `PublicKeyCredential` from `navigator.credentials.create`,
    /// binary fields base64url-encoded.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub credential: serde_json::Value,
}

/// `POST /v1/auth/passkeys/login/finish`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FinishPasskeyLogin {
    pub ceremony: Uuid,
    /// The browser's `PublicKeyCredential` from `navigator.credentials.get`,
    /// binary fields base64url-encoded.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub credential: serde_json::Value,
}
