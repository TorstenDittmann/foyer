//! Outgoing email: verification and password reset links.
//!
//! Handlers queue a `send_email` job; the worker sends it over SMTP
//! (`FOYER_SMTP_URL`, `FOYER_MAIL_FROM`). Without SMTP the worker logs the
//! message instead, which is enough for development. A sent message's payload
//! is cleared, so the jobs table does not keep working links around.

use lettre::message::Mailbox;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use libsql::Connection;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::jobs::{self, Job, JobKind};
use crate::worker::{JobError, WorkerConfig};

/// Payload of a `send_email` job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub body: String,
}

pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Mailer {
    /// `url` as lettre reads it, e.g. `smtps://user:pass@smtp.example.com`
    /// or `smtp://host:587?tls=required`.
    pub fn new(url: &str, from: &str) -> anyhow::Result<Self> {
        Ok(Self {
            transport: AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
                .map_err(|e| anyhow::anyhow!("FOYER_SMTP_URL: {e}"))?
                .build(),
            from: from
                .parse()
                .map_err(|e| anyhow::anyhow!("FOYER_MAIL_FROM: {e}"))?,
        })
    }
}

/// Queues an email. Pass a transaction to send it only if the change
/// commits.
pub async fn queue(conn: &Connection, email: &Email) -> Result<(), libsql::Error> {
    jobs::enqueue(conn, JobKind::SendEmail, email).await?;
    Ok(())
}

pub(crate) async fn send_job(
    state: &AppState,
    config: &WorkerConfig,
    job: &Job,
) -> Result<(), JobError> {
    let email: Email = match job.payload() {
        Ok(email) => email,
        // Already sent and cleared by an earlier attempt.
        Err(_) if job.payload == "{}" => return Ok(()),
        Err(e) => return Err(JobError::Permanent(format!("bad payload: {e}"))),
    };
    match &config.mailer {
        Some(mailer) => {
            let to: Mailbox = email
                .to
                .parse()
                .map_err(|e| JobError::Permanent(format!("bad recipient {:?}: {e}", email.to)))?;
            let message = Message::builder()
                .from(mailer.from.clone())
                .to(to)
                .subject(&email.subject)
                .body(email.body.clone())
                .map_err(|e| JobError::Permanent(format!("bad message: {e}")))?;
            mailer
                .transport
                .send(message)
                .await
                .map_err(|e| JobError::Retry(anyhow::anyhow!("SMTP: {e}")))?;
            tracing::info!(to = %email.to, subject = %email.subject, "email sent");
        }
        None => {
            tracing::info!(
                to = %email.to,
                subject = %email.subject,
                body = %email.body,
                "FOYER_SMTP_URL is not set; email not sent"
            );
        }
    }
    let conn = crate::db::conn(&state.db).await?;
    conn.execute(
        "UPDATE jobs SET payload = '{}' WHERE id = ?1",
        [job.id.to_string()],
    )
    .await?;
    Ok(())
}

pub fn verification(to: &str, username: &str, link: &str) -> Email {
    Email {
        to: to.to_string(),
        subject: "Confirm your email for Foyer".into(),
        body: format!(
            "Hi {username},\n\n\
             Open this link to confirm your email address:\n\n{link}\n\n\
             The link works for 48 hours. If you didn't create a Foyer account, \
             you can ignore this email.\n\n\
             Foyer\n"
        ),
    }
}

pub fn password_reset(to: &str, username: &str, link: &str) -> Email {
    Email {
        to: to.to_string(),
        subject: "Reset your Foyer password".into(),
        body: format!(
            "Hi {username},\n\n\
             Someone asked to reset the password of your Foyer account. Open this \
             link to choose a new one:\n\n{link}\n\n\
             The link works for one hour. If it wasn't you, ignore this email; \
             your password stays the same.\n\n\
             Foyer\n"
        ),
    }
}
