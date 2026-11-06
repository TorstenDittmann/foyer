//! Outgoing email. Placeholder until the auth work lands.

use crate::AppState;
use crate::jobs::Job;
use crate::worker::JobError;

pub(crate) async fn send_job(_state: &AppState, job: &Job) -> Result<(), JobError> {
    Err(JobError::Permanent(format!(
        "email jobs are not supported yet ({})",
        job.id
    )))
}
