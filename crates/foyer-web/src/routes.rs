//! Plain Axum routes next to the Leptos pages: things that answer with a
//! redirect rather than a page.
//!
//! - `GET /download/{slug}/{channel}` sends the browser to a fresh, signed
//!   link to the channel's zip. The link is made per click, so it never
//!   expires on an open page, and every click is counted.
//! - `GET /auth/discord` and `GET /auth/discord/callback` run the Discord
//!   sign-in. The OAuth `state` is kept in a short-lived cookie and compared
//!   on the way back, so a callback only completes in the browser that
//!   started it.
//! - `POST /auth/passkey/{login,register}/{start,finish}` relay passkey
//!   ceremonies between `public/passkey.js` and the API as JSON, so the
//!   session token stays in its `HttpOnly` cookie. Only JSON bodies are
//!   accepted, which other sites can't send with the visitor's cookies.
//! - `GET /tester?key=...` checks a tester key, remembers it in the
//!   `foyer_keys` cookie and opens the game it unlocks.
//! - `GET /install.sh` and `GET /install.ps1` serve the CLI install scripts
//!   from `install/`.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, Query};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use foyer_types::{FinishPasskeyLogin, FinishPasskeyRegistration};
use serde::Deserialize;
use serde_json::json;

use crate::backend::{self, CLIENT_IP, TESTER_KEYS};

pub const SESSION_COOKIE: &str = "foyer_session";
const OAUTH_STATE_COOKIE: &str = "foyer_oauth_state";
pub const SESSION_MAX_AGE: u32 = 60 * 60 * 24 * 30;
/// Tester keys the visitor unlocked, dot separated.
pub const TESTER_COOKIE: &str = "foyer_keys";
/// Keys expire on their own; the cookie only needs to outlive the longest.
const TESTER_MAX_AGE: u32 = 60 * 60 * 24 * 365;

const INSTALL_SH: &str = include_str!("../../../install/install.sh");
const INSTALL_PS1: &str = include_str!("../../../install/install.ps1");

/// A cookie's value from the request's `Cookie` headers.
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
        .filter(|v| !v.is_empty())
}

/// A `Set-Cookie` value. `Secure` is left off so sign-in works over plain
/// http in local dev; add it when the site is served over https.
pub fn set_cookie(name: &str, value: &str, path: &str, max_age: u32) -> Option<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{name}={value}; Path={path}; HttpOnly; SameSite=Lax; Max-Age={max_age}"
    ))
    .ok()
}

pub fn router<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {
    axum::Router::new()
        .route("/download/{slug}/{channel}", get(download))
        .route("/auth/discord", get(discord_start))
        .route("/auth/discord/callback", get(discord_callback))
        .route("/auth/passkey/login/start", post(passkey_login_start))
        .route("/auth/passkey/login/finish", post(passkey_login_finish))
        .route("/auth/passkey/register/start", post(passkey_register_start))
        .route(
            "/auth/passkey/register/finish",
            post(passkey_register_finish),
        )
        .route("/tester", get(tester))
        .route("/install.sh", get(|| async { script(INSTALL_SH) }))
        .route("/install.ps1", get(|| async { script(INSTALL_PS1) }))
}

fn script(body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        body,
    )
        .into_response()
}

/// Runs `f` with the visitor's IP and tester keys set for API calls.
async fn as_visitor<F: Future>(headers: &HeaderMap, peer: SocketAddr, f: F) -> F::Output {
    let ip = backend::client_ip(headers, Some(peer.ip()));
    let keys = cookie(headers, TESTER_COOKIE)
        .map(|v| backend::parse_tester_keys(&v))
        .unwrap_or_default();
    CLIENT_IP.scope(ip, TESTER_KEYS.scope(keys, f)).await
}

#[derive(Deserialize)]
struct TesterQuery {
    key: Option<String>,
}

/// Checks a tester key and adds it to the visitor's keys. The newest key
/// goes first, so the oldest falls off once the cookie is full.
async fn tester(
    Query(query): Query<TesterQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let key = query.key.unwrap_or_default().trim().to_string();
    let redeemed = if key.starts_with("fyk_") {
        as_visitor(&headers, peer, backend::redeem_tester_key(&key))
            .await
            .ok()
    } else {
        None
    };
    let Some(access) = redeemed else {
        return Redirect::to("/tester-key").into_response();
    };
    let mut keys = vec![key];
    keys.extend(
        cookie(&headers, TESTER_COOKIE)
            .map(|v| backend::parse_tester_keys(&v))
            .unwrap_or_default(),
    );
    let mut seen = std::collections::HashSet::new();
    keys.retain(|k| seen.insert(k.clone()));
    let keys = backend::parse_tester_keys(&keys.join("."));
    let slug = clean_slug(&access.game);
    let mut response = Redirect::to(&format!("/games/{slug}?tester=1")).into_response();
    if let Some(v) = set_cookie(TESTER_COOKIE, &keys.join("."), "/", TESTER_MAX_AGE) {
        response.headers_mut().append(header::SET_COOKIE, v);
    }
    response
}

