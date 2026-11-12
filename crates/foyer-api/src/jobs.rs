//! A job queue in a libSQL table, worked by `foyer-api worker`.
//!
//! Handlers enqueue jobs in the same transaction as the change that needs
//! them, so a job exists exactly when its change was committed. Workers
//! claim one due job at a time with a lease (`locked_until`); a worker that
//! dies leaves the lease to run out and another worker takes the job over.
//! Failed attempts are retried with exponential backoff until
//! `max_attempts`.

use chrono::{DateTime, Duration, Utc};
use libsql::Connection;
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::model::{self, fmt_ts};

/// How long a claimed job is reserved before another worker may take it.
/// Workers renew the lease while they work.
pub const LEASE: Duration = Duration::minutes(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    /// Scan a finalized build for malware, pack its zip and mark it ready.
    ProcessBuild,
    /// Send one email.
    SendEmail,
    /// Email the followers of a newly released game's developer.
    NotifyRelease,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::ProcessBuild => "process_build",
            JobKind::SendEmail => "send_email",
            JobKind::NotifyRelease => "notify_release",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "process_build" => Some(JobKind::ProcessBuild),
            "send_email" => Some(JobKind::SendEmail),
            "notify_release" => Some(JobKind::NotifyRelease),
            _ => None,
        }
    }

    fn max_attempts(self) -> i64 {
        match self {
            JobKind::ProcessBuild => 5,
            JobKind::SendEmail | JobKind::NotifyRelease => 8,
        }
    }
}

/// Adds a job that is due now. Pass a transaction to tie it to a change.
pub async fn enqueue(
    conn: &Connection,
    kind: JobKind,
    payload: &impl Serialize,
) -> Result<Uuid, libsql::Error> {
    let id = Uuid::now_v7();
    let now = fmt_ts(model::now());
    let payload = serde_json::to_string(payload)
        .map_err(|e| libsql::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO jobs (id, kind, payload, max_attempts, run_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        libsql::params![
            id.to_string(),
            kind.as_str(),
            payload,
            kind.max_attempts(),
            now
        ],
    )
    .await?;
    Ok(id)
}

/// A job a worker has claimed.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: Uuid,
    pub kind: String,
    pub payload: String,
    /// Including this one.
    pub attempts: i64,
    pub max_attempts: i64,
}

impl Job {
    pub fn payload<T: DeserializeOwned>(&self) -> anyhow::Result<T> {
        Ok(serde_json::from_str(&self.payload)?)
    }

    pub fn is_last_attempt(&self) -> bool {
        self.attempts >= self.max_attempts
    }
}

/// Claims the oldest due job: a queued one whose time has come, or a running
/// one whose lease ran out.
pub async fn claim(conn: &Connection) -> anyhow::Result<Option<Job>> {
    let now = model::now();
    let mut rows = conn
        .query(
            "UPDATE jobs SET state = 'running', attempts = attempts + 1, locked_until = ?2
             WHERE id = (
                 SELECT id FROM jobs
                 WHERE (state = 'queued' AND run_at <= ?1)
                    OR (state = 'running' AND locked_until <= ?1)
                 ORDER BY run_at, id LIMIT 1
             )
             RETURNING id, kind, payload, attempts, max_attempts",
            [fmt_ts(now), fmt_ts(now + LEASE)],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    Ok(Some(Job {
        id: model::uuid(&row, 0)?,
        kind: row.get(1)?,
        payload: row.get(2)?,
        attempts: row.get(3)?,
        max_attempts: row.get(4)?,
    }))
}

/// Extends the lease of a job that is still being worked on.
pub async fn renew(conn: &Connection, id: Uuid) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE jobs SET locked_until = ?1 WHERE id = ?2 AND state = 'running'",
        [fmt_ts(model::now() + LEASE), id.to_string()],
    )
    .await?;
    Ok(())
}

pub async fn complete(conn: &Connection, id: Uuid) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE jobs SET state = 'done', locked_until = NULL, finished_at = ?1 WHERE id = ?2",
        [fmt_ts(model::now()), id.to_string()],
    )
    .await?;
    Ok(())
}

/// Records a failed attempt. The job is retried later unless `permanent` is
/// set or it has no attempts left; then it ends as `failed`.
pub async fn fail(
    conn: &Connection,
    job: &Job,
    error: &str,
    permanent: bool,
) -> anyhow::Result<()> {
    let now = model::now();
    if permanent || job.is_last_attempt() {
        conn.execute(
            "UPDATE jobs SET state = 'failed', locked_until = NULL, last_error = ?1,
               finished_at = ?2 WHERE id = ?3",
            [error.to_string(), fmt_ts(now), job.id.to_string()],
        )
        .await?;
    } else {
        conn.execute(
            "UPDATE jobs SET state = 'queued', locked_until = NULL, last_error = ?1, run_at = ?2
             WHERE id = ?3",
            [
                error.to_string(),
                fmt_ts(retry_at(now, job.attempts)),
                job.id.to_string(),
            ],
        )
        .await?;
    }
    Ok(())
}

/// 30 s, 1 min, 2 min, … capped at one hour.
fn retry_at(now: DateTime<Utc>, attempts: i64) -> DateTime<Utc> {
    let seconds = 30i64.saturating_mul(1 << attempts.clamp(1, 7).saturating_sub(1));
    now + Duration::seconds(seconds.min(3600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_is_capped() {
        let now = model::now();
        assert_eq!(retry_at(now, 1) - now, Duration::seconds(30));
        assert_eq!(retry_at(now, 2) - now, Duration::seconds(60));
        assert_eq!(retry_at(now, 20) - now, Duration::seconds(1920));
    }
}
