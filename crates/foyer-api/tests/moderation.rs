//! Player reports, delisting, appeals and strikes.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::TestApp;
use serde_json::{Value, json};

/// A published game `lantern` by `studio`. Returns (developer, admin).
async fn published(app: &TestApp) -> (String, String) {
    let dev = app.developer_with_game("studio", "lantern").await;
    app.push_build(&dev, "lantern", "windows", b"game bytes")
        .await;
    let admin = app.publish(&dev, "lantern").await;
    (dev, admin)
}

/// Registers a user and makes them a reviewer.
async fn reviewer(app: &TestApp, admin: &str, name: &str) -> String {
    let token = app.register(name, &format!("{name}@example.com")).await;
    let id = app.get("/v1/me", Some(&token)).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let reply = app
        .call(
            Method::PUT,
            &format!("/v1/admin/users/{id}/role"),
            Some(admin),
            Some(json!({ "role": "reviewer" })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    token
}

/// An anonymous report from `ip` (the router trusts one proxy hop in these
/// tests only when configured; otherwise every request shares one address).
async fn report_from(app: &TestApp, ip: &str, body: Value) -> StatusCode {
    let request = Request::post("/v1/games/lantern/reports")
        .header("content-type", "application/json")
        .header("x-forwarded-for", ip)
        .body(Body::from(body.to_string()))
        .unwrap();
    tower::ServiceExt::oneshot(app.router.clone(), request)
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn reports_reach_staff_and_delisting_closes_them() {
    let app = TestApp::new().await;
    let (dev, admin) = published(&app).await;
    let player = app.register("player", "player@example.com").await;

    // Signed in or not; "other" needs details.
    let reply = app
        .post(
            "/v1/games/lantern/reports",
            Some(&player),
            json!({ "reason": "malware", "details": "Defender flags the exe." }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.body);
    let reply = app
        .post(
            "/v1/games/lantern/reports",
            None,
            json!({ "reason": "other" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    let reply = app
        .post(
            "/v1/games/lantern/reports",
            None,
            json!({ "reason": "broken" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    // A repeat while the first is open is accepted and ignored.
    let reply = app
        .post(
            "/v1/games/lantern/reports",
            None,
            json!({ "reason": "spam" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    // Developers can't report their own game; unknown games are 404.
    let reply = app
        .post(
            "/v1/games/lantern/reports",
            Some(&dev),
            json!({ "reason": "spam" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    let reply = app
        .post("/v1/games/nope/reports", None, json!({ "reason": "spam" }))
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    // Staff only.
    assert_eq!(
        app.get("/v1/review/reports", Some(&player)).await.status,
        StatusCode::FORBIDDEN
    );
    let queue = app.get("/v1/review/reports", Some(&admin)).await;
    assert_eq!(queue.status, StatusCode::OK, "{}", queue.body);
    let item = &queue.body["items"][0];
    assert_eq!(item["game"]["slug"], "lantern");
    assert_eq!(item["open_reports"], 2);
    assert_eq!(item["developer_strikes"], 0);
    let reports = app
        .get("/v1/review/games/lantern/reports?status=open", Some(&admin))
        .await;
    let reports = reports.body.as_array().unwrap().clone();
    assert_eq!(reports.len(), 2);
    let anon = reports.iter().find(|r| r["reason"] == "broken").unwrap();
    assert!(anon["reporter"].is_null());
    let signed = reports.iter().find(|r| r["reason"] == "malware").unwrap();
    assert_eq!(signed["reporter"]["username"], "player");
    assert_eq!(signed["details"], "Defender flags the exe.");

    // Delisting with a strike closes them as actioned and emails the
    // developer.
    let reply = app
        .post(
            "/v1/review/games/lantern/delist",
            Some(&admin),
            json!({ "rule": "quality_runs", "notes": "Crashes on start for everyone.", "strike": true }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let queue = app.get("/v1/review/reports", Some(&admin)).await;
    assert_eq!(queue.body["items"].as_array().unwrap().len(), 0);
    let reports = app
        .get("/v1/review/games/lantern/reports", Some(&admin))
        .await;
    assert!(
        reports
            .body
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "actioned"
                && r["resolution_notes"] == "Crashes on start for everyone.")
    );
    let emails = app.queued_emails("studio@example.com").await;
    assert!(
        emails
            .iter()
            .any(|e| e["subject"] == "Lantern Keeper was delisted from Foyer"),
        "{emails:?}"
    );
    let strikes = app.get("/v1/developers/studio/strikes", Some(&dev)).await;
    assert_eq!(strikes.status, StatusCode::OK);
    assert_eq!(strikes.body[0]["rule"], "quality_runs");
    assert_eq!(strikes.body[0]["game"], "lantern");
    // Strikes are private to the developer and staff.
    assert_eq!(
        app.get("/v1/developers/studio/strikes", Some(&player))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    // Delisted games can't be reported.
    let reply = app
        .post(
            "/v1/games/lantern/reports",
            Some(&player),
            json!({ "reason": "spam" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn dismissing_reports_keeps_the_game_listed() {
    let app = TestApp::new().await;
    let (_dev, admin) = published(&app).await;
    let player = app.register("player", "player@example.com").await;
    app.post(
        "/v1/games/lantern/reports",
        Some(&player),
        json!({ "reason": "misleading" }),
    )
    .await;
    let reply = app
        .post(
            "/v1/review/games/lantern/reports/dismiss",
            Some(&admin),
            json!({ "notes": "Descriptors are accurate." }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["resolved"], 1);
    assert_eq!(
        app.get("/v1/games/lantern", None).await.body["status"],
        "published"
    );
    // The player can report again once their report is closed.
    let reply = app
        .post(
            "/v1/games/lantern/reports",
            Some(&player),
            json!({ "reason": "misleading" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let queue = app.get("/v1/review/reports", Some(&admin)).await;
    assert_eq!(queue.body["items"][0]["open_reports"], 1);
}

#[tokio::test]
async fn reports_are_rate_limited_and_deduplicated_per_address() {
    use foyer_api::ratelimit::{ClientIpConfig, Limit, RateLimits};
    let app = TestApp::new().await;
    let (_dev, admin) = published(&app).await;
    let loose = Limit {
        per_minute: 1000,
        burst: 1000,
    };
    let app = app.with_rate_limits(
        RateLimits {
            auth: loose,
            upload: loose,
            download: loose,
            report: Limit {
                per_minute: 1,
                burst: 2,
            },
        },
        ClientIpConfig {
            proxy_hops: 1,
            internal_key: None,
        },
    );
    let body = json!({ "reason": "spam" });
    assert_eq!(
        report_from(&app, "1.1.1.1", body.clone()).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        report_from(&app, "1.1.1.1", body.clone()).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        report_from(&app, "1.1.1.1", body.clone()).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        report_from(&app, "2.2.2.2", body.clone()).await,
        StatusCode::NO_CONTENT
    );
    // Two addresses, two reports; the repeat from 1.1.1.1 was dropped.
    let queue = app.get("/v1/review/reports", Some(&admin)).await;
    assert_eq!(queue.body["items"][0]["open_reports"], 2);
}

#[tokio::test]
async fn appeals_go_to_another_reviewer() {
    let app = TestApp::new().await;
    let admin = app.register("root", common::ADMIN_EMAIL).await;
    let alice = reviewer(&app, &admin, "alice").await;
    let bob = reviewer(&app, &admin, "bob").await;
    let dev = app.developer_with_game("studio", "lantern").await;
    app.push_build(&dev, "lantern", "windows", b"game bytes")
        .await;

    // Nothing to appeal yet.
    let reply = app
        .post(
            "/v1/games/lantern/appeals",
            Some(&dev),
            json!({ "statement": "Please look again." }),
        )
        .await;
    assert_eq!(reply.body["error"], "not_appealable");

    // Alice rejects with a strike.
    let sub = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    let sub_id = sub.body["id"].as_str().unwrap().to_string();
    let reply = app
        .post(
            &format!("/v1/review/submissions/{sub_id}/decision"),
            Some(&alice),
            json!({ "outcome": "approved", "strike": true }),
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::BAD_REQUEST,
        "strike needs a rejection"
    );
    let reply = app
        .post(
            &format!("/v1/review/submissions/{sub_id}/decision"),
            Some(&alice),
            json!({ "outcome": "rejected", "rule": "asset_flip", "notes": "Store asset pack.", "strike": true }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let queue = app.get("/v1/review/queue", Some(&admin)).await;
    assert_eq!(queue.body["items"].as_array().unwrap().len(), 0);
    let strikes = app.get("/v1/developers/studio/strikes", Some(&bob)).await;
    assert_eq!(strikes.body.as_array().unwrap().len(), 1);
    assert!(strikes.body[0]["revoked_at"].is_null());

    // Only the owner appeals, with a statement, once.
    let reply = app
        .post(
            "/v1/games/lantern/appeals",
            Some(&dev),
            json!({ "statement": " " }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    let reply = app
        .post(
            "/v1/games/lantern/appeals",
            Some(&bob),
            json!({ "statement": "Mine now." }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    let appeal = app
        .post(
            "/v1/games/lantern/appeals",
            Some(&dev),
            json!({ "statement": "The art is ours; here are the source files." }),
        )
        .await;
    assert_eq!(appeal.status, StatusCode::CREATED, "{}", appeal.body);
    assert_eq!(appeal.body["status"], "pending");
    assert_eq!(appeal.body["submission"]["id"], sub_id);
    let appeal_id = appeal.body["id"].as_str().unwrap().to_string();
    let again = app
        .post(
            "/v1/games/lantern/appeals",
            Some(&dev),
            json!({ "statement": "Again." }),
        )
        .await;
    assert_eq!(again.body["error"], "already_appealed");

    // In the staff queue, with the developer's strike count.
    let queue = app.get("/v1/review/appeals", Some(&bob)).await;
    assert_eq!(queue.status, StatusCode::OK, "{}", queue.body);
    assert_eq!(queue.body["items"][0]["appeal"]["id"], appeal_id);
    assert_eq!(queue.body["items"][0]["developer_strikes"], 1);

    // Alice made the decision, so she can't judge the appeal.
    let path = format!("/v1/review/appeals/{appeal_id}/decision");
    let reply = app
        .post(
            &path,
            Some(&alice),
            json!({ "outcome": "upheld", "notes": "Still a flip." }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    let reply = app
        .post(&path, Some(&bob), json!({ "outcome": "overturned" }))
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "notes are required");
    let reply = app
        .post(
            &path,
            Some(&bob),
            json!({ "outcome": "overturned", "notes": "Source files check out." }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["status"], "overturned");
    assert_eq!(reply.body["reviewer"]["username"], "bob");
    let reply = app
        .post(
            &path,
            Some(&bob),
            json!({ "outcome": "upheld", "notes": "Changed my mind." }),
        )
        .await;
    assert_eq!(reply.body["error"], "already_decided");

    // Published, strike revoked, developer told.
    let game = app.get("/v1/games/lantern", None).await;
    assert_eq!(game.body["status"], "published");
    assert!(game.body["published_at"].is_string());
    let strikes = app.get("/v1/developers/studio/strikes", Some(&dev)).await;
    assert!(strikes.body[0]["revoked_at"].is_string());
    assert_eq!(strikes.body[0]["revoke_reason"], "Appeal overturned");
    let appeals = app.get("/v1/games/lantern/appeals", Some(&dev)).await;
    assert_eq!(appeals.body[0]["decision_notes"], "Source files check out.");
    let emails = app.queued_emails("studio@example.com").await;
    assert!(
        emails
            .iter()
            .any(|e| e["subject"] == "Your appeal for Lantern Keeper"),
        "{emails:?}"
    );
    assert!(
        app.get("/v1/review/appeals", Some(&bob)).await.body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn reopened_appeals_let_the_developer_resubmit() {
    let app = TestApp::new().await;
    let (dev, admin) = published(&app).await;
    let bob = reviewer(&app, &admin, "bob").await;
    app.post(
        "/v1/review/games/lantern/delist",
        Some(&admin),
        json!({ "rule": "quality_metadata", "notes": "Missing the horror warning." }),
    )
    .await;
    let appeal = app
        .post(
            "/v1/games/lantern/appeals",
            Some(&dev),
            json!({ "statement": "Happy to add it." }),
        )
        .await;
    assert_eq!(appeal.status, StatusCode::CREATED, "{}", appeal.body);
    assert_eq!(appeal.body["submission"]["kind"], "delisting");
    let id = appeal.body["id"].as_str().unwrap();
    let reply = app
        .post(
            &format!("/v1/review/appeals/{id}/decision"),
            Some(&bob),
            json!({ "outcome": "reopened", "notes": "Add the descriptor and resubmit." }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(
        app.get("/v1/games/lantern", Some(&dev)).await.body["status"],
        "changes_requested"
    );
    let resubmitted = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    assert_eq!(
        resubmitted.status,
        StatusCode::CREATED,
        "{}",
        resubmitted.body
    );
}

#[tokio::test]
async fn staff_issue_and_revoke_strikes() {
    let app = TestApp::new().await;
    let (dev, admin) = published(&app).await;
    let bob = reviewer(&app, &admin, "bob").await;
    let reply = app
        .post(
            "/v1/review/developers/studio/strikes",
            Some(&dev),
            json!({ "rule": "spam", "notes": "x" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    let strike = app
        .post(
            "/v1/review/developers/studio/strikes",
            Some(&bob),
            json!({ "rule": "spam", "notes": "Keyword-stuffed tags.", "game": "lantern" }),
        )
        .await;
    assert_eq!(strike.status, StatusCode::CREATED, "{}", strike.body);
    assert_eq!(strike.body["issued_by"]["username"], "bob");
    let id = strike.body["id"].as_str().unwrap();
    let path = format!("/v1/review/strikes/{id}/revoke");
    let reply = app
        .post(&path, Some(&admin), json!({ "reason": "Tags were fine." }))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["revoke_reason"], "Tags were fine.");
    let reply = app
        .post(&path, Some(&admin), json!({ "reason": "Again." }))
        .await;
    assert_eq!(reply.body["error"], "already_revoked");
}
