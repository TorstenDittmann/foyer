use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use foyer_api::config::DatabaseConfig;
use foyer_api::{AppState, db, router};

#[tokio::test]
async fn health_reports_ok_with_in_memory_db() {
    let database = db::connect(&DatabaseConfig::Local {
        path: ":memory:".into(),
    })
    .await
    .unwrap();
    db::migrate(&database).await.unwrap();

    let response = router(AppState::new(database))
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}
