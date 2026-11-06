//! Tester keys: who may see an unpublished game or a private channel.
//!
//! Every endpoint players use takes a [`Viewer`]: the signed-in user, if
//! any, plus the tester keys the client sent in `X-Foyer-Key`. Keys that are
//! unknown or expired are ignored, so a stale key never breaks browsing.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{Duration, NaiveTime};
use foyer_types::{MAX_KEYS_PER_REQUEST, TESTER_KEY_HEADER};
use libsql::Connection;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{self, CurrentUser, MaybeUser};
use crate::error::{AppError, AppResult};
use crate::model::{self, fmt_ts};

/// Every tester key starts with this.
pub const KEY_PREFIX: &str = "fyk_";

/// What one tester key unlocks.
#[derive(Debug, Clone)]
pub struct KeyGrant {
    pub key_id: Uuid,
    pub game_id: Uuid,
    /// Empty means every channel.
    pub channels: Vec<String>,
}

impl KeyGrant {
    pub fn covers(&self, channel: &str) -> bool {
        self.channels.is_empty() || self.channels.iter().any(|c| c == channel)
    }
}

/// The caller of a player-facing endpoint.
#[derive(Debug, Clone, Default)]
pub struct Viewer {
    pub user: Option<CurrentUser>,
    pub keys: Vec<KeyGrant>,
}

impl Viewer {
    pub fn signed_in(user: CurrentUser) -> Self {
        Self {
            user: Some(user),
            keys: Vec::new(),
        }
    }

    pub fn user(&self) -> Option<&CurrentUser> {
        self.user.as_ref()
    }

    /// The caller's key for `game_id`, if any. With several, the one that
    /// covers the most channels.
    pub fn grant(&self, game_id: Uuid) -> Option<&KeyGrant> {
        self.keys
            .iter()
            .filter(|k| k.game_id == game_id)
            .max_by_key(|k| k.channels.is_empty())
    }

    pub fn grant_covering(&self, game_id: Uuid, channel: &str) -> Option<&KeyGrant> {
        self.keys
            .iter()
            .find(|k| k.game_id == game_id && k.covers(channel))
    }
}

impl FromRequestParts<AppState> for Viewer {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> AppResult<Self> {
        let MaybeUser(user) = MaybeUser::from_request_parts(parts, state).await?;
        let secrets = header_keys(parts);
        let keys = if secrets.is_empty() {
            Vec::new()
        } else {
            lookup(&state.conn().await?, &secrets).await?
        };
        Ok(Viewer { user, keys })
    }
}

/// The well-formed keys in every `X-Foyer-Key` header, deduplicated.
fn header_keys(parts: &Parts) -> Vec<String> {
    let mut keys: Vec<String> = parts
        .headers
        .get_all(TESTER_KEY_HEADER)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|k| k.starts_with(KEY_PREFIX) && k.len() <= 100)
        .map(str::to_string)
        .collect();
    keys.sort();
    keys.dedup();
    keys.truncate(MAX_KEYS_PER_REQUEST);
    keys
}

/// Loads the keys that exist and haven't expired, and notes today's use.
pub async fn lookup(conn: &Connection, secrets: &[String]) -> AppResult<Vec<KeyGrant>> {
    let now = model::now();
    let mut grants = Vec::new();
    for secret in secrets {
        let mut rows = conn
            .query(
                "SELECT id, game_id, channels, last_used_at FROM tester_keys
                 WHERE key_hash = ?1 AND (expires_at IS NULL OR expires_at > ?2)",
                libsql::params![auth::secret_hash(secret), fmt_ts(now)],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            continue;
        };
        let grant = KeyGrant {
            key_id: model::uuid(&row, 0)?,
            game_id: model::uuid(&row, 1)?,
            channels: model::json(&row, 2)?,
        };
        let last_used = model::opt_ts(&row, 3)?;
        drop(rows);
        // One write per key and day at most: "last used" is shown to the day.
        let today = now.date_naive().and_time(NaiveTime::MIN).and_utc();
        if last_used.is_none_or(|t| t < today) {
            conn.execute(
                "UPDATE tester_keys SET last_used_at = ?1 WHERE id = ?2",
                libsql::params![fmt_ts(now), grant.key_id.to_string()],
            )
            .await?;
        }
        grants.push(grant);
    }
    Ok(grants)
}

/// Default and longest lifetime of a key.
pub const DEFAULT_TTL_DAYS: u32 = 30;
pub const MAX_TTL_DAYS: u32 = 365;
/// Keys one game may have at a time.
pub const MAX_KEYS_PER_GAME: i64 = 200;

pub fn ttl(days: Option<u32>) -> AppResult<Duration> {
    let days = days.unwrap_or(DEFAULT_TTL_DAYS);
    if !(1..=MAX_TTL_DAYS).contains(&days) {
        return Err(AppError::validation(format!(
            "expires_in_days must be 1 to {MAX_TTL_DAYS}"
        )));
    }
    Ok(Duration::days(i64::from(days)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(game: Uuid, channels: &[&str]) -> KeyGrant {
        KeyGrant {
            key_id: Uuid::now_v7(),
            game_id: game,
            channels: channels.iter().map(|c| c.to_string()).collect(),
        }
    }

    #[test]
    fn empty_channel_list_covers_everything() {
        let game = Uuid::now_v7();
        assert!(grant(game, &[]).covers("beta"));
        assert!(grant(game, &["beta"]).covers("beta"));
        assert!(!grant(game, &["beta"]).covers("stable"));
    }

    #[test]
    fn viewer_prefers_the_widest_key() {
        let game = Uuid::now_v7();
        let viewer = Viewer {
            user: None,
            keys: vec![
                grant(game, &["beta"]),
                grant(game, &[]),
                grant(Uuid::now_v7(), &[]),
            ],
        };
        assert!(viewer.grant(game).unwrap().channels.is_empty());
        assert!(viewer.grant_covering(game, "stable").is_some());
        assert!(viewer.grant(Uuid::now_v7()).is_none());
    }

    #[test]
    fn header_keys_are_split_and_filtered() {
        let req = axum::http::Request::builder()
            .header(TESTER_KEY_HEADER, "fyk_b, fyk_a,junk")
            .header(TESTER_KEY_HEADER, "fyk_a")
            .body(())
            .unwrap();
        let (parts, _) = req.into_parts();
        assert_eq!(header_keys(&parts), ["fyk_a", "fyk_b"]);
    }

    #[test]
    fn ttl_bounds() {
        assert_eq!(ttl(None).unwrap(), Duration::days(30));
        assert!(ttl(Some(0)).is_err());
        assert!(ttl(Some(366)).is_err());
    }
}
