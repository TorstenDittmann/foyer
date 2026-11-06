//! `foyer-api worker`: runs jobs from the [`jobs`](crate::jobs) table.
//!
//! One worker runs one job at a time; run more replicas for more throughput.
//! Jobs that fail are retried with backoff (see [`jobs::fail`]).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::AppState;
use crate::error::AppError;
use crate::jobs::{self, Job, JobKind};
use crate::mail::Mailer;

mod build;

pub use build::ProcessBuild;

/// Settings only the worker needs.
#[derive(Clone, Default)]
pub struct WorkerConfig {
    /// clamd's `host:port` (`FOYER_CLAMD_ADDR`). Without it builds are not
    /// scanned and are marked `skipped`.
    pub clamd_addr: Option<String>,
    /// SMTP for `send_email` jobs. Without it emails are only logged.
    pub mailer: Option<Arc<Mailer>>,
}

/// How long to wait when the queue is empty.
const IDLE: Duration = Duration::from_secs(2);

/// Why a job attempt failed.
#[derive(Debug)]
pub enum JobError {
    /// Might work next time (storage or scanner unreachable, …).
    Retry(anyhow::Error),
    /// Will never work (bad input); don't retry.
    Permanent(String),
}

impl JobError {
    fn message(&self) -> String {
        match self {
            JobError::Retry(err) => format!("{err:#}"),
            JobError::Permanent(msg) => msg.clone(),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for JobError {
    fn from(err: E) -> Self {
        JobError::Retry(err.into())
    }
}

pub(crate) fn app_error(err: AppError) -> JobError {
    match err {
        AppError::Internal(err) => JobError::Retry(err),
        other => JobError::Retry(anyhow::anyhow!("{other:?}")),
    }
}

/// Works the queue until `shutdown` resolves. A job in progress at shutdown
/// is abandoned; its lease runs out and it is retried.
pub async fn run(
    state: AppState,
    config: WorkerConfig,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    tracing::info!(
        clamd = config
            .clamd_addr
            .as_deref()
            .unwrap_or("(none, scans skipped)"),
        "worker started"
    );
    tokio::pin!(shutdown);
    loop {
        let idle = tokio::select! {
            () = &mut shutdown => return Ok(()),
            ran = run_one(&state, &config) => match ran {
                Ok(true) => false,
                Ok(false) => true,
                Err(err) => {
                    tracing::error!(error = ?err, "worker loop failed");
                    true
                }
            },
        };
        if idle {
            tokio::select! {
                () = &mut shutdown => return Ok(()),
                () = tokio::time::sleep(IDLE) => {}
            }
        }
    }
}

/// Runs every due job, one after another, and returns how many ran. For
/// tests and one-off maintenance.
pub async fn run_pending(state: &AppState, config: &WorkerConfig) -> anyhow::Result<usize> {
    let mut ran = 0;
    while run_one(state, config).await? {
        ran += 1;
    }
    Ok(ran)
}

/// Claims and runs one due job. Returns false when none was due.
pub async fn run_one(state: &AppState, config: &WorkerConfig) -> anyhow::Result<bool> {
    let conn = crate::db::conn(&state.db).await?;
    let Some(job) = jobs::claim(&conn).await? else {
        return Ok(false);
    };
    tracing::info!(job = %job.id, kind = %job.kind, attempt = job.attempts, "running job");

    let renewer = {
        let state = state.clone();
        let id = job.id;
        tokio::spawn(async move { renew_lease(state, id).await })
    };
    let result = execute(state, config, &job).await;
    renewer.abort();

    match result {
        Ok(()) => {
            jobs::complete(&conn, job.id).await?;
            tracing::info!(job = %job.id, kind = %job.kind, "job done");
        }
        Err(err) => {
            let permanent = matches!(err, JobError::Permanent(_));
            let message = err.message();
            tracing::warn!(job = %job.id, kind = %job.kind, error = %message, permanent, "job failed");
            jobs::fail(&conn, &job, &message, permanent).await?;
            if permanent || job.is_last_attempt() {
                on_gave_up(state, &job, &message).await?;
            }
        }
    }
    Ok(true)
}

async fn renew_lease(state: AppState, id: Uuid) {
    let every = (jobs::LEASE / 3).to_std().expect("positive lease");
    loop {
        tokio::time::sleep(every).await;
        let renewed = match crate::db::conn(&state.db).await {
            Ok(conn) => jobs::renew(&conn, id).await,
            Err(err) => Err(err.into()),
        };
        if let Err(err) = renewed {
            tracing::warn!(job = %id, error = %err, "could not renew job lease");
        }
    }
}

async fn execute(state: &AppState, config: &WorkerConfig, job: &Job) -> Result<(), JobError> {
    match JobKind::parse(&job.kind) {
        Some(JobKind::ProcessBuild) => {
            let payload: ProcessBuild = job
                .payload()
                .map_err(|e| JobError::Permanent(format!("bad payload: {e}")))?;
            build::process(state, config, payload.build_id).await
        }
        Some(JobKind::SendEmail) => crate::mail::send_job(state, config, job).await,
        None => Err(JobError::Permanent(format!(
            "unknown job kind {}",
            job.kind
        ))),
    }
}

/// Cleanup when a job will not be retried any more.
async fn on_gave_up(state: &AppState, job: &Job, message: &str) -> anyhow::Result<()> {
    if JobKind::parse(&job.kind) == Some(JobKind::ProcessBuild)
        && let Ok(payload) = job.payload::<ProcessBuild>()
    {
        build::mark_failed(state, payload.build_id, message).await?;
    }
    Ok(())
}
