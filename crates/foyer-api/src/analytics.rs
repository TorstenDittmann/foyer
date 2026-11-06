//! Anonymous download counters: one row per game, platform, source and UTC
//! day, incremented in place. The daily rows are the rollup; nothing about
//! the downloader is stored, so there is nothing to aggregate later.

use foyer_types::Platform;
use libsql::Connection;
use uuid::Uuid;

use crate::model;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A zip download from the website.
    Web,
    ClientInstall,
    ClientUpdate,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Web => "web",
            Source::ClientInstall => "client_install",
            Source::ClientUpdate => "client_update",
        }
    }
}

/// Adds one download for today.
pub async fn count_download(
    conn: &Connection,
    game_id: Uuid,
    platform: Platform,
    source: Source,
) -> Result<(), libsql::Error> {
    let day = model::now().date_naive().to_string();
    conn.execute(
        "INSERT INTO download_counts (game_id, platform, source, day, count)
         VALUES (?1, ?2, ?3, ?4, 1)
         ON CONFLICT (game_id, platform, source, day) DO UPDATE SET count = count + 1",
        [
            game_id.to_string(),
            platform.as_str().to_string(),
            source.as_str().to_string(),
            day,
        ],
    )
    .await?;
    Ok(())
}
