//! Where the website's data comes from. Server only.
//!
//! By default every call goes to the Foyer API at `FOYER_API_URL` (default
//! `http://127.0.0.1:8080`), forwarding the visitor's session token as a
//! bearer token. With `FOYER_WEB_MOCK=1` the in-memory mock in `mock.rs` is
//! used instead. The functions here also stitch several API responses into
//! the shapes one page needs.
//!
//! Tester keys the visitor unlocked (the `foyer_keys` cookie, set by
//! `/tester?key=...`) go along in `X-Foyer-Key`, so unpublished games and
//! private channels they cover show up like public ones.
//!
//! Every API request carries the visitor's IP (`X-Foyer-Client-IP`) with
//! the shared `FOYER_INTERNAL_KEY`, so the API rate limits visitors rather
//! than the website. The visitor's IP is the TCP peer, or with
//! `FOYER_PROXY_HOPS` set, the matching `X-Forwarded-For` entry.

use std::net::{IpAddr, SocketAddr};
use std::sync::LazyLock;

use axum::http::HeaderMap;
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::mock;
use crate::model::*;

#[derive(Debug, Clone)]
pub struct BackendError {
    pub status: u16,
    pub message: String,
}

impl BackendError {
    fn is(&self, status: StatusCode) -> bool {
        self.status == status.as_u16()
    }
}

pub type BackendResult<T> = Result<T, BackendError>;

/// Turns a 404 into `None`.
fn optional<T>(r: BackendResult<T>) -> BackendResult<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is(StatusCode::NOT_FOUND) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The mock's errors, given the status the API would have used.
fn mocked<T>(r: mock::MockResult<T>) -> BackendResult<T> {
    r.map_err(|message| {
        let status = if message == "Not found." {
            404
        } else if message == "Sign in to continue." {
            401
        } else {
            400
        };
        BackendError { status, message }
    })
}

enum Backend {
    Api(ApiClient),
    Mock,
}

