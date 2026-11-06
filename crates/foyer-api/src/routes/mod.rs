use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::Serialize;
use tower_http::trace::TraceLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::ratelimit::{self, Bucket};
use crate::{AppState, db, openapi};

mod account;
mod auth;
mod builds;
mod developers;
mod downloads;
mod games;
mod review;

/// Applies one rate limit bucket to every route in `router`.
pub(crate) fn limited(router: OpenApiRouter<AppState>, bucket: Bucket) -> OpenApiRouter<AppState> {
    router.layer(axum::middleware::from_fn(move |request, next| {
        ratelimit::limit(bucket, request, next)
    }))
}

/// Every `/v1` route, with its OpenAPI description.
pub(crate) fn api_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(openapi::base())
        .merge(auth::routes())
        .merge(account::routes())
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
        // Route-level rate limit layers read the limiter from here.
        .layer(Extension(state.rate_limiter.clone()))
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
