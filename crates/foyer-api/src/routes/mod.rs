use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tower_http::trace::TraceLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::{AppState, db, openapi};

mod auth;
mod builds;
mod developers;
mod downloads;
mod games;
mod review;

/// Every `/v1` route, with its OpenAPI description.
pub(crate) fn api_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(openapi::base())
        .merge(auth::routes())
        .merge(developers::routes())
        .merge(games::routes())
        .merge(builds::routes())
        .merge(downloads::routes())
        .merge(review::routes())
}

pub fn router(state: AppState) -> Router {
    let (api, spec) = api_routes().split_for_parts();
    Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(move || async move { Json(spec) }))
        .merge(api)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    database: &'static str,
}

async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    match db::ping(&state.db).await {
        Ok(()) => (
            StatusCode::OK,
            Json(Health {
                status: "ok",
                database: "ok",
            }),
        ),
        Err(err) => {
            tracing::error!(error = %err, "database health check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(Health {
                    status: "degraded",
                    database: "unreachable",
                }),
            )
        }
    }
}
