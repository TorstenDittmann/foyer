//! Tester keys: private access to a game before it is published, or to its
//! private channels (betas, playtests) after.
//!
//! A developer creates a key for one game, optionally limited to some
//! channels, and shares it. Players need no account: clients send the keys
//! they hold in the `X-Foyer-Key` header (several separated by commas), and
//! every public endpoint then also serves what those keys unlock.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The header clients send tester keys in.
pub const TESTER_KEY_HEADER: &str = "x-foyer-key";

/// Most keys one request may carry.
pub const MAX_KEYS_PER_REQUEST: usize = 20;

/// A tester key as the developer sees it. The secret is shown only once,
/// in [`CreatedTesterKey`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TesterKey {
    pub id: Uuid,
    /// The developer's note, e.g. who the key is for.
    pub label: String,
    /// Channels the key unlocks. Empty means every channel.
    pub channels: Vec<String>,
    /// The first characters of the secret, to tell keys apart.
    pub prefix: String,
    pub created_at: DateTime<Utc>,
    /// The key stops working after this. `None` never expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// Last time a client used the key, to the day.
    pub last_used_at: Option<DateTime<Utc>>,
}

/// `POST /v1/games/{slug}/tester-keys`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateTesterKey {
    /// Up to 80 characters.
    #[serde(default)]
    pub label: String,
    /// Channels to unlock; empty for all of them. Channels that don't exist
    /// yet are fine, so a key can be handed out before the first push.
    #[serde(default)]
    pub channels: Vec<String>,
    /// Days until the key expires, 1 to 365. Default 30.
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreatedTesterKey {
    pub key: TesterKey,
    /// The key itself (`fyk_...`). Store it now; it can't be shown again.
    pub secret: String,
    /// A website link that unlocks the game in the visitor's browser.
    pub link: String,
}

/// `POST /v1/tester-keys/redeem`: which game a key unlocks, so clients can
/// add a key without knowing the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RedeemTesterKey {
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TesterAccess {
    /// The game's slug.
    pub game: String,
    pub title: String,
    /// Channels the key unlocks. Empty means every channel.
    pub channels: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}
