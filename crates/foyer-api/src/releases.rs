//! Release emails: when a game is published for the first time, followers of
//! its developer who turned release emails on get one email each.
//!
//! Publishing queues a `notify_release` job; the worker fans it out into
//! `send_email` jobs. `release_notices` records who was told, so a retried
//! job never emails anyone twice. Only confirmed addresses are emailed.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth;
use crate::jobs::{self, JobKind};
use crate::mail::{self, Email};
use crate::model::{self, fmt_ts};
use crate::worker::JobError;

/// Payload of a `notify_release` job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyRelease {
    pub game_id: Uuid,
}

/// Queues the release emails for a game. Pass the transaction that
/// publishes it.
pub async fn queue(conn: &libsql::Connection, game_id: Uuid) -> Result<(), libsql::Error> {
    jobs::enqueue(conn, JobKind::NotifyRelease, &NotifyRelease { game_id }).await?;
    Ok(())
}

struct Follower {
    id: Uuid,
    username: String,
    email: String,
    unsubscribe_token: Option<String>,
}

/// Runs a `notify_release` job.
pub(crate) async fn notify(state: &AppState, game_id: Uuid) -> Result<(), JobError> {
    let conn = crate::db::conn(&state.db).await?;
    let game = match model::game_by_id(state, &conn, game_id).await {
        Ok(record) => record.game,
        // Deleted since; nothing to announce.
        Err(crate::error::AppError::NotFound(_)) => return Ok(()),
        Err(err) => return Err(crate::worker::app_error(err)),
    };
    if game.status != foyer_types::GameStatus::Published {
        // Taken down before the job ran.
        return Ok(());
    }

    let mut rows = conn
        .query(
            "SELECT u.id, u.username, u.email, u.unsubscribe_token FROM follows f
             JOIN games g ON g.developer_id = f.developer_id
             JOIN users u ON u.id = f.user_id
             WHERE g.id = ?1 AND u.release_emails = 1 AND u.email_verified_at IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM release_notices n
                               WHERE n.game_id = g.id AND n.user_id = u.id)",
            [game_id.to_string()],
        )
        .await?;
    let mut followers = Vec::new();
    while let Some(row) = rows.next().await? {
        followers.push(Follower {
            id: model::uuid(&row, 0)?,
            username: row.get(1)?,
            email: row.get(2)?,
            unsubscribe_token: row.get(3)?,
        });
    }
    drop(rows);

    let game_link = state.web_link(&format!("/games/{}", game.slug), &[]);
    let now = fmt_ts(model::now());
    for follower in followers {
        let token = match follower.unsubscribe_token {
            Some(token) => token,
            None => {
                // Another worker may be announcing another game to the same
                // follower right now; keep whichever token was stored first,
                // so every link sent stays valid.
                let fresh = auth::random_secret("").map_err(crate::worker::app_error)?;
                let mut rows = conn
                    .query(
                        "UPDATE users SET unsubscribe_token = coalesce(unsubscribe_token, ?1)
                         WHERE id = ?2 RETURNING unsubscribe_token",
                        [fresh, follower.id.to_string()],
                    )
                    .await?;
                let row = rows
                    .next()
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("follower {} vanished", follower.id))?;
                row.get::<String>(0)?
            }
        };
        let unsubscribe = state.web_link("/unsubscribe", &[("token", &token)]);
        let email = release_email(
            &follower.email,
            &follower.username,
            &game,
            &game_link,
            &unsubscribe,
        );
        // The notice and the email commit together.
        let tx = conn.transaction().await?;
        let inserted = tx
            .execute(
                "INSERT INTO release_notices (game_id, user_id, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT DO NOTHING",
                [game_id.to_string(), follower.id.to_string(), now.clone()],
            )
            .await?;
        if inserted == 1 {
            mail::queue(&tx, &email).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

pub fn release_email(
    to: &str,
    username: &str,
    game: &foyer_types::Game,
    game_link: &str,
    unsubscribe_link: &str,
) -> Email {
    let developer = &game.developer.display_name;
    let tagline = if game.tagline.is_empty() {
        String::new()
    } else {
        format!("{}\n\n", game.tagline)
    };
    Email {
        to: to.to_string(),
        subject: format!("{developer} released {} on Foyer", game.title),
        body: format!(
            "Hi {username},\n\n\
             {developer} just released a new game on Foyer: {title}\n\n\
             {tagline}\
             It's free to download:\n\n{game_link}\n\n\
             You get this email because you follow {developer} and turned on \
             release emails. To stop them, open this link:\n\n{unsubscribe_link}\n\n\
             Foyer\n",
            title = game.title,
        ),
    }
}
