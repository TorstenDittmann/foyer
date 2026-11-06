//! Tester keys and private channels.

mod common;

use axum::http::{Method, StatusCode};
use common::{Reply, TestApp};
use serde_json::{Value, json};

async fn get_with(app: &TestApp, path: &str, keys: &[&str]) -> Reply {
    app.call_with_keys(Method::GET, path, None, keys, None)
        .await
}

async fn post_with(app: &TestApp, path: &str, keys: &[&str], body: Value) -> Reply {
    app.call_with_keys(Method::POST, path, None, keys, Some(body))
        .await
}

async fn create_key(app: &TestApp, dev: &str, game: &str, body: Value) -> (String, Value) {
    let reply = app
        .post(&format!("/v1/games/{game}/tester-keys"), Some(dev), body)
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let secret = reply.body["secret"].as_str().unwrap().to_string();
    assert!(secret.starts_with("fyk_"));
    (secret, reply.body)
}

fn channel_names(reply: &Reply) -> Vec<String> {
    reply
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["channel"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_key_unlocks_an_unpublished_game_until_it_is_revoked() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    let build = app.push_build(&dev, "lantern", "windows", b"MZ v1").await;
    let build_id = build["id"].as_str().unwrap();

    // Nobody else can see a draft.
    assert_eq!(
        get_with(&app, "/v1/games/lantern", &[]).await.status,
        StatusCode::NOT_FOUND
    );

    let (key, created) = create_key(&app, &dev, "lantern", json!({ "label": "Sam" })).await;
    assert_eq!(created["key"]["label"], "Sam");
    assert!(created["key"]["channels"].as_array().unwrap().is_empty());
    assert!(created["key"]["expires_at"].is_string());
    assert!(
        created["link"]
            .as_str()
            .unwrap()
            .contains(&format!("/tester?key={key}"))
    );

    let redeemed = post_with(&app, "/v1/tester-keys/redeem", &[], json!({ "key": key })).await;
    assert_eq!(redeemed.status, StatusCode::OK, "{}", redeemed.body);
    assert_eq!(redeemed.body["game"], "lantern");
    assert_eq!(redeemed.body["title"], "Lantern Keeper");

    let game = get_with(&app, "/v1/games/lantern", &[&key]).await;
    assert_eq!(game.status, StatusCode::OK, "{}", game.body);
    assert_eq!(game.body["status"], "draft");
    let channels = get_with(&app, "/v1/games/lantern/channels", &[&key]).await;
    assert_eq!(channel_names(&channels), ["windows"]);
    let manifest = get_with(&app, &format!("/v1/builds/{build_id}/manifest"), &[&key]).await;
    assert_eq!(manifest.status, StatusCode::OK, "{}", manifest.body);
    let urls = post_with(
        &app,
        &format!("/v1/builds/{build_id}/downloads"),
        &[&key],
        json!({ "event": "install" }),
    )
    .await;
    assert_eq!(urls.status, StatusCode::OK, "{}", urls.body);
    let updates = post_with(
        &app,
        "/v1/updates",
        &[&key],
        json!({ "installed": [{ "game": "lantern", "channel": "windows", "build_id": build_id }] }),
    )
    .await;
    assert!(updates.body["unavailable"].as_array().unwrap().is_empty());

    // The developer sees when it was last used.
    let keys = app.get("/v1/games/lantern/tester-keys", Some(&dev)).await;
    assert_eq!(keys.status, StatusCode::OK);
    let listed = &keys.body.as_array().unwrap()[0];
    assert!(listed["last_used_at"].is_string(), "{listed}");
    assert!(listed.get("secret").is_none());
    assert!(key.starts_with(listed["prefix"].as_str().unwrap()));

    let id = listed["id"].as_str().unwrap();
    let revoked = app
        .call(
            Method::DELETE,
            &format!("/v1/games/lantern/tester-keys/{id}"),
            Some(&dev),
            None,
        )
        .await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);
    assert_eq!(
        get_with(&app, "/v1/games/lantern", &[&key]).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_with(&app, &format!("/v1/builds/{build_id}/manifest"), &[&key])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let redeemed = post_with(&app, "/v1/tester-keys/redeem", &[], json!({ "key": key })).await;
    assert_eq!(redeemed.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn keys_can_be_limited_to_channels_and_games() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    let beta = app.push_build(&dev, "lantern", "beta", b"MZ beta").await;
    let stable = app
        .push_build(&dev, "lantern", "stable", b"MZ stable")
        .await;

    let (key, _) = create_key(&app, &dev, "lantern", json!({ "channels": ["beta"] })).await;
    let channels = get_with(&app, "/v1/games/lantern/channels", &[&key]).await;
    assert_eq!(channel_names(&channels), ["beta"]);
    let beta_id = beta["id"].as_str().unwrap();
    let stable_id = stable["id"].as_str().unwrap();
    assert_eq!(
        get_with(&app, &format!("/v1/builds/{beta_id}"), &[&key])
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        get_with(&app, &format!("/v1/builds/{stable_id}"), &[&key])
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // A key for another game unlocks nothing here, and junk is ignored.
    let other = app.developer_with_game("other", "comet").await;
    let (comet_key, _) = create_key(&app, &other, "comet", json!({})).await;
    assert_eq!(
        get_with(
            &app,
            "/v1/games/lantern",
            &[&comet_key, "fyk_nonsense", "bogus"]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    // Several keys in one header each unlock their own game.
    for game in ["lantern", "comet"] {
        let reply = get_with(&app, &format!("/v1/games/{game}"), &[&comet_key, &key]).await;
        assert_eq!(reply.status, StatusCode::OK, "{game}");
    }

    // Only the developer manages keys.
    let reply = app
        .post("/v1/games/lantern/tester-keys", Some(&other), json!({}))
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let reply = app.get("/v1/games/lantern/tester-keys", Some(&other)).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    for bad in [
        json!({ "expires_in_days": 0 }),
        json!({ "expires_in_days": 400 }),
        json!({ "channels": ["Not A Channel"] }),
        json!({ "label": "x".repeat(81) }),
    ] {
        let reply = app
            .post("/v1/games/lantern/tester-keys", Some(&dev), bad.clone())
            .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn private_channels_are_hidden_from_players_without_a_key() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    app.push_build(&dev, "lantern", "windows", b"MZ 1.0").await;
    app.publish(&dev, "lantern").await;
    let beta = app
        .push_build(&dev, "lantern", "windows-beta", b"MZ 1.1 beta")
        .await;
    let beta_id = beta["id"].as_str().unwrap();

    // Only the owner can change a channel.
    let other = app.developer_with_game("other", "comet").await;
    let reply = app
        .patch(
            "/v1/games/lantern/channels/windows-beta",
            Some(&other),
            json!({ "private": true }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.body);
    let reply = app
        .patch(
            "/v1/games/lantern/channels/nope",
            Some(&dev),
            json!({ "private": true }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let reply = app
        .patch(
            "/v1/games/lantern/channels/windows-beta",
            Some(&dev),
            json!({ "private": true }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["private"], true);
    assert_eq!(reply.body["latest"]["id"], beta_id);

    // Players see the public channel only.
    let channels = get_with(&app, "/v1/games/lantern/channels", &[]).await;
    assert_eq!(channel_names(&channels), ["windows"]);
    assert_eq!(
        get_with(&app, &format!("/v1/builds/{beta_id}"), &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let updates = post_with(
        &app,
        "/v1/updates",
        &[],
        json!({ "installed": [{ "game": "lantern", "channel": "windows-beta", "build_id": beta_id }] }),
    )
    .await;
    assert_eq!(updates.body["unavailable"].as_array().unwrap().len(), 1);

    // The developer still sees it, marked private.
    let channels = app.get("/v1/games/lantern/channels", Some(&dev)).await;
    let beta_channel = channels
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["channel"] == "windows-beta")
        .unwrap()
        .clone();
    assert_eq!(beta_channel["private"], true);

    // A key for the private channel adds it on top of the public ones.
    let (key, _) = create_key(
        &app,
        &dev,
        "lantern",
        json!({ "channels": ["windows-beta"] }),
    )
    .await;
    let channels = get_with(&app, "/v1/games/lantern/channels", &[&key]).await;
    assert_eq!(channel_names(&channels), ["windows", "windows-beta"]);
    assert_eq!(
        get_with(&app, &format!("/v1/builds/{beta_id}"), &[&key])
            .await
            .status,
        StatusCode::OK
    );

    // Pushing with `private: false` makes it public again.
    let (manifest, chunks) = common::manifest_for(&[("game.exe", b"MZ 1.2 beta")]);
    let reply = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "windows-beta", "platform": "windows", "private": false, "manifest": manifest }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    drop(chunks);
    let channels = get_with(&app, "/v1/games/lantern/channels", &[]).await;
    assert_eq!(channel_names(&channels), ["windows", "windows-beta"]);
}

#[tokio::test]
async fn new_private_channels_never_show_up_in_platforms() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    let (manifest, chunks) = common::manifest_for(&[("game.exe", b"MZ secret")]);
    let reply = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "playtest", "platform": "windows", "private": true, "manifest": manifest }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    for upload in reply.body["uploads"].as_array().unwrap() {
        let hash = upload["hash"].as_str().unwrap();
        app.upload(upload["url"].as_str().unwrap(), &chunks[hash])
            .await;
    }
    let id = reply.body["build"]["id"].as_str().unwrap();
    let reply = app
        .post(
            &format!("/v1/builds/{id}/finalize"),
            Some(&dev),
            json!(null),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    app.run_jobs().await;

    let game = app.get("/v1/games/lantern", Some(&dev)).await;
    assert!(
        game.body["platforms"].as_array().unwrap().is_empty(),
        "{}",
        game.body
    );
    // So it can't be submitted on a private build alone.
    let reply = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.body);
    assert_eq!(reply.body["error"], "no_ready_build");
}
