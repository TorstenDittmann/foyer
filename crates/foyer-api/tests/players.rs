//! Player accounts: wishlists, follows, release emails and passkeys.

mod common;

use axum::http::{Method, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::TestApp;
use serde_json::{Value, json};
use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::softpasskey::SoftPasskey;
use webauthn_rs_proto::{CreationChallengeResponse, RequestChallengeResponse};

async fn put(app: &TestApp, path: &str, token: &str) -> common::Reply {
    app.call(Method::PUT, path, Some(token), None).await
}

async fn delete(app: &TestApp, path: &str, token: &str) -> common::Reply {
    app.call(Method::DELETE, path, Some(token), None).await
}

/// A developer `studio` with the game `lantern` published.
async fn published_game(app: &TestApp) -> String {
    let dev = app.developer_with_game("studio", "lantern").await;
    app.push_build(&dev, "lantern", "windows", b"v1").await;
    app.publish(&dev, "lantern").await;
    dev
}

#[tokio::test]
async fn wishlist_holds_published_games() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    let player = app.register("player", "player@example.com").await;

    assert_eq!(
        app.get("/v1/me/wishlist", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    // Drafts can't be wishlisted, and don't leak.
    assert_eq!(
        put(&app, "/v1/me/wishlist/lantern", &player).await.status,
        StatusCode::NOT_FOUND
    );

    app.push_build(&dev, "lantern", "windows", b"v1").await;
    app.publish(&dev, "lantern").await;
    let added = put(&app, "/v1/me/wishlist/lantern", &player).await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);
    assert_eq!(added.body["game"]["slug"], "lantern");
    // Adding twice keeps one entry with the first date.
    let again = put(&app, "/v1/me/wishlist/lantern", &player).await;
    assert_eq!(again.body["added_at"], added.body["added_at"]);

    let list = app.get("/v1/me/wishlist", Some(&player)).await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(list.body["items"][0]["game"]["title"], "Lantern Keeper");
    assert_eq!(
        app.get("/v1/me/wishlist/lantern", Some(&player))
            .await
            .status,
        StatusCode::OK
    );
    // Another player's wishlist is their own.
    let other = app.register("other", "other@example.com").await;
    assert_eq!(
        app.get("/v1/me/wishlist/lantern", Some(&other))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        delete(&app, "/v1/me/wishlist/lantern", &player)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get("/v1/me/wishlist/lantern", Some(&player))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let list = app.get("/v1/me/wishlist", Some(&player)).await;
    assert!(list.body["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn players_follow_developers() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    let player = app.register("player", "player@example.com").await;

    let own = put(&app, "/v1/me/following/studio", &dev).await;
    assert_eq!(own.status, StatusCode::BAD_REQUEST, "{}", own.body);
    assert_eq!(
        put(&app, "/v1/me/following/nobody", &player).await.status,
        StatusCode::NOT_FOUND
    );

    let followed = put(&app, "/v1/me/following/studio", &player).await;
    assert_eq!(followed.status, StatusCode::OK, "{}", followed.body);
    assert_eq!(followed.body["developer"]["display_name"], "STUDIO");
    let list = app.get("/v1/me/following", Some(&player)).await;
    assert_eq!(list.body["items"][0]["developer"]["slug"], "studio");
    assert_eq!(
        app.get("/v1/me/following/studio", Some(&player))
            .await
            .status,
        StatusCode::OK
    );

    assert_eq!(
        delete(&app, "/v1/me/following/studio", &player)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get("/v1/me/following/studio", Some(&player))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

/// Runs jobs until the release announcement has been fanned out, leaving
/// the emails it queued unsent so the test can read them.
async fn fan_out_release(app: &TestApp) {
    loop {
        let pending: i64 = {
            let conn = app.state.db.connect().unwrap();
            let mut rows = conn
                .query(
                    "SELECT count(*) FROM jobs WHERE kind = 'notify_release' AND state != 'done'",
                    (),
                )
                .await
                .unwrap();
            rows.next().await.unwrap().unwrap().get(0).unwrap()
        };
        if pending == 0 {
            return;
        }
        assert!(
            foyer_api::worker::run_one(&app.state, &app.worker)
                .await
                .unwrap(),
            "the notify_release job never became due"
        );
    }
}

/// Every email job ever queued, sent or not.
async fn email_jobs(app: &TestApp) -> i64 {
    let conn = app.state.db.connect().unwrap();
    let mut rows = conn
        .query("SELECT count(*) FROM jobs WHERE kind = 'send_email'", ())
        .await
        .unwrap();
    rows.next().await.unwrap().unwrap().get(0).unwrap()
}

fn release_emails(emails: &[Value]) -> Vec<&Value> {
    emails
        .iter()
        .filter(|e| e["subject"].as_str().unwrap_or("").contains("released"))
        .collect()
}

#[tokio::test]
async fn followers_who_opted_in_get_one_release_email() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    app.push_build(&dev, "lantern", "windows", b"v1").await;

    // Opted in and confirmed: gets the email.
    let fan = app.register("fan", "fan@example.com").await;
    // Follows, but never opted in.
    let quiet = app.register("quiet", "quiet@example.com").await;
    // Opted in, but never confirmed the address.
    let unconfirmed = app
        .register_unverified("unconfirmed", "unconfirmed@example.com")
        .await;
    for player in [&fan, &quiet, &unconfirmed] {
        put(&app, "/v1/me/following/studio", player).await;
    }
    assert_eq!(
        app.get("/v1/me/settings", Some(&fan)).await.body,
        json!({ "release_emails": false })
    );
    for player in [&fan, &unconfirmed] {
        let on = app
            .patch(
                "/v1/me/settings",
                Some(player),
                json!({ "release_emails": true }),
            )
            .await;
        assert_eq!(on.body, json!({ "release_emails": true }));
    }

    app.publish(&dev, "lantern").await;
    fan_out_release(&app).await;

    let emails = app.queued_emails("fan@example.com").await;
    let sent = release_emails(&emails);
    assert_eq!(sent.len(), 1, "{emails:?}");
    assert_eq!(
        sent[0]["subject"],
        "STUDIO released Lantern Keeper on Foyer"
    );
    let body = sent[0]["body"].as_str().unwrap();
    assert!(
        body.contains("http://localhost:3000/games/lantern"),
        "{body}"
    );
    assert!(release_emails(&app.queued_emails("quiet@example.com").await).is_empty());
    assert!(release_emails(&app.queued_emails("unconfirmed@example.com").await).is_empty());

    // A re-run of the announcement emails nobody twice.
    let emails_before = email_jobs(&app).await;
    foyer_api::releases::queue(&app.state.db.connect().unwrap(), {
        let game = app.get("/v1/games/lantern", None).await;
        game.body["id"].as_str().unwrap().parse().unwrap()
    })
    .await
    .unwrap();
    fan_out_release(&app).await;
    assert_eq!(email_jobs(&app).await, emails_before);

    // The unsubscribe link turns release emails off, without signing in.
    let unsubscribe = body
        .split_whitespace()
        .find(|w| w.contains("/unsubscribe?token="))
        .expect("no unsubscribe link");
    let token = common::token_from_link(unsubscribe);
    let bad = app
        .post("/v1/unsubscribe", None, json!({ "token": "nope" }))
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let done = app
        .post("/v1/unsubscribe", None, json!({ "token": token }))
        .await;
    assert_eq!(done.status, StatusCode::NO_CONTENT, "{}", done.body);
    assert_eq!(
        app.get("/v1/me/settings", Some(&fan)).await.body,
        json!({ "release_emails": false })
    );
}

#[tokio::test]
async fn delisted_games_drop_off_the_wishlist() {
    let app = TestApp::new().await;
    published_game(&app).await;
    let player = app.register("player", "player@example.com").await;
    put(&app, "/v1/me/wishlist/lantern", &player).await;

    let admin = app
        .post(
            "/v1/auth/login",
            None,
            json!({ "login": "root", "password": "correct horse battery" }),
        )
        .await
        .body["token"]
        .as_str()
        .unwrap()
        .to_string();
    let delisted = app
        .post(
            "/v1/review/games/lantern/delist",
            Some(&admin),
            json!({ "notes": "spam", "rule": "spam" }),
        )
        .await;
    assert_eq!(delisted.status, StatusCode::OK, "{}", delisted.body);

    let list = app.get("/v1/me/wishlist", Some(&player)).await;
    assert!(list.body["items"].as_array().unwrap().is_empty());
    // It can still be removed.
    assert_eq!(
        delete(&app, "/v1/me/wishlist/lantern", &player)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

/// What the browser does with `navigator.credentials.create`, using a
/// software authenticator. It can't store discoverable credentials, so it is
/// told not to (browsers with platform passkeys always do).
fn create_passkey(authenticator: &mut SoftPasskey, mut options: Value) -> Value {
    options["publicKey"]["authenticatorSelection"]["requireResidentKey"] = json!(false);
    options["publicKey"]["authenticatorSelection"]
        .as_object_mut()
        .unwrap()
        .remove("residentKey");
    let options: CreationChallengeResponse = serde_json::from_value(options).unwrap();
    let credential = authenticator
        .do_registration("http://localhost:3000".parse().unwrap(), options)
        .unwrap();
    serde_json::to_value(credential).unwrap()
}

/// What the browser does with `navigator.credentials.get` when the user
/// picks the passkey `credential_id`: a discoverable passkey also returns the
/// account's user handle.
fn use_passkey(
    authenticator: &mut SoftPasskey,
    mut options: Value,
    credential_id: &str,
    user_id: &str,
) -> Value {
    options["publicKey"]["allowCredentials"] =
        json!([{ "type": "public-key", "id": credential_id }]);
    let options: RequestChallengeResponse = serde_json::from_value(options).unwrap();
    let credential = authenticator
        .do_authentication("http://localhost:3000".parse().unwrap(), options)
        .unwrap();
    let mut credential = serde_json::to_value(credential).unwrap();
    let user: uuid::Uuid = user_id.parse().unwrap();
    credential["response"]["userHandle"] = json!(URL_SAFE_NO_PAD.encode(user.as_bytes()));
    credential
}

#[tokio::test]
async fn passkeys_sign_in_without_a_password() {
    let app = TestApp::new().await;
    let player = app.register("player", "player@example.com").await;
    let user_id = app.get("/v1/me", Some(&player)).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut authenticator = SoftPasskey::new(true);

    // Registering needs a session.
    assert_eq!(
        app.post("/v1/auth/passkeys/register/start", None, json!(null))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let start = app
        .post(
            "/v1/auth/passkeys/register/start",
            Some(&player),
            json!(null),
        )
        .await;
    assert_eq!(start.status, StatusCode::OK, "{}", start.body);
    let selection = &start.body["options"]["publicKey"]["authenticatorSelection"];
    assert_eq!(selection["residentKey"], "required");
    assert_eq!(selection["userVerification"], "required");
    assert_eq!(start.body["options"]["publicKey"]["rp"]["id"], "localhost");

    let credential = create_passkey(&mut authenticator, start.body["options"].clone());
    let credential_id = credential["id"].as_str().unwrap().to_string();
    // Another account can't finish this user's ceremony.
    let other = app.register("other", "other@example.com").await;
    let stolen = app
        .post(
            "/v1/auth/passkeys/register/finish",
            Some(&other),
            json!({ "ceremony": start.body["ceremony"], "credential": credential }),
        )
        .await;
    assert_eq!(stolen.status, StatusCode::BAD_REQUEST, "{}", stolen.body);
    let finished = app
        .post(
            "/v1/auth/passkeys/register/finish",
            Some(&player),
            json!({ "ceremony": start.body["ceremony"], "name": "Laptop", "credential": credential }),
        )
        .await;
    assert_eq!(finished.status, StatusCode::CREATED, "{}", finished.body);
    assert_eq!(finished.body["name"], "Laptop");
    // A ceremony works once.
    let replay = app
        .post(
            "/v1/auth/passkeys/register/finish",
            Some(&player),
            json!({ "ceremony": start.body["ceremony"], "credential": credential }),
        )
        .await;
    assert_eq!(replay.status, StatusCode::BAD_REQUEST);

    // The next registration excludes the passkey already on the account.
    let again = app
        .post(
            "/v1/auth/passkeys/register/start",
            Some(&player),
            json!(null),
        )
        .await;
    assert_eq!(
        again.body["options"]["publicKey"]["excludeCredentials"][0]["id"],
        credential_id.as_str()
    );

    let keys = app.get("/v1/auth/passkeys", Some(&player)).await;
    assert_eq!(keys.body.as_array().unwrap().len(), 1);
    assert_eq!(keys.body[0]["last_used_at"], Value::Null);

    // Sign in: no username, no password.
    let login = app
        .post("/v1/auth/passkeys/login/start", None, json!(null))
        .await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.body);
    assert!(login.body["options"].get("mediation").is_none());
    let assertion = use_passkey(
        &mut authenticator,
        login.body["options"].clone(),
        &credential_id,
        &user_id,
    );
    let signed_in = app
        .post(
            "/v1/auth/passkeys/login/finish",
            None,
            json!({ "ceremony": login.body["ceremony"], "credential": assertion }),
        )
        .await;
    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.body);
    assert_eq!(signed_in.body["user"]["username"], "player");
    let session = signed_in.body["token"].as_str().unwrap();
    assert_eq!(
        app.get("/v1/me", Some(session)).await.body["username"],
        "player"
    );
    let keys = app.get("/v1/auth/passkeys", Some(&player)).await;
    assert_ne!(keys.body[0]["last_used_at"], Value::Null);

    // The same answer can't be replayed.
    let replay = app
        .post(
            "/v1/auth/passkeys/login/finish",
            None,
            json!({ "ceremony": login.body["ceremony"], "credential": assertion }),
        )
        .await;
    assert_eq!(replay.status, StatusCode::BAD_REQUEST);

    // A passkey claiming to belong to another account doesn't work.
    let login = app
        .post("/v1/auth/passkeys/login/start", None, json!(null))
        .await;
    let other_id = app.get("/v1/me", Some(&other)).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let forged = use_passkey(
        &mut authenticator,
        login.body["options"].clone(),
        &credential_id,
        &other_id,
    );
    let refused = app
        .post(
            "/v1/auth/passkeys/login/finish",
            None,
            json!({ "ceremony": login.body["ceremony"], "credential": forged }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);

    // Once removed, the passkey no longer signs in.
    let id = keys.body[0]["id"].as_str().unwrap();
    assert_eq!(
        delete(&app, &format!("/v1/auth/passkeys/{id}"), &other)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        delete(&app, &format!("/v1/auth/passkeys/{id}"), &player)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let login = app
        .post("/v1/auth/passkeys/login/start", None, json!(null))
        .await;
    let assertion = use_passkey(
        &mut authenticator,
        login.body["options"].clone(),
        &credential_id,
        &user_id,
    );
    let refused = app
        .post(
            "/v1/auth/passkeys/login/finish",
            None,
            json!({ "ceremony": login.body["ceremony"], "credential": assertion }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn passkeys_need_a_domain_name() {
    let app = TestApp::new().await;
    let state = app
        .state
        .clone()
        .with_public_web_url("http://127.0.0.1:3000".parse().unwrap());
    let router = foyer_api::router(state);
    let response = tower::ServiceExt::oneshot(
        router,
        axum::http::Request::post("/v1/auth/passkeys/login/start")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