static BACKEND: LazyLock<Backend> = LazyLock::new(|| {
    if std::env::var("FOYER_WEB_MOCK").is_ok_and(|v| v == "1" || v == "true") {
        leptos::logging::log!("using in-memory mock data (FOYER_WEB_MOCK)");
        Backend::Mock
    } else {
        let base =
            std::env::var("FOYER_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
        leptos::logging::log!("using the Foyer API at {base}");
        Backend::Api(ApiClient::new(base))
    }
});

pub fn is_mock() -> bool {
    matches!(*BACKEND, Backend::Mock)
}

// ---- HTTP client -----------------------------------------------------------

struct ApiClient {
    http: reqwest::Client,
    base: String,
    internal_key: Option<String>,
}

tokio::task_local! {
    /// The visitor's IP for API calls made outside a Leptos request (the
    /// plain Axum handlers in `routes.rs`).
    pub static CLIENT_IP: Option<IpAddr>;
    /// The visitor's tester keys, likewise.
    pub static TESTER_KEYS: Vec<String>;
}

/// The tester keys in a `foyer_keys` cookie value (dot separated, since
/// cookie values can't hold commas).
pub fn parse_tester_keys(value: &str) -> Vec<String> {
    let mut keys: Vec<String> = value
        .split('.')
        .map(str::trim)
        .filter(|k| k.starts_with("fyk_") && k.len() <= 100)
        .map(str::to_string)
        .collect();
    keys.dedup();
    keys.truncate(MAX_KEYS_PER_REQUEST);
    keys
}

/// The current visitor's tester keys, from the task-local or the cookie.
fn current_tester_keys() -> Vec<String> {
    if let Ok(keys) = TESTER_KEYS.try_with(Clone::clone) {
        return keys;
    }
    leptos::prelude::use_context::<axum::http::request::Parts>()
        .and_then(|parts| crate::routes::cookie(&parts.headers, crate::routes::TESTER_COOKIE))
        .map(|v| parse_tester_keys(&v))
        .unwrap_or_default()
}

/// The visitor's address: the matching `X-Forwarded-For` entry when the site
/// runs behind `FOYER_PROXY_HOPS` proxies, else the TCP peer.
pub fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>) -> Option<IpAddr> {
    static HOPS: LazyLock<usize> = LazyLock::new(|| {
        std::env::var("FOYER_PROXY_HOPS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    });
    if *HOPS > 0
        && let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
    {
        let entries: Vec<&str> = forwarded.split(',').map(str::trim).collect();
        if let Some(ip) = entries
            .get(entries.len().saturating_sub(*HOPS))
            .and_then(|e| e.parse().ok())
        {
            return Some(ip);
        }
    }
    peer
}

/// The current visitor's IP, from the task-local or the Leptos request.
fn current_client_ip() -> Option<IpAddr> {
    if let Ok(ip) = CLIENT_IP.try_with(|ip| *ip) {
        return ip;
    }
    let parts = leptos::prelude::use_context::<axum::http::request::Parts>()?;
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    client_ip(&parts.headers, peer)
}

impl ApiClient {
    fn new(base: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.trim_end_matches('/').to_string(),
            internal_key: std::env::var("FOYER_INTERNAL_KEY")
                .ok()
                .filter(|k| !k.trim().is_empty()),
        }
    }

    async fn call<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        query: &[(&str, String)],
        body: Option<&B>,
    ) -> BackendResult<T> {
        let mut req = self
            .http
            .request(method, format!("{}{API_PREFIX}{path}", self.base))
            .query(query);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        if let (Some(key), Some(ip)) = (&self.internal_key, current_client_ip()) {
            req = req
                .header("x-foyer-internal-key", key)
                .header("x-foyer-client-ip", ip.to_string());
        }
        let keys = current_tester_keys();
        if !keys.is_empty() {
            req = req.header(TESTER_KEY_HEADER, keys.join(","));
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req.send().await.map_err(|e| {
            leptos::logging::error!("API request to {path} failed: {e}");
            BackendError {
                status: 503,
                message: "Foyer can't reach its API right now. Try again in a moment.".into(),
            }
        })?;
        let status = resp.status();
        if status.is_success() {
            // 204 and friends: deserialize from `null` so `()` works.
            let bytes = resp.bytes().await.map_err(|e| BackendError {
                status: 502,
                message: e.to_string(),
            })?;
            let bytes: &[u8] = if bytes.is_empty() { b"null" } else { &bytes };
            return serde_json::from_slice(bytes).map_err(|e| {
                leptos::logging::error!("unexpected response from {path}: {e}");
                BackendError {
                    status: 502,
                    message: "The API sent a response the website didn't understand.".into(),
                }
            });
        }
        let message = match resp.json::<ApiError>().await {
            Ok(e) => capitalize(&e.message),
            Err(_) => status
                .canonical_reason()
                .unwrap_or("Something went wrong")
                .to_string(),
        };
        Err(BackendError {
            status: status.as_u16(),
            message,
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        token: Option<&str>,
        query: &[(&str, String)],
    ) -> BackendResult<T> {
        self.call::<(), T>(Method::GET, path, token, query, None)
            .await
    }

    async fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: &B,
    ) -> BackendResult<T> {
        self.call(method, path, token, &[], Some(body)).await
    }
}

/// The API's messages are lowercase fragments; the site shows sentences.
fn capitalize(message: &str) -> String {
    let mut chars = message.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out: String = first.to_uppercase().chain(chars).collect();
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

fn games_query(tag: Option<&str>, q: Option<&str>, limit: usize) -> Vec<(&'static str, String)> {
    let mut query = vec![("limit", limit.to_string())];
    if let Some(tag) = tag {
        query.push(("tag", tag.to_string()));
    }
    if let Some(q) = q {
        query.push(("q", q.to_string()));
    }
    query
}

// ---- public catalog --------------------------------------------------------

pub async fn list_games(
    tag: Option<&str>,
    q: Option<&str>,
    limit: usize,
) -> BackendResult<Page<Game>> {
    match &*BACKEND {
        Backend::Mock => Ok(mock::store().list_games(tag, q, limit)),
        Backend::Api(api) => api.get("/games", None, &games_query(tag, q, limit)).await,
    }
}

pub async fn discover() -> BackendResult<Discover> {
    let new_and_approved = list_games(None, None, 12).await?.items;
    // The API has no tag listing yet, so offer the tags of recent games.
    let mut tags: Vec<String> = new_and_approved
        .iter()
        .flat_map(|g| g.tags.clone())
        .collect();
    tags.sort();
    tags.dedup();
    Ok(Discover {
        new_and_approved,
        tags,
    })
}

async fn game(slug: &str, token: Option<&str>) -> BackendResult<Game> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().game(slug, token)),
        Backend::Api(api) => api.get(&format!("/games/{slug}"), token, &[]).await,
    }
}

/// Mock data can't do this.
fn mock_unavailable<T>() -> BackendResult<T> {
    Err(BackendError {
        status: 501,
        message: "This needs the real API; the sample data can't do it.".into(),
    })
}

/// A short-lived link to the zip of a channel's current build. Counts as a
/// download.
pub async fn archive(
    slug: &str,
    channel: &str,
    token: Option<&str>,
) -> BackendResult<ArchiveDownload> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => {
            api.call::<(), _>(
                Method::POST,
                &format!("/games/{slug}/channels/{channel}/archive"),
                token,
                &[],
                None,
            )
            .await
        }
    }
}