async fn download(
    Path((slug, channel)): Path<(String, String)>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    // Staff and the game's developer may download builds that aren't public
    // yet; the API decides with their session.
    let token = cookie(&headers, SESSION_COOKIE);
    let result = as_visitor(
        &headers,
        peer,
        backend::archive(&slug, &channel, token.as_deref()),
    )
    .await;
    match result {
        Ok(archive) => Redirect::to(&archive.url).into_response(),
        Err(e) => {
            let reason = if e.status == 429 { "busy" } else { "failed" };
            let slug = clean_slug(&slug);
            Redirect::to(&format!("/games/{slug}?download={reason}")).into_response()
        }
    }
}

async fn discord_start(ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    match as_visitor(&headers, peer, backend::discord_authorize()).await {
        Ok(auth) => {
            let mut response = Redirect::to(&auth.url).into_response();
            if let Some(v) = set_cookie(OAUTH_STATE_COOKIE, &auth.state, "/auth/discord", 600) {
                response.headers_mut().append(header::SET_COOKIE, v);
            }
            response
        }
        Err(e) => {
            let code = if matches!(e.status, 501 | 503) {
                "discord_off"
            } else {
                "discord"
            };
            Redirect::to(&format!("/signin?error={code}")).into_response()
        }
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn discord_callback(
    Query(query): Query<CallbackQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let expected = cookie(&headers, OAUTH_STATE_COOKIE);
    let result = match (query.code, query.state, expected) {
        // The visitor cancelled on Discord, or this browser didn't start
        // the sign-in.
        (Some(code), Some(state), Some(expected)) if state == expected => {
            as_visitor(&headers, peer, backend::discord_callback(&code, &state))
                .await
                .ok()
        }
        _ => None,
    };
    let mut response = match &result {
        Some(_) => Redirect::to("/account").into_response(),
        None => Redirect::to("/signin?error=discord").into_response(),
    };
    let cookies = response.headers_mut();
    if let Some(v) = set_cookie(OAUTH_STATE_COOKIE, "", "/auth/discord", 0) {
        cookies.append(header::SET_COOKIE, v);
    }
    if let Some(token) = result
        && let Some(v) = set_cookie(SESSION_COOKIE, &token, "/", SESSION_MAX_AGE)
    {
        cookies.append(header::SET_COOKIE, v);
    }
    response
}

/// An API error as JSON for `passkey.js`, which shows `message`.
fn json_error(e: backend::BackendError) -> Response {
    let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let message = if matches!(e.status, 501 | 503) && e.message.contains("passkeys") {
        "Passkeys aren't set up on this site.".to_string()
    } else {
        e.message
    };
    (status, Json(json!({ "message": message }))).into_response()
}

fn signed_out() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "message": "Sign in to continue." })),
    )
        .into_response()
}

async fn passkey_login_start(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(_): Json<serde_json::Value>,
) -> Response {
    match as_visitor(&headers, peer, backend::passkey_login_start()).await {
        Ok(challenge) => Json(challenge).into_response(),
        Err(e) => json_error(e),
    }
}

#[derive(Deserialize)]
struct PasskeyLogin {
    #[serde(flatten)]
    finish: FinishPasskeyLogin,
    next: Option<String>,
}

async fn passkey_login_finish(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<PasskeyLogin>,
) -> Response {
    match as_visitor(&headers, peer, backend::passkey_login_finish(&req.finish)).await {
        Ok(token) => {
            let redirect = crate::api::safe_next(req.next, "/account");
            let mut response = Json(json!({ "redirect": redirect })).into_response();
            if let Some(v) = set_cookie(SESSION_COOKIE, &token, "/", SESSION_MAX_AGE) {
                response.headers_mut().append(header::SET_COOKIE, v);
            }
            response
        }
        Err(e) => json_error(e),
    }
}

async fn passkey_register_start(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(_): Json<serde_json::Value>,
) -> Response {
    let Some(token) = cookie(&headers, SESSION_COOKIE) else {
        return signed_out();
    };
    match as_visitor(&headers, peer, backend::passkey_register_start(&token)).await {
        Ok(challenge) => Json(challenge).into_response(),
        Err(e) => json_error(e),
    }
}

async fn passkey_register_finish(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<FinishPasskeyRegistration>,
) -> Response {
    let Some(token) = cookie(&headers, SESSION_COOKIE) else {
        return signed_out();
    };
    match as_visitor(
        &headers,
        peer,
        backend::passkey_register_finish(&token, &req),
    )
    .await
    {
        Ok(info) => (StatusCode::CREATED, Json(info)).into_response(),
        Err(e) => json_error(e),
    }
}

/// Slugs are plain; drop anything else the visitor put in the path.
fn clean_slug(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}
