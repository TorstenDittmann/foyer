//! Full-text search over published games.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

fn slugs(body: &serde_json::Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["slug"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn search_ranks_and_follows_edits() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    // A second game that mentions lanterns only in its description.
    let reply = app
        .post(
            "/v1/games",
            Some(&dev),
            json!({
                "slug": "tides",
                "title": "Tides of Glass",
                "tagline": "A sailing game",
                "description": "Sail at night with only a lantern to guide you.",
                "tags": ["sailing"],
            }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    for game in ["lantern", "tides"] {
        app.push_build(&dev, game, "windows", game.as_bytes()).await;
    }
    let admin = app.publish(&dev, "lantern").await;
    let sub = app
        .post("/v1/games/tides/submissions", Some(&dev), json!({}))
        .await;
    let id = sub.body["id"].as_str().unwrap();
    app.post(
        &format!("/v1/review/submissions/{id}/decision"),
        Some(&admin),
        json!({ "outcome": "approved" }),
    )
    .await;

    // Title match ranks above a description match.
    let found = app.get("/v1/games?q=lantern", None).await;
    assert_eq!(slugs(&found.body), ["lantern", "tides"]);
    // Prefix of the last word, any case, accents ignored.
    assert_eq!(
        slugs(&app.get("/v1/games?q=SAIL", None).await.body),
        ["tides"]
    );
    assert_eq!(
        slugs(&app.get("/v1/games?q=gl%C3%A1ss", None).await.body),
        ["tides"]
    );
    // Every word has to match.
    assert_eq!(
        slugs(&app.get("/v1/games?q=night%20glass", None).await.body),
        ["tides"]
    );
    // Tags and the developer's name are searched too, and tag filters combine.
    assert_eq!(
        slugs(&app.get("/v1/games?q=pixel", None).await.body),
        ["lantern"]
    );
    assert_eq!(
        slugs(&app.get("/v1/games?q=STUDIO", None).await.body).len(),
        2
    );
    assert_eq!(
        slugs(&app.get("/v1/games?q=lantern&tag=sailing", None).await.body),
        ["tides"]
    );
    // FTS syntax is treated as text, not as a query.
    for q in ["%22", "title%3Aglass", "NEAR(", "*", "lantern%20OR"] {
        let reply = app.get(&format!("/v1/games?q={q}"), None).await;
        assert_eq!(reply.status, StatusCode::OK, "{q}: {}", reply.body);
    }

    // Edits and developer renames are picked up.
    let rename = app
        .patch(
            "/v1/developers/studio",
            Some(&dev),
            json!({ "display_name": "Moonlit Works" }),
        )
        .await;
    assert_eq!(rename.status, StatusCode::OK, "{}", rename.body);
    assert_eq!(
        slugs(&app.get("/v1/games?q=moonlit", None).await.body).len(),
        2
    );
    let edit = app
        .patch(
            "/v1/games/tides",
            Some(&dev),
            json!({ "description": "Sail by starlight." }),
        )
        .await;
    assert_eq!(edit.status, StatusCode::OK, "{}", edit.body);
    assert_eq!(
        slugs(&app.get("/v1/games?q=lantern", None).await.body),
        ["lantern"]
    );
}
