mod common;

use axum::http::{Method, StatusCode};
use common::{ADMIN_EMAIL, TestApp, manifest_for};
use serde_json::json;

#[tokio::test]
async fn register_login_me_and_tokens() {
    let app = TestApp::new().await;
    let session = app.register("ada", "Ada@Example.com").await;

    let me = app.get("/v1/me", Some(&session)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], "ada");
    assert_eq!(me.body["email"], "ada@example.com");
    assert_eq!(me.body["role"], "user");

    // Duplicate username and email.
    let dup = app
        .post(
            "/v1/auth/register",
            None,
            json!({ "username": "ada", "email": "other@example.com", "password": "correct horse battery" }),
        )
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    assert_eq!(dup.body["error"], "username_taken");
    let dup = app
        .post(
            "/v1/auth/register",
            None,
            json!({ "username": "ada2", "email": "ADA@example.com", "password": "correct horse battery" }),
        )
        .await;
    assert_eq!(dup.body["error"], "email_taken");

    // Short password.
    let bad = app
        .post(
            "/v1/auth/register",
            None,
            json!({ "username": "bob", "email": "bob@example.com", "password": "short" }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad.body["error"], "validation");

    // Login by username or email; wrong password and unknown user look alike.
    for login in ["ada", "ada@example.com"] {
        let reply = app
            .post(
                "/v1/auth/login",
                None,
                json!({ "login": login, "password": "correct horse battery" }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    }
    for (login, password) in [
        ("ada", "wrong password!"),
        ("nobody", "correct horse battery"),
    ] {
        let reply = app
            .post(
                "/v1/auth/login",
                None,
                json!({ "login": login, "password": password }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.body["error"], "unauthorized");
    }

    // API tokens: create, use, list, revoke.
    let created = app
        .post("/v1/auth/tokens", Some(&session), json!({ "name": "CI" }))
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let api_token = created.body["token"].as_str().unwrap().to_string();
    assert!(api_token.starts_with("fyp_"));
    assert_eq!(
        app.get("/v1/me", Some(&api_token)).await.status,
        StatusCode::OK
    );
    let list = app.get("/v1/auth/tokens", Some(&session)).await;
    assert_eq!(list.body.as_array().unwrap().len(), 1);
    assert!(list.body[0]["last_used_at"].is_string());
    assert!(list.body[0].get("token").is_none());
    let id = created.body["info"]["id"].as_str().unwrap();
    let revoked = app
        .call(
            Method::DELETE,
            &format!("/v1/auth/tokens/{id}"),
            Some(&session),
            None,
        )
        .await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get("/v1/me", Some(&api_token)).await.status,
        StatusCode::UNAUTHORIZED
    );

    // Logout revokes the session.
    let out = app
        .post("/v1/auth/logout", Some(&session), json!(null))
        .await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get("/v1/me", Some(&session)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.get("/v1/me", None).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn admins_assign_roles() {
    let app = TestApp::new().await;
    let admin = app.register("root", ADMIN_EMAIL).await;
    let user = app.register("rev", "rev@example.com").await;
    let user_id = app.get("/v1/me", Some(&user)).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();

    let path = format!("/v1/admin/users/{user_id}/role");
    let denied = app
        .call(
            Method::PUT,
            &path,
            Some(&user),
            Some(json!({ "role": "reviewer" })),
        )
        .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let ok = app
        .call(
            Method::PUT,
            &path,
            Some(&admin),
            Some(json!({ "role": "reviewer" })),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.body["role"], "reviewer");
    assert_eq!(
        app.get("/v1/me", Some(&user)).await.body["role"],
        "reviewer"
    );
}

#[tokio::test]
async fn developer_profiles_and_game_pages() {
    let app = TestApp::new().await;
    let dev = app.register("studio", "studio@example.com").await;

    // A game needs a developer profile.
    let no_profile = app
        .post(
            "/v1/games",
            Some(&dev),
            json!({ "slug": "nope", "title": "Nope" }),
        )
        .await;
    assert_eq!(no_profile.status, StatusCode::FORBIDDEN);
    assert_eq!(
        app.get("/v1/me/developer", Some(&dev)).await.status,
        StatusCode::NOT_FOUND
    );

    let bad_link = app
        .post(
            "/v1/developers",
            Some(&dev),
            json!({ "slug": "studio", "display_name": "Studio",
                    "support_links": [{ "kind": "patreon", "url": "javascript:alert(1)" }] }),
        )
        .await;
    assert_eq!(bad_link.status, StatusCode::BAD_REQUEST);

    let created = app
        .post(
            "/v1/developers",
            Some(&dev),
            json!({ "slug": "studio", "display_name": "Studio", "bio": "We make games.",
                    "support_links": [{ "kind": "patreon", "url": "https://patreon.com/studio" }] }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let again = app
        .post(
            "/v1/developers",
            Some(&dev),
            json!({ "slug": "studio2", "display_name": "Again" }),
        )
        .await;
    assert_eq!(again.body["error"], "developer_exists");

    let other = app.register("other", "other@example.com").await;
    let taken = app
        .post(
            "/v1/developers",
            Some(&other),
            json!({ "slug": "studio", "display_name": "Copy" }),
        )
        .await;
    assert_eq!(taken.body["error"], "slug_taken");
    let not_mine = app
        .patch(
            "/v1/developers/studio",
            Some(&other),
            json!({ "bio": "hijacked" }),
        )
        .await;
    assert_eq!(not_mine.status, StatusCode::FORBIDDEN);

    let updated = app
        .patch(
            "/v1/developers/studio",
            Some(&dev),
            json!({ "bio": "Small studio.", "website_url": "https://studio.example" }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(updated.body["bio"], "Small studio.");
    assert_eq!(
        app.get("/v1/developers/studio", None).await.body["website_url"],
        "https://studio.example"
    );

    let game = app
        .post(
            "/v1/games",
            Some(&dev),
            json!({ "slug": "lantern", "title": "Lantern", "tags": ["Puzzle", "puzzle", "pixel-art"] }),
        )
        .await;
    assert_eq!(game.status, StatusCode::CREATED, "{}", game.body);
    assert_eq!(game.body["status"], "draft");
    assert_eq!(game.body["tags"], json!(["puzzle", "pixel-art"]));
    assert_eq!(game.body["developer"]["slug"], "studio");

    // Drafts are private.
    assert_eq!(
        app.get("/v1/games/lantern", None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get("/v1/games/lantern", Some(&other)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get("/v1/games/lantern", Some(&dev)).await.status,
        StatusCode::OK
    );
    assert_eq!(app.get("/v1/games", None).await.body["items"], json!([]));
    assert_eq!(
        app.get("/v1/me/games", Some(&dev))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let edited = app
        .patch(
            "/v1/games/lantern",
            Some(&dev),
            json!({ "tagline": "Light the way", "ai_disclosure": "" }),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK);
    assert_eq!(edited.body["tagline"], "Light the way");
    assert!(edited.body["ai_disclosure"].is_null());
    let hijack = app
        .patch(
            "/v1/games/lantern",
            Some(&other),
            json!({ "title": "Mine" }),
        )
        .await;
    assert_eq!(hijack.status, StatusCode::NOT_FOUND);

    // Media: a capsule replaces the previous one.
    for _ in 0..2 {
        let media = app
            .post(
                "/v1/games/lantern/media",
                Some(&dev),
                json!({ "kind": "capsule", "content_type": "image/png" }),
            )
            .await;
        assert_eq!(media.status, StatusCode::CREATED, "{}", media.body);
        assert!(
            media.body["media"]["url"]
                .as_str()
                .unwrap()
                .starts_with("https://media.foyer.test/games/")
        );
        assert!(
            media.body["upload_url"]
                .as_str()
                .unwrap()
                .starts_with(&app.s3_base)
        );
    }
    let shot = app
        .post(
            "/v1/games/lantern/media",
            Some(&dev),
            json!({ "kind": "screenshot", "content_type": "image/webp" }),
        )
        .await;
    assert_eq!(shot.status, StatusCode::CREATED);
    let svg = app
        .post(
            "/v1/games/lantern/media",
            Some(&dev),
            json!({ "kind": "screenshot", "content_type": "image/svg+xml" }),
        )
        .await;
    assert_eq!(svg.status, StatusCode::BAD_REQUEST);
    let media = app.get("/v1/games/lantern", Some(&dev)).await.body["media"].clone();
    assert_eq!(media.as_array().unwrap().len(), 2);
    let shot_id = shot.body["media"]["id"].as_str().unwrap();
    let deleted = app
        .call(
            Method::DELETE,
            &format!("/v1/games/lantern/media/{shot_id}"),
            Some(&dev),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn build_push_resume_finalize_and_download() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;

    let (manifest, chunks) = manifest_for(&[
        ("game.exe", b"binary v1"),
        ("data/level1.dat", b"level one"),
    ]);
    let created = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "windows", "platform": "windows", "manifest": manifest }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let build_id = created.body["build"]["id"].as_str().unwrap().to_string();
    assert_eq!(created.body["build"]["state"], "uploading");
    assert_eq!(created.body["build"]["chunk_count"], 2);
    let uploads = created.body["uploads"].as_array().unwrap().clone();
    assert_eq!(uploads.len(), 2);

    // Upload only one chunk; finalize lists the other as missing.
    let first = &uploads[0];
    app.upload(
        first["url"].as_str().unwrap(),
        &chunks[first["hash"].as_str().unwrap()],
    )
    .await;
    let early = app
        .post(
            &format!("/v1/builds/{build_id}/finalize"),
            Some(&dev),
            json!(null),
        )
        .await;
    assert_eq!(early.status, StatusCode::CONFLICT);
    assert_eq!(early.body["error"], "missing_chunks");
    assert_eq!(early.body["missing"], json!([uploads[1]["hash"]]));

    // Resume: fresh URLs only for what is still missing.
    let resume = app
        .post(
            &format!("/v1/builds/{build_id}/uploads"),
            Some(&dev),
            json!(null),
        )
        .await;
    assert_eq!(resume.status, StatusCode::OK);
    let remaining = resume.body["uploads"].as_array().unwrap();
    assert_eq!(remaining.len(), 1);
    app.upload(
        remaining[0]["url"].as_str().unwrap(),
        &chunks[remaining[0]["hash"].as_str().unwrap()],
    )
    .await;

    let done = app
        .post(
            &format!("/v1/builds/{build_id}/finalize"),
            Some(&dev),
            json!(null),
        )
        .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["state"], "processing");
    // Nothing is served until the worker has scanned it.
    let channels = app.get("/v1/games/lantern/channels", Some(&dev)).await;
    assert!(channels.body[0]["latest"].is_null(), "{}", channels.body);
    assert_eq!(app.run_jobs().await, 1);
    let ready = app.get(&format!("/v1/builds/{build_id}"), Some(&dev)).await;
    assert_eq!(ready.body["state"], "ready");
    assert_eq!(ready.body["scan"]["status"], "skipped");
    // Finalize is idempotent.
    let again = app
        .post(
            &format!("/v1/builds/{build_id}/finalize"),
            Some(&dev),
            json!(null),
        )
        .await;
    assert_eq!(again.status, StatusCode::OK);
    let late = app
        .post(
            &format!("/v1/builds/{build_id}/uploads"),
            Some(&dev),
            json!(null),
        )
        .await;
    assert_eq!(late.body["error"], "build_not_uploading");

    // A second push that shares a chunk only uploads the new one.
    let (manifest2, _) = manifest_for(&[
        ("game.exe", b"binary v2"),
        ("data/level1.dat", b"level one"),
    ]);
    let second = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "windows", "platform": "windows", "version": "1.1", "manifest": manifest2 }),
        )
        .await;
    assert_eq!(second.body["uploads"].as_array().unwrap().len(), 1);

    // The channel is tied to its platform.
    let (manifest3, _) = manifest_for(&[("game", b"linux")]);
    let mismatch = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "windows", "platform": "linux", "manifest": manifest3 }),
        )
        .await;
    assert_eq!(mismatch.body["error"], "channel_platform_mismatch");

    // Unsafe paths are rejected.
    let (evil, _) = manifest_for(&[("../escape.exe", b"x")]);
    let evil = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "windows", "platform": "windows", "manifest": evil }),
        )
        .await;
    assert_eq!(evil.status, StatusCode::BAD_REQUEST);

    // Channels, builds and the game's platforms.
    let channels = app.get("/v1/games/lantern/channels", Some(&dev)).await;
    assert_eq!(channels.body[0]["channel"], "windows");
    assert_eq!(channels.body[0]["latest"]["id"], build_id);
    assert_eq!(
        app.get("/v1/games/lantern/builds", Some(&dev))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        app.get("/v1/games/lantern", Some(&dev)).await.body["platforms"],
        json!(["windows"])
    );

    // The draft's builds are private; the developer can download.
    assert_eq!(
        app.get(&format!("/v1/builds/{build_id}"), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let manifest = app
        .get(&format!("/v1/builds/{build_id}/manifest"), Some(&dev))
        .await;
    assert_eq!(manifest.body["files"]["game.exe"]["size"], 9);
    let downloads = app
        .post(
            &format!("/v1/builds/{build_id}/downloads"),
            Some(&dev),
            json!({}),
        )
        .await;
    assert_eq!(downloads.status, StatusCode::OK, "{}", downloads.body);
    let downloads = downloads.body["downloads"].as_array().unwrap();
    assert_eq!(downloads.len(), 2);
    let body = reqwest::get(downloads[0]["url"].as_str().unwrap())
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(
        body.as_ref(),
        chunks[downloads[0]["hash"].as_str().unwrap()].as_slice()
    );
    let unknown = app
        .post(
            &format!("/v1/builds/{build_id}/downloads"),
            Some(&dev),
            json!({ "hashes": ["00".repeat(32)] }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);

    // Other developers can't push to this game.
    let other = app.developer_with_game("rival", "rival-game").await;
    let (m, _) = manifest_for(&[("x", b"x")]);
    let denied = app
        .post(
            "/v1/games/lantern/builds",
            Some(&other),
            json!({ "channel": "windows", "platform": "windows", "manifest": m }),
        )
        .await;
    assert_eq!(denied.status, StatusCode::NOT_FOUND);
    // Chunks are stored per game.
    assert!(
        app.objects
            .lock()
            .unwrap()
            .keys()
            .all(|k| k.starts_with("foyer/games/"))
    );
}

#[tokio::test]
async fn submission_review_and_publication() {
    let app = TestApp::new().await;
    let admin = app.register("root", ADMIN_EMAIL).await;
    let dev = app.developer_with_game("studio", "lantern").await;
    let player = app.register("player", "player@example.com").await;

    // Needs a ready build first.
    let early = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    assert_eq!(early.body["error"], "no_ready_build");
    app.push_build(&dev, "lantern", "windows", b"game bytes")
        .await;

    let submitted = app
        .post(
            "/v1/games/lantern/submissions",
            Some(&dev),
            json!({ "developer_notes": "Plays in about an hour." }),
        )
        .await;
    assert_eq!(submitted.status, StatusCode::CREATED, "{}", submitted.body);
    assert_eq!(submitted.body["status"], "pending");
    let sub_id = submitted.body["id"].as_str().unwrap().to_string();
    let twice = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    assert_eq!(twice.body["error"], "not_submittable");
    // Page is locked while in review.
    let locked = app
        .patch("/v1/games/lantern", Some(&dev), json!({ "title": "x" }))
        .await;
    assert_eq!(locked.body["error"], "game_in_review");

    // Only staff see the queue.
    assert_eq!(
        app.get("/v1/review/queue", Some(&player)).await.status,
        StatusCode::FORBIDDEN
    );
    let queue = app.get("/v1/review/queue", Some(&admin)).await;
    assert_eq!(queue.status, StatusCode::OK);
    assert_eq!(queue.body["items"][0]["submission"]["id"], sub_id);
    assert_eq!(queue.body["items"][0]["game"]["slug"], "lantern");

    let claimed = app
        .post(
            &format!("/v1/review/submissions/{sub_id}/claim"),
            Some(&admin),
            json!(null),
        )
        .await;
    assert_eq!(claimed.status, StatusCode::OK);
    assert_eq!(claimed.body["status"], "in_review");
    assert_eq!(claimed.body["reviewer"]["username"], "root");

    // A rejection must cite a rule; change requests need notes.
    let no_rule = app
        .post(
            &format!("/v1/review/submissions/{sub_id}/decision"),
            Some(&admin),
            json!({ "outcome": "rejected", "notes": "no" }),
        )
        .await;
    assert_eq!(no_rule.status, StatusCode::BAD_REQUEST);
    let changes = app
        .post(
            &format!("/v1/review/submissions/{sub_id}/decision"),
            Some(&admin),
            json!({ "outcome": "changes_requested", "notes": "Add three screenshots.", "rule": "quality_page" }),
        )
        .await;
    assert_eq!(changes.status, StatusCode::OK, "{}", changes.body);
    assert_eq!(changes.body["status"], "changes_requested");
    assert_eq!(changes.body["rule"], "quality_page");
    assert_eq!(
        app.get("/v1/games/lantern", Some(&dev)).await.body["status"],
        "changes_requested"
    );
    let decided = app
        .post(
            &format!("/v1/review/submissions/{sub_id}/decision"),
            Some(&admin),
            json!({ "outcome": "approved" }),
        )
        .await;
    assert_eq!(decided.body["error"], "already_decided");

    // Developer sees the feedback, edits and resubmits; reviewer approves.
    let history = app.get("/v1/games/lantern/submissions", Some(&dev)).await;
    assert_eq!(history.body[0]["decision_notes"], "Add three screenshots.");
    assert_eq!(
        app.patch(
            "/v1/games/lantern",
            Some(&dev),
            json!({ "description": "Now with screenshots." })
        )
        .await
        .status,
        StatusCode::OK
    );
    let resubmitted = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    assert_eq!(resubmitted.status, StatusCode::CREATED);
    let sub2 = resubmitted.body["id"].as_str().unwrap().to_string();
    let approved = app
        .post(
            &format!("/v1/review/submissions/{sub2}/decision"),
            Some(&admin),
            json!({ "outcome": "approved" }),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    assert_eq!(approved.body["status"], "approved");

    // Now public: game page, catalog, tag and search filters, developer page,
    // channels and downloads.
    let page = app.get("/v1/games/lantern", None).await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(page.body["status"], "published");
    assert!(page.body["published_at"].is_string());
    let catalog = app.get("/v1/games", None).await;
    assert_eq!(catalog.body["items"][0]["slug"], "lantern");
    assert!(catalog.body["next_cursor"].is_null());
    assert_eq!(
        app.get("/v1/games?tag=puzzle", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        app.get("/v1/games?tag=shooter", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        app.get("/v1/games?q=LANTERN", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        app.get("/v1/games?q=100%25", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        app.get("/v1/developers/studio/games", None).await.body["items"][0]["slug"],
        "lantern"
    );
    let channels = app.get("/v1/games/lantern/channels", None).await;
    let build_id = channels.body[0]["latest"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let downloads = app
        .post(&format!("/v1/builds/{build_id}/downloads"), None, json!({}))
        .await;
    assert_eq!(downloads.status, StatusCode::OK);

    // Staff can delist; the reason lands in the history.
    let delisted = app
        .post(
            "/v1/review/games/lantern/delist",
            Some(&admin),
            json!({ "rule": "sexual_content", "notes": "Adult patch linked from the page." }),
        )
        .await;
    assert_eq!(delisted.status, StatusCode::OK, "{}", delisted.body);
    assert_eq!(delisted.body["status"], "delisted");
    assert_eq!(
        app.get("/v1/games/lantern", None).await.status,
        StatusCode::NOT_FOUND
    );
    let history = app.get("/v1/games/lantern/submissions", Some(&dev)).await;
    assert_eq!(history.body[0]["rule"], "sexual_content");
}

#[tokio::test]
async fn reviewers_cannot_review_their_own_games() {
    let app = TestApp::new().await;
    let admin = app.register("root", ADMIN_EMAIL).await;
    app.post(
        "/v1/developers",
        Some(&admin),
        json!({ "slug": "root", "display_name": "Root" }),
    )
    .await;
    app.post(
        "/v1/games",
        Some(&admin),
        json!({ "slug": "own", "title": "Own", "description": "Mine." }),
    )
    .await;
    app.push_build(&admin, "own", "linux", b"bytes").await;
    let sub = app
        .post("/v1/games/own/submissions", Some(&admin), json!({}))
        .await;
    assert_eq!(sub.status, StatusCode::CREATED, "{}", sub.body);
    let id = sub.body["id"].as_str().unwrap();
    let self_review = app
        .post(
            &format!("/v1/review/submissions/{id}/decision"),
            Some(&admin),
            json!({ "outcome": "approved" }),
        )
        .await;
    assert_eq!(self_review.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn storage_endpoints_report_503_without_storage() {
    use axum::body::Body;
    use axum::http::Request;
    use foyer_api::config::DatabaseConfig;
    use foyer_api::{AppState, db, router};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let dir = tempfile::tempdir().unwrap();
    let database = db::connect(&DatabaseConfig::Local {
        path: dir.path().join("t.db").to_string_lossy().into_owned(),
    })
    .await
    .unwrap();
    db::migrate(&database).await.unwrap();
    let app = router(AppState::new(database));

    let register = Request::post("/v1/auth/register")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "username": "dev", "email": "dev@example.com", "password": "correct horse battery" })
                .to_string(),
        ))
        .unwrap();
    let body = app.clone().oneshot(register).await.unwrap().into_body();
    let auth: serde_json::Value =
        serde_json::from_slice(&body.collect().await.unwrap().to_bytes()).unwrap();
    let token = auth["token"].as_str().unwrap();

    let finalize = Request::post("/v1/builds/00000000-0000-0000-0000-000000000000/finalize")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(finalize).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "storage_unavailable");
}

#[tokio::test]
async fn claims_can_be_released_or_taken_over() {
    let app = TestApp::new().await;
    let admin = app.register("root", ADMIN_EMAIL).await;
    let reviewer = app.register("rev", "rev@example.com").await;
    let rev_id = app.get("/v1/me", Some(&reviewer)).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();
    app.call(
        Method::PUT,
        &format!("/v1/admin/users/{rev_id}/role"),
        Some(&admin),
        Some(json!({ "role": "reviewer" })),
    )
    .await;
    let second = app.register("rev2", "rev2@example.com").await;
    let second_id = app.get("/v1/me", Some(&second)).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();
    app.call(
        Method::PUT,
        &format!("/v1/admin/users/{second_id}/role"),
        Some(&admin),
        Some(json!({ "role": "reviewer" })),
    )
    .await;

    let dev = app.developer_with_game("studio", "lantern").await;
    app.push_build(&dev, "lantern", "windows", b"v1").await;
    let sub = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    let id = sub.body["id"].as_str().unwrap().to_string();

    // No pushes while in review.
    let (manifest, _) = manifest_for(&[("game.exe", b"v2")]);
    let push = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "windows", "platform": "windows", "manifest": manifest }),
        )
        .await;
    assert_eq!(push.body["error"], "game_in_review");

    let claim = format!("/v1/review/submissions/{id}/claim");
    let release = format!("/v1/review/submissions/{id}/release");
    assert_eq!(
        app.post(&claim, Some(&reviewer), json!(null)).await.status,
        StatusCode::OK
    );
    // Another reviewer can neither claim nor release it.
    assert_eq!(
        app.post(&claim, Some(&second), json!(null)).await.body["error"],
        "already_claimed"
    );
    assert_eq!(
        app.post(&release, Some(&second), json!(null)).await.status,
        StatusCode::FORBIDDEN
    );
    // The claimant releases; it is back in the queue.
    let released = app.post(&release, Some(&reviewer), json!(null)).await;
    assert_eq!(released.body["status"], "pending");
    assert!(released.body["reviewer"].is_null());
    // An admin can take over someone else's claim and decide.
    assert_eq!(
        app.post(&claim, Some(&second), json!(null)).await.status,
        StatusCode::OK
    );
    let taken = app.post(&claim, Some(&admin), json!(null)).await;
    assert_eq!(taken.body["reviewer"]["username"], "root");
    let decided = app
        .post(
            &format!("/v1/review/submissions/{id}/decision"),
            Some(&admin),
            json!({ "outcome": "approved" }),
        )
        .await;
    assert_eq!(decided.body["status"], "approved");
}
