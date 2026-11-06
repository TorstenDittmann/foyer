use anyhow::Context;
use libsql::{Builder, Connection, Database};

use crate::config::DatabaseConfig;

/// Schema migrations, applied in order and tracked in `_migrations`.
///
/// Append new entries; never edit or reorder ones that have shipped.
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_core", include_str!("migrations/0001_core.sql")),
    (
        "0002_worker_downloads",
        include_str!("migrations/0002_worker_downloads.sql"),
    ),
    ("0003_search", include_str!("migrations/0003_search.sql")),
];

pub async fn connect(config: &DatabaseConfig) -> anyhow::Result<Database> {
    let db = match config {
        DatabaseConfig::Local { path } => Builder::new_local(path).build().await,
        DatabaseConfig::Remote { url, auth_token } => {
            Builder::new_remote(url.clone(), auth_token.clone())
                .build()
                .await
        }
    };
    db.context("failed to open database")
}

/// Opens a connection with the settings every request needs.
pub async fn conn(db: &Database) -> Result<Connection, libsql::Error> {
    let conn = db.connect()?;
    // Foreign keys are off by default in SQLite. sqld and Turso enforce them
    // per connection too, so this is safe on both backends.
    conn.execute("PRAGMA foreign_keys = ON", ()).await?;
    Ok(conn)
}

pub async fn migrate(db: &Database) -> anyhow::Result<()> {
    let conn = conn(db).await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        (),
    )
    .await?;

    for (name, sql) in MIGRATIONS {
        let mut rows = conn
            .query("SELECT 1 FROM _migrations WHERE name = ?1", [*name])
            .await?;
        if rows.next().await?.is_some() {
            continue;
        }
        tracing::info!(migration = name, "applying migration");
        let tx = conn.transaction().await?;
        tx.execute_batch(sql)
            .await
            .with_context(|| format!("migration {name} failed"))?;
        tx.execute("INSERT INTO _migrations (name) VALUES (?1)", [*name])
            .await?;
        tx.commit().await?;
    }
    Ok(())
}

/// Cheap round trip used by the health check.
pub async fn ping(db: &Database) -> anyhow::Result<()> {
    let conn = db.connect()?;
    let mut rows = conn.query("SELECT 1", ()).await?;
    rows.next().await?.context("SELECT 1 returned no rows")?;
    Ok(())
}
