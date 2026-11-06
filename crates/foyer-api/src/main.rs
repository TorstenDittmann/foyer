use std::net::SocketAddr;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

use foyer_api::{AppState, Config, db, router};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "foyer_api=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    let db = db::connect(&config.database).await?;
    db::migrate(&db).await?;

    let app = router(AppState::new(db));
    let addr: SocketAddr = config.bind.parse().context("invalid FOYER_BIND address")?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
