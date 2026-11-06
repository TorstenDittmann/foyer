//! `foyer-api` serves the HTTP API; `foyer-api worker` runs background jobs
//! (build scans and zips, emails). Both read the same `FOYER_*` variables.

use std::net::SocketAddr;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

use foyer_api::mail::Mailer;
use foyer_api::storage::Storage;
use foyer_api::worker::{self, WorkerConfig};
use foyer_api::{AppState, Config, db, router};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "foyer_api=info,tower_http=info".into()),
        )
        .init();

    let command = std::env::args().nth(1);
    let config = Config::from_env()?;
    let db = db::connect(&config.database).await?;
    db::migrate(&db).await?;

    let mut state = AppState::from_config(db, &config)?;
    match &config.storage {
        Some(storage) => state = state.with_storage(Storage::new(storage)?),
        None => tracing::warn!("FOYER_S3_BUCKET is not set; build and media uploads are disabled"),
    }

    match command.as_deref() {
        None | Some("serve") => serve(state, &config).await,
        Some("worker") => {
            if state.storage.is_none() {
                anyhow::bail!("the worker needs object storage; set FOYER_S3_BUCKET");
            }
            let mailer = match &config.smtp_url {
                Some(url) => Some(std::sync::Arc::new(Mailer::new(url, &config.mail_from)?)),
                None => {
                    tracing::warn!("FOYER_SMTP_URL is not set; emails are logged, not sent");
                    None
                }
            };
            let config = WorkerConfig {
                clamd_addr: config.clamd_addr.clone(),
                mailer,
            };
            if config.clamd_addr.is_none() {
                tracing::warn!("FOYER_CLAMD_ADDR is not set; builds are not scanned for malware");
            }
            worker::run(state, config, shutdown_signal()).await
        }
        Some(other) => {
            anyhow::bail!("unknown command {other:?}; use `serve` (default) or `worker`")
        }
    }
}

async fn serve(state: AppState, config: &Config) -> anyhow::Result<()> {
    let app = router(state);
    let addr: SocketAddr = config.bind.parse().context("invalid FOYER_BIND address")?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// Ctrl-C, or SIGTERM from the container runtime.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = term => {}
    }
    tracing::info!("shutting down");
}