async fn channels(slug: &str) -> BackendResult<Vec<ChannelStatus>> {
    match &*BACKEND {
        Backend::Mock => Ok(mock::store().channels(slug)),
        Backend::Api(api) => api.get(&format!("/games/{slug}/channels"), None, &[]).await,
    }
}

async fn developer(slug: &str) -> BackendResult<Developer> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().developer(slug)),
        Backend::Api(api) => api.get(&format!("/developers/{slug}"), None, &[]).await,
    }
}

async fn developer_games(slug: &str, limit: usize) -> BackendResult<Page<Game>> {
    match &*BACKEND {
        Backend::Mock => Ok(mock::store().developer_games(slug, limit)),
        Backend::Api(api) => {
            api.get(
                &format!("/developers/{slug}/games"),
                None,
                &[("limit", limit.to_string())],
            )
            .await
        }
    }
}

/// A game with its developer, downloads and a few more games by the same
/// developer. `None` when it doesn't exist or the visitor can't see it.
/// Unpublished games only show up with a tester key (never through the
/// visitor's session, so developers see what players will see).
pub async fn game_page(slug: &str) -> BackendResult<Option<GamePage>> {
    let Some(game) = optional(game(slug, None).await)? else {
        return Ok(None);
    };
    if matches!(game.status, GameStatus::Rejected | GameStatus::Delisted) {
        return Ok(None);
    }
    let dev_slug = game.developer.slug.clone();
    let developer = developer(&dev_slug).await?;
    let downloads = match &*BACKEND {
        // The mock has no zips; show the channels' builds.
        Backend::Mock => Download::from_channels(channels(slug).await?),
        Backend::Api(api) => Download::from_archives(
            api.get(&format!("/games/{slug}/downloads"), None, &[])
                .await?,
        ),
    };
    let more_from_developer = developer_games(&dev_slug, 5)
        .await?
        .items
        .into_iter()
        .filter(|g| g.slug != slug)
        .take(4)
        .collect();
    Ok(Some(GamePage {
        game,
        developer,
        downloads,
        more_from_developer,
    }))
}

pub async fn developer_profile(slug: &str) -> BackendResult<Option<DeveloperProfile>> {
    let Some(developer) = optional(developer(slug).await)? else {
        return Ok(None);
    };
    let games = developer_games(slug, 50).await?.items;
    Ok(Some(DeveloperProfile { developer, games }))
}

// ---- auth --------------------------------------------------------------------

const BAD_LOGIN: &str = "That email or username and password don't match an account.";

