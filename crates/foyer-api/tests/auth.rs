//! Email verification, password resets and Discord sign-in.

mod common;

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use common::{ADMIN_EMAIL, TestApp, token_from_link};
use foyer_api::DiscordConfig;
use serde_json::{Value, json};

#[tokio::test]
async fn email_verification_gates_submissions() {
    let app = TestApp::new().await;
    let dev = app
        .register_unverified("studio", "studio@example.com")
        .await;
    assert_eq!(
        app.get("/v1/me", Some(&dev)).await.body["email_verified"],
        false
    );
    let first_link = app.last_email_link("studio@example.com").await;
    assert!(
        first_link.starts_with("http://127.0.0.1:3000/verify-email?token="),
        "{first_link}"
    );

    // Unverified developers can set up a game but not submit it.
    app.post(
        "/v1/developers",
        Some(&dev),
        json!({ "slug": "studio", "display_name": "Studio" }),
    )
    .await;
    app.post(
        "/v1/games",
        Some(&dev),
        json!({ "slug": "lantern", "title": "Lantern", "description": "A game." }),
    )
    .await;
    app.push_build(&dev, "lantern", "windows", b"bytes").await;
    let blocked = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    assert_eq!(blocked.status, StatusCode::CONFLICT);
    assert_eq!(blocked.body["error"], "email_unverified");

    // Resending replaces the earlier link.
    let resent = app
        .post("/v1/auth/verify-email/resend", Some(&dev), json!(null))
        .await;
    assert_eq!(resent.status, StatusCode::NO_CONTENT);
    let link = app.last_email_link("studio@example.com").await;
    let stale = app
        .post(
            "/v1/auth/verify-email",
            None,
            json!({ "token": token_from_link(&first_link) }),
        )
        .await;
    assert_eq!(stale.status, StatusCode::BAD_REQUEST);
    assert_eq!(stale.body["error"], "invalid_token");

    let verified = app
        .post(
            "/v1/auth/verify-email",
            None,
            json!({ "token": token_from_link(&link) }),
        )
        .await;
    assert_eq!(verified.status, StatusCode::OK, "{}", verified.body);
    assert_eq!(verified.body["email_verified"], true);
    // One use only.
    let again = app
        .post(
            "/v1/auth/verify-email",
            None,
            json!({ "token": token_from_link(&link) }),
        )
        .await;
    assert_eq!(again.body["error"], "invalid_token");
    let resend = app
        .post("/v1/auth/verify-email/resend", Some(&dev), json!(null))
        .await;
    assert_eq!(resend.body["error"], "already_verified");

    let submitted = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    assert_eq!(submitted.status, StatusCode::CREATED, "{}", submitted.body);
}

#[tokio::test]
async fn admin_emails_need_confirming() {
    let app = TestApp::new().await;
    let squatter = app.register_unverified("root", ADMIN_EMAIL).await;
    assert_eq!(
        app.get("/v1/me", Some(&squatter)).await.body["role"],
        "user"
    );
    let link = app.last_email_link(ADMIN_EMAIL).await;
    app.post(
        "/v1/auth/verify-email",
        None,
        json!({ "token": token_from_link(&link) }),
    )
    .await;
    assert_eq!(
        app.get("/v1/me", Some(&squatter)).await.body["role"],
        "admin"
    );
}

