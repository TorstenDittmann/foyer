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

/// The API and the worker both migrate on start, often at the same moment.
#[tokio::test]
async fn migrations_tolerate_two_processes_starting_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.db").to_string_lossy().into_owned();
    let open = || async {
        db::connect(&DatabaseConfig::Local { path: path.clone() })
            .await
            .unwrap()
    };
    // Separate `Database`s, like separate processes.
    let (a, b) = (open().await, open().await);
    let (ra, rb) = tokio::join!(db::migrate(&a), db::migrate(&b));
    ra.unwrap();
    rb.unwrap();
    db::migrate(&a).await.unwrap();
}