/// Returns a session token.
pub async fn login(login: &str, password: &str) -> BackendResult<String> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().login(login, password)).map_err(|mut e| {
            e.message = BAD_LOGIN.into();
            e
        }),
        Backend::Api(api) => {
            let req = LoginRequest {
                login: login.trim().to_string(),
                password: password.to_string(),
            };
            let resp: BackendResult<AuthResponse> =
                api.send(Method::POST, "/auth/login", None, &req).await;
            match resp {
                Ok(r) => Ok(r.token),
                // The API answers a wrong password like a bad token; say what
                // actually happened, and don't let it look like an expired
                // session.
                Err(e) if e.is(StatusCode::UNAUTHORIZED) => Err(BackendError {
                    status: 400,
                    message: BAD_LOGIN.into(),
                }),
                Err(e) => Err(e),
            }
        }
    }
}

/// Returns a session token.
pub async fn register(req: RegisterRequest) -> BackendResult<String> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().register(req)),
        Backend::Api(api) => {
            let resp: AuthResponse = api.send(Method::POST, "/auth/register", None, &req).await?;
            Ok(resp.token)
        }
    }
}

pub async fn logout(token: &str) -> BackendResult<()> {
    match &*BACKEND {
        Backend::Mock => {
            mock::store().logout(token);
            Ok(())
        }
        Backend::Api(api) => {
            api.call::<(), ()>(Method::POST, "/auth/logout", Some(token), &[], None)
                .await
        }
    }
}

/// The signed-in user and their developer profile. `None` when the token is
/// missing, expired or revoked.
pub async fn session(token: &str) -> BackendResult<Option<Session>> {
    let user = match &*BACKEND {
        Backend::Mock => mocked(mock::store().me(token)),
        Backend::Api(api) => api.get::<User>("/me", Some(token), &[]).await,
    };
    let user = match user {
        Ok(u) => u,
        Err(e) if e.is(StatusCode::UNAUTHORIZED) => return Ok(None),
        Err(e) => return Err(e),
    };
    let developer = match &*BACKEND {
        Backend::Mock => mocked(mock::store().me_developer(token)),
        Backend::Api(api) => api.get("/me/developer", Some(token), &[]).await,
    };
    Ok(Some(Session {
        user,
        developer: optional(developer)?,
    }))
}

pub async fn discord_authorize() -> BackendResult<DiscordAuthorize> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => api.get("/auth/discord/authorize", None, &[]).await,
    }
}

/// Returns a session token.
pub async fn discord_callback(code: &str, state: &str) -> BackendResult<String> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => {
            let req = DiscordCallback {
                code: code.to_string(),
                state: state.to_string(),
            };
            let resp: AuthResponse = api
                .send(Method::POST, "/auth/discord/callback", None, &req)
                .await?;
            Ok(resp.token)
        }
    }
}

pub async fn verify_email(token: &str) -> BackendResult<User> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => {
            let req = VerifyEmailRequest {
                token: token.to_string(),
            };
            api.send(Method::POST, "/auth/verify-email", None, &req)
                .await
        }
    }
}

pub async fn resend_verification(token: &str) -> BackendResult<()> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => {
            api.call::<(), ()>(
                Method::POST,
                "/auth/verify-email/resend",
                Some(token),
                &[],
                None,
            )
            .await
        }
    }
}

pub async fn request_password_reset(email: &str) -> BackendResult<()> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => {
            let req = PasswordResetRequest {
                email: email.trim().to_string(),
            };
            api.send(Method::POST, "/auth/password-reset/request", None, &req)
                .await
        }
    }
}

pub async fn reset_password(token: &str, password: &str) -> BackendResult<()> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => {
            let req = PasswordReset {
                token: token.to_string(),
                password: password.to_string(),
            };
            api.send(Method::POST, "/auth/password-reset", None, &req)
                .await
        }
    }
}

// ---- staff review ----------------------------------------------------------------

/// Open submissions, oldest first.
pub async fn review_queue(token: &str) -> BackendResult<Vec<QueueItem>> {
    match &*BACKEND {
        Backend::Mock => Ok(Vec::new()),
        Backend::Api(api) => {
            let page: Page<QueueItem> = api
                .get("/review/queue", Some(token), &[("limit", "100".into())])
                .await?;
            Ok(page.items)
        }
    }
}