#[tokio::test]
async fn password_reset_by_email() {
    let app = TestApp::new().await;
    let session = app.register("ada", "ada@example.com").await;

    // Unknown addresses look the same and send nothing.
    let unknown = app
        .post(
            "/v1/auth/password-reset/request",
            None,
            json!({ "email": "nobody@example.com" }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NO_CONTENT);
    assert!(app.queued_emails("nobody@example.com").await.is_empty());

    let requested = app
        .post(
            "/v1/auth/password-reset/request",
            None,
            json!({ "email": "ADA@example.com" }),
        )
        .await;
    assert_eq!(requested.status, StatusCode::NO_CONTENT);
    let emails = app.queued_emails("ada@example.com").await;
    assert_eq!(
        emails.last().unwrap()["subject"],
        "Reset your Foyer password"
    );
    let link = app.last_email_link("ada@example.com").await;
    assert!(link.contains("/reset-password?token="), "{link}");
    let token = token_from_link(&link);

    let weak = app
        .post(
            "/v1/auth/password-reset",
            None,
            json!({ "token": token, "password": "short" }),
        )
        .await;
    assert_eq!(weak.status, StatusCode::BAD_REQUEST);
    let reset = app
        .post(
            "/v1/auth/password-reset",
            None,
            json!({ "token": token, "password": "a brand new password" }),
        )
        .await;
    assert_eq!(reset.status, StatusCode::NO_CONTENT, "{}", reset.body);
    // Sessions are signed out; the new password works, the old one doesn't.
    assert_eq!(
        app.get("/v1/me", Some(&session)).await.status,
        StatusCode::UNAUTHORIZED
    );
    for (password, status) in [
        ("correct horse battery", StatusCode::UNAUTHORIZED),
        ("a brand new password", StatusCode::OK),
    ] {
        let login = app
            .post(
                "/v1/auth/login",
                None,
                json!({ "login": "ada", "password": password }),
            )
            .await;
        assert_eq!(login.status, status);
    }
    let reused = app
        .post(
            "/v1/auth/password-reset",
            None,
            json!({ "token": token, "password": "yet another password" }),
        )
        .await;
    assert_eq!(reused.body["error"], "invalid_token");
}

type TokenForm = Vec<(String, String)>;

/// What the fake Discord returns for `/users/@me`.
#[derive(Clone, Default)]
struct FakeDiscord {
    profile: Arc<Mutex<Value>>,
    /// Form bodies of token requests.
    exchanges: Arc<Mutex<Vec<TokenForm>>>,
}

async fn fake_token(
    State(fake): State<FakeDiscord>,
    Form(form): Form<Vec<(String, String)>>,
) -> Result<Json<Value>, StatusCode> {
    let ok = form.iter().any(|(k, v)| k == "code" && v == "good-code");
    fake.exchanges.lock().unwrap().push(form);
    if ok {
        Ok(Json(
            json!({ "access_token": "discord-access", "token_type": "Bearer" }),
        ))
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

async fn fake_me(
    State(fake): State<FakeDiscord>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer discord-access") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(fake.profile.lock().unwrap().clone()))
}

async fn with_discord(mut app: TestApp) -> (TestApp, FakeDiscord) {
    let fake = FakeDiscord::default();
    let server = Router::new()
        .route("/api/oauth2/token", post(fake_token))
        .route("/api/users/@me", get(fake_me))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
    app.state = app
        .state
        .clone()
        .with_public_web_url("https://foyer.test".parse().unwrap())
        .with_discord(DiscordConfig {
            client_id: "client-123".into(),
            client_secret: "shh".into(),
            api_url: format!("http://{addr}/api").parse().unwrap(),
            authorize_url: "https://discord.test/oauth2/authorize".parse().unwrap(),
        });
    app.router = foyer_api::router(app.state.clone());
    (app, fake)
}

async fn discord_sign_in(app: &TestApp) -> common::Reply {
    let start = app.get("/v1/auth/discord/authorize", None).await;
    assert_eq!(start.status, StatusCode::OK, "{}", start.body);
    let state = start.body["state"].as_str().unwrap();
    app.post(
        "/v1/auth/discord/callback",
        None,
        json!({ "code": "good-code", "state": state }),
    )
    .await
}

#[tokio::test]
async fn discord_sign_in_creates_and_reuses_accounts() {
    let (app, fake) = with_discord(TestApp::new().await).await;
    *fake.profile.lock().unwrap() =
        json!({ "id": "4242", "username": "Ada.L", "email": "Ada@Example.com", "verified": true });

    let start = app.get("/v1/auth/discord/authorize", None).await;
    let url: url::Url = start.body["url"].as_str().unwrap().parse().unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(url.host_str(), Some("discord.test"));
    assert_eq!(query["client_id"], "client-123");
    assert_eq!(query["scope"], "identify email");
    assert_eq!(
        query["redirect_uri"],
        "https://foyer.test/auth/discord/callback"
    );
    assert_eq!(query["state"], start.body["state"].as_str().unwrap());

    // A state this API didn't issue is refused before Discord is asked.
    let forged = app
        .post(
            "/v1/auth/discord/callback",
            None,
            json!({ "code": "good-code", "state": "made-up" }),
        )
        .await;
    assert_eq!(forged.status, StatusCode::BAD_REQUEST);
    assert_eq!(forged.body["error"], "discord_failed");
    assert!(fake.exchanges.lock().unwrap().is_empty());
    // A bad code fails cleanly.
    let bad_code = app
        .post(
            "/v1/auth/discord/callback",
            None,
            json!({ "code": "bad", "state": start.body["state"] }),
        )
        .await;
    assert_eq!(bad_code.body["error"], "discord_failed");

    let first = discord_sign_in(&app).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(first.body["user"]["username"], "ada_l");
    assert_eq!(first.body["user"]["email"], "ada@example.com");
    assert_eq!(first.body["user"]["email_verified"], true);
    let exchange = fake.exchanges.lock().unwrap().last().unwrap().clone();
    assert!(exchange.contains(&(
        "redirect_uri".into(),
        "https://foyer.test/auth/discord/callback".into()
    )));
    // States are single use.
    let second = discord_sign_in(&app).await;
    assert_eq!(second.body["user"]["id"], first.body["user"]["id"]);

    // No verified email on Discord: refused.
    *fake.profile.lock().unwrap() =
        json!({ "id": "777", "username": "anon", "email": null, "verified": false });
    let refused = discord_sign_in(&app).await;
    assert_eq!(refused.body["error"], "discord_email_unverified");
}

#[tokio::test]
async fn discord_links_an_account_with_the_same_email() {
    let (app, fake) = with_discord(TestApp::new().await).await;
    // Someone registered this address without confirming it.
    let squatter = app
        .register_unverified("squatter", "owner@example.com")
        .await;
    *fake.profile.lock().unwrap() = json!({
        "id": "9001", "username": "owner", "email": "owner@example.com", "verified": true
    });
    let signed_in = discord_sign_in(&app).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.body);
    assert_eq!(signed_in.body["user"]["username"], "squatter");
    assert_eq!(signed_in.body["user"]["email_verified"], true);
    // The unconfirmed registration loses its session and password.
    assert_eq!(
        app.get("/v1/me", Some(&squatter)).await.status,
        StatusCode::UNAUTHORIZED
    );
    let login = app
        .post(
            "/v1/auth/login",
            None,
            json!({ "login": "squatter", "password": "correct horse battery" }),
        )
        .await;
    assert_eq!(login.status, StatusCode::UNAUTHORIZED);

    // A confirmed account keeps its password when linked, and another
    // Discord account can't take it over.
    let ada = app.register("ada", "ada@example.com").await;
    *fake.profile.lock().unwrap() =
        json!({ "id": "1", "username": "ada", "email": "ada@example.com", "verified": true });
    assert_eq!(discord_sign_in(&app).await.status, StatusCode::OK);
    assert_eq!(app.get("/v1/me", Some(&ada)).await.status, StatusCode::OK);
    *fake.profile.lock().unwrap() =
        json!({ "id": "2", "username": "ada2", "email": "ada@example.com", "verified": true });
    let taken = discord_sign_in(&app).await;
    assert_eq!(taken.status, StatusCode::CONFLICT);
    assert_eq!(taken.body["error"], "discord_email_taken");
}

#[tokio::test]
async fn discord_is_unavailable_without_configuration() {
    let app = TestApp::new().await;
    let reply = app.get("/v1/auth/discord/authorize", None).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["error"], "discord_unavailable");
}
