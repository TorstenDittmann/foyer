//! Game page design: colours, art and blocks.

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use serde_json::json;

#[tokio::test]
async fn developers_style_their_game_pages() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("dev", "lantern").await;

    // A new game has the standard look.
    let game = app.get("/v1/games/lantern", Some(&dev)).await.body;
    assert_eq!(game["page"]["title_style"], "standard");
    assert!(game["page"].get("blocks").is_none());

    let art = app
        .post(
            "/v1/games/lantern/media",
            Some(&dev),
            json!({ "kind": "artwork", "content_type": "image/png" }),
        )
        .await;
    assert_eq!(art.status, StatusCode::CREATED, "{}", art.body);
    let art_id = art.body["media"]["id"].as_str().unwrap().to_string();
    let shot = app
        .post(
            "/v1/games/lantern/media",
            Some(&dev),
            json!({ "kind": "screenshot", "content_type": "image/png" }),
        )
        .await;
    let shot_id = shot.body["media"]["id"].as_str().unwrap().to_string();

    let page = json!({
        "background": "#0B1A2E",
        "accent": "#ffb000",
        "hero": shot_id,
        "logo": art_id,
        "title_style": "uppercase",
        "blocks": [
            { "type": "trailer", "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ" },
            { "type": "about" },
            { "type": "features", "heading": "Features", "items": ["Hand-drawn", "No timers"] },
            { "type": "image", "media": art_id, "caption": "The keeper" },
            { "type": "quote", "text": "A small gem.", "source": "A player" },
            { "type": "screenshots" },
        ],
    });
    let saved = app
        .patch("/v1/games/lantern", Some(&dev), json!({ "page": page }))
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    // Colours are stored lowercase.
    assert_eq!(saved.body["page"]["background"], "#0b1a2e");
    assert_eq!(saved.body["page"]["blocks"].as_array().unwrap().len(), 6);
    // Other edits leave the design alone.
    let renamed = app
        .patch(
            "/v1/games/lantern",
            Some(&dev),
            json!({ "title": "Lantern" }),
        )
        .await;
    assert_eq!(renamed.body["page"]["accent"], "#ffb000");

    // Unreadable colours, foreign images and unknown video hosts are refused.
    for (bad, field) in [
        (json!({ "background": "#777777" }), "page.background"),
        (
            json!({ "background": "#101010", "accent": "#1c1c1c" }),
            "page.accent",
        ),
        (json!({ "accent": "green" }), "page.accent"),
        (json!({ "logo": shot_id }), "page.logo"),
        (
            json!({ "hero": "00000000-0000-7000-8000-000000000000" }),
            "page.hero",
        ),
        (
            json!({ "blocks": [{ "type": "trailer", "url": "https://evil.example/v.mp4" }] }),
            "page.blocks[0].url",
        ),
        (
            json!({ "blocks": [{ "type": "about" }, { "type": "about" }] }),
            "page.blocks",
        ),
    ] {
        let reply = app
            .patch("/v1/games/lantern", Some(&dev), json!({ "page": bad }))
            .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{bad}");
        let message = reply.body["message"].as_str().unwrap();
        assert!(message.starts_with(field), "{bad}: {message}");
    }

    // Deleting an image takes it out of the design.
    let deleted = app
        .call(
            Method::DELETE,
            &format!("/v1/games/lantern/media/{art_id}"),
            Some(&dev),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let game = app.get("/v1/games/lantern", Some(&dev)).await.body;
    assert!(game["page"].get("logo").is_none());
    assert_eq!(game["page"]["hero"], shot_id.as_str());
    let kinds: Vec<&str> = game["page"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["trailer", "about", "features", "quote", "screenshots"]
    );

    // Only the developer can change it.
    let other = app.register("other", "other@example.com").await;
    let reply = app
        .patch(
            "/v1/games/lantern",
            Some(&other),
            json!({ "page": { "accent": "#ff0000" } }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn artwork_is_capped() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("dev", "lantern").await;
    for _ in 0..12 {
        let reply = app
            .post(
                "/v1/games/lantern/media",
                Some(&dev),
                json!({ "kind": "artwork", "content_type": "image/webp" }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED);
    }
    let reply = app
        .post(
            "/v1/games/lantern/media",
            Some(&dev),
            json!({ "kind": "artwork", "content_type": "image/webp" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.body["error"], "too_much_artwork");
}