/// Everything a reviewer needs to judge one game.
pub async fn review_game(token: &str, slug: &str) -> BackendResult<ReviewGame> {
    let Backend::Api(api) = &*BACKEND else {
        return mock_unavailable();
    };
    let game: Game = api.get(&format!("/games/{slug}"), Some(token), &[]).await?;
    let submissions = submissions(token, slug).await?;
    let builds: Vec<Build> = api
        .get(&format!("/games/{slug}/builds"), Some(token), &[])
        .await?;
    let downloads: Vec<GameDownload> = api
        .get(&format!("/games/{slug}/downloads"), Some(token), &[])
        .await?;
    let developer = api
        .get(
            &format!("/developers/{}", game.developer.slug),
            Some(token),
            &[],
        )
        .await
        .ok();
    Ok(ReviewGame {
        game,
        developer,
        submissions,
        builds,
        downloads,
    })
}

pub async fn claim(token: &str, submission: &str) -> BackendResult<Submission> {
    review_post(
        token,
        &format!("/review/submissions/{submission}/claim"),
        &(),
    )
    .await
}

pub async fn release(token: &str, submission: &str) -> BackendResult<Submission> {
    review_post(
        token,
        &format!("/review/submissions/{submission}/release"),
        &(),
    )
    .await
}

pub async fn decide(
    token: &str,
    submission: &str,
    decision: &Decision,
) -> BackendResult<Submission> {
    review_post(
        token,
        &format!("/review/submissions/{submission}/decision"),
        decision,
    )
    .await
}

async fn review_post<B: Serialize>(token: &str, path: &str, body: &B) -> BackendResult<Submission> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => api.send(Method::POST, path, Some(token), body).await,
    }
}

// ---- developer dashboard -------------------------------------------------------

pub async fn create_developer(token: &str, req: CreateDeveloper) -> BackendResult<Developer> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().create_developer(token, req)),
        Backend::Api(api) => {
            api.send(Method::POST, "/developers", Some(token), &req)
                .await
        }
    }
}

async fn submissions(token: &str, slug: &str) -> BackendResult<Vec<Submission>> {
    let mut subs: Vec<Submission> = match &*BACKEND {
        Backend::Mock => mocked(mock::store().submissions(token, slug))?,
        Backend::Api(api) => {
            api.get(&format!("/games/{slug}/submissions"), Some(token), &[])
                .await?
        }
    };
    subs.sort_by_key(|x| std::cmp::Reverse(x.created_at));
    Ok(subs)
}

pub async fn my_games(token: &str) -> BackendResult<Vec<DashboardGame>> {
    let games: Vec<Game> = match &*BACKEND {
        Backend::Mock => mocked(mock::store().my_games(token))?,
        Backend::Api(api) => api.get("/me/games", Some(token), &[]).await?,
    };
    let mut out = Vec::with_capacity(games.len());
    for game in games {
        let submissions = submissions(token, &game.slug).await?;
        out.push(DashboardGame { game, submissions });
    }
    out.sort_by_key(|x| std::cmp::Reverse(x.game.updated_at));
    Ok(out)
}

pub async fn my_game(token: &str, slug: &str) -> BackendResult<DashboardGame> {
    let game = game(slug, Some(token)).await?;
    let submissions = submissions(token, slug).await?;
    Ok(DashboardGame { game, submissions })
}

pub async fn create_game(token: &str, req: CreateGame) -> BackendResult<Game> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().create_game(token, req)),
        Backend::Api(api) => api.send(Method::POST, "/games", Some(token), &req).await,
    }
}

pub async fn update_game(token: &str, slug: &str, req: UpdateGame) -> BackendResult<Game> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().update_game(token, slug, req)),
        Backend::Api(api) => {
            api.send(Method::PATCH, &format!("/games/{slug}"), Some(token), &req)
                .await
        }
    }
}

pub async fn submit_for_review(
    token: &str,
    slug: &str,
    req: CreateSubmission,
) -> BackendResult<Submission> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().create_submission(token, slug, req)),
        Backend::Api(api) => {
            api.send(
                Method::POST,
                &format!("/games/{slug}/submissions"),
                Some(token),
                &req,
            )
            .await
        }
    }
}

// ---- developer tooling -----------------------------------------------------

pub async fn api_tokens(token: &str) -> BackendResult<Vec<ApiToken>> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().api_tokens(token)),
        Backend::Api(api) => api.get("/auth/tokens", Some(token), &[]).await,
    }
}

pub async fn create_api_token(token: &str, name: &str) -> BackendResult<CreatedApiToken> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().create_api_token(token, name)),
        Backend::Api(api) => {
            api.send(
                Method::POST,
                "/auth/tokens",
                Some(token),
                &CreateApiTokenRequest { name: name.into() },
            )
            .await
        }
    }
}

pub async fn delete_api_token(token: &str, id: uuid::Uuid) -> BackendResult<()> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().delete_api_token(token, id)),
        Backend::Api(api) => {
            api.call::<(), _>(
                Method::DELETE,
                &format!("/auth/tokens/{id}"),
                Some(token),
                &[],
                None,
            )
            .await
        }
    }
}

pub async fn game_stats(token: &str, slug: &str, days: i64) -> BackendResult<GameStats> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().stats(token, slug, days)),
        Backend::Api(api) => {
            api.get(
                &format!("/games/{slug}/stats"),
                Some(token),
                &[("days", days.to_string())],
            )
            .await
        }
    }
}

/// Every channel of the developer's game, private ones included.
pub async fn my_channels(token: &str, slug: &str) -> BackendResult<Vec<ChannelStatus>> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().my_channels(token, slug)),
        Backend::Api(api) => {
            api.get(&format!("/games/{slug}/channels"), Some(token), &[])
                .await
        }
    }
}

pub async fn set_channel_private(
    token: &str,
    slug: &str,
    channel: &str,
    private: bool,
) -> BackendResult<ChannelStatus> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().set_channel_private(token, slug, channel, private)),
        Backend::Api(api) => {
            api.send(
                Method::PATCH,
                &format!("/games/{slug}/channels/{channel}"),
                Some(token),
                &UpdateChannel { private },
            )
            .await
        }
    }
}

pub async fn tester_keys(token: &str, slug: &str) -> BackendResult<Vec<TesterKey>> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().tester_keys(token, slug)),
        Backend::Api(api) => {
            api.get(&format!("/games/{slug}/tester-keys"), Some(token), &[])
                .await
        }
    }
}

pub async fn create_tester_key(
    token: &str,
    slug: &str,
    req: CreateTesterKey,
) -> BackendResult<CreatedTesterKey> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().create_tester_key(token, slug, req)),
        Backend::Api(api) => {
            api.send(
                Method::POST,
                &format!("/games/{slug}/tester-keys"),
                Some(token),
                &req,
            )
            .await
        }
    }
}

pub async fn delete_tester_key(token: &str, slug: &str, id: uuid::Uuid) -> BackendResult<()> {
    match &*BACKEND {
        Backend::Mock => mocked(mock::store().delete_tester_key(token, slug, id)),
        Backend::Api(api) => {
            api.call::<(), _>(
                Method::DELETE,
                &format!("/games/{slug}/tester-keys/{id}"),
                Some(token),
                &[],
                None,
            )
            .await
        }
    }
}

/// Which game a tester key unlocks.
pub async fn redeem_tester_key(key: &str) -> BackendResult<TesterAccess> {
    match &*BACKEND {
        Backend::Mock => mock_unavailable(),
        Backend::Api(api) => {
            api.send(
                Method::POST,
                "/tester-keys/redeem",
                None,
                &RedeemTesterKey { key: key.into() },
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalizes_api_messages() {
        assert_eq!(
            capitalize("add a description before submitting"),
            "Add a description before submitting."
        );
        assert_eq!(capitalize("Done!"), "Done!");
        assert_eq!(capitalize(""), "");
    }
}
