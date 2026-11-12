//! In-memory stand-in for the Foyer API.
//!
//! Server only, and only used when `FOYER_WEB_MOCK=1`: handy for working on
//! pages without running the API, and it ships sample games so there is
//! something to look at. Each method mirrors one API endpoint, takes and
//! returns `foyer-types` shapes, and follows the API's rules closely enough
//! that the pages behave the same against either.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

use crate::model::*;

/// The account the sign-in page offers in mock mode. It owns a developer
/// profile with games in every review state.
pub use crate::{DEMO_EMAIL, DEMO_PASSWORD};

struct Account {
    user: User,
    /// Mock only: the real API hashes passwords with Argon2.
    password: String,
    developer_slug: Option<String>,
}

pub struct Store {
    developers: Vec<Developer>,
    games: Vec<Game>,
    channels: HashMap<String, Vec<ChannelStatus>>,
    submissions: Vec<Submission>,
    accounts: Vec<Account>,
    /// Session token to user id.
    sessions: HashMap<String, Uuid>,
    /// User, game slug, when it was added.
    wishlist: Vec<(Uuid, String, DateTime<Utc>)>,
    /// User, developer slug, when they followed.
    follows: Vec<(Uuid, String, DateTime<Utc>)>,
    /// Users who turned release emails on.
    release_emails: HashSet<Uuid>,
    /// API tokens by owner.
    api_tokens: Vec<(Uuid, ApiToken)>,
    /// Tester keys by game slug.
    tester_keys: HashMap<String, Vec<TesterKey>>,
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| Mutex::new(seed()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1000);

pub fn store() -> MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

fn new_id() -> Uuid {
    Uuid::from_u128(u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed)))
}

fn new_token() -> String {
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    blake3::hash(format!("{nanos}:{n}").as_bytes())
        .to_hex()
        .to_string()
}

fn date(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, 12, 0, 0)
        .single()
        .unwrap_or_default()
}

/// Errors carry the same messages the API would send.
pub type MockResult<T> = Result<T, String>;

fn not_found<T>() -> MockResult<T> {
    Err("Not found.".into())
}

fn check_slug(field: &str, slug: &str) -> MockResult<()> {
    let ok = (2..=40).contains(&slug.len())
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{field} must be 2 to 40 lowercase letters, digits or dashes"
        ))
    }
}

fn check_links(links: &[SupportLink]) -> MockResult<()> {
    if links.iter().all(|l| is_https_url(&l.url)) {
        Ok(())
    } else {
        Err("support links must be https:// URLs".into())
    }
}

fn check_tags(tags: &[String]) -> MockResult<()> {
    if tags.len() > MAX_TAGS {
        return Err("use at most 10 tags".into());
    }
    Ok(())
}

impl Store {
    // ---- public catalog -------------------------------------------------

    fn published(&self) -> impl Iterator<Item = &Game> {
        self.games
            .iter()
            .filter(|g| g.status == GameStatus::Published)
    }

    /// `GET /v1/games?tag=&q=&limit=`
    pub fn list_games(&self, tag: Option<&str>, q: Option<&str>, limit: usize) -> Page<Game> {
        let q = q.map(str::to_lowercase);
        let mut items: Vec<Game> = self
            .published()
            .filter(|g| tag.is_none_or(|t| g.tags.iter().any(|x| x == t)))
            .filter(|g| {
                q.as_deref().is_none_or(|q| {
                    g.title.to_lowercase().contains(q)
                        || g.tagline.to_lowercase().contains(q)
                        || g.developer.display_name.to_lowercase().contains(q)
                        || g.tags.iter().any(|t| t.contains(q))
                })
            })
            .cloned()
            .collect();
        items.sort_by_key(|x| std::cmp::Reverse(x.published_at));
        items.truncate(limit);
        Page {
            items,
            next_cursor: None,
        }
    }

    /// `GET /v1/games/{slug}`: published games for everyone, any status for
    /// the owner.
    pub fn game(&self, slug: &str, token: Option<&str>) -> MockResult<Game> {
        let game = self
            .games
            .iter()
            .find(|g| g.slug == slug)
            .ok_or("Not found.")?;
        let owner = token
            .and_then(|t| self.account(t))
            .and_then(|a| a.developer_slug.as_deref())
            == Some(game.developer.slug.as_str());
        if game.status == GameStatus::Published || owner {
            Ok(game.clone())
        } else {
            not_found()
        }
    }

    /// `GET /v1/games/{slug}/channels`
    pub fn channels(&self, slug: &str) -> Vec<ChannelStatus> {
        self.channels.get(slug).cloned().unwrap_or_default()
    }

    /// `GET /v1/developers/{slug}`
    pub fn developer(&self, slug: &str) -> MockResult<Developer> {
        self.developers
            .iter()
            .find(|d| d.slug == slug)
            .cloned()
            .ok_or_else(|| "Not found.".into())
    }

    /// `GET /v1/developers/{slug}/games`
    pub fn developer_games(&self, slug: &str, limit: usize) -> Page<Game> {
        let mut items: Vec<Game> = self
            .published()
            .filter(|g| g.developer.slug == slug)
            .cloned()
            .collect();
        items.sort_by_key(|x| std::cmp::Reverse(x.published_at));
        items.truncate(limit);
        Page {
            items,
            next_cursor: None,
        }
    }

    // ---- auth -----------------------------------------------------------

    /// `POST /v1/auth/login`. Returns a session token.
    pub fn login(&mut self, login: &str, password: &str) -> MockResult<String> {
        let login = login.trim().to_lowercase();
        let account = self
            .accounts
            .iter()
            .find(|a| (a.user.email == login || a.user.username == login) && a.password == password)
            .ok_or("invalid login or password")?;
        let token = new_token();
        self.sessions.insert(token.clone(), account.user.id);
        Ok(token)
    }

    /// `POST /v1/auth/register`. Returns a session token.
    pub fn register(&mut self, req: RegisterRequest) -> MockResult<String> {
        let email = req.email.trim().to_lowercase();
        let username = req.username.trim().to_string();
        let username_ok = (3..=32).contains(&username.len())
            && username
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !username_ok {
            return Err("username must be 3 to 32 lowercase letters, digits, - or _".into());
        }
        if !email.contains('@') {
            return Err("enter a valid email address".into());
        }
        if req.password.chars().count() < 10 {
            return Err("password must be at least 10 characters".into());
        }
        if self
            .accounts
            .iter()
            .any(|a| a.user.email == email || a.user.username == username)
        {
            return Err("an account with that email or username already exists".into());
        }
        let user = User {
            id: new_id(),
            username,
            email,
            role: Role::User,
            email_verified: true,
            created_at: Utc::now(),
        };
        let token = new_token();
        self.sessions.insert(token.clone(), user.id);
        self.accounts.push(Account {
            user,
            password: req.password,
            developer_slug: None,
        });
        Ok(token)
    }

    /// `POST /v1/auth/logout`
    pub fn logout(&mut self, token: &str) {
        self.sessions.remove(token);
    }

    fn account(&self, token: &str) -> Option<&Account> {
        let user_id = self.sessions.get(token)?;
        self.accounts.iter().find(|a| &a.user.id == user_id)
    }

    /// `GET /v1/me`
    pub fn me(&self, token: &str) -> MockResult<User> {
        self.account(token)
            .map(|a| a.user.clone())
            .ok_or_else(|| "Sign in to continue.".into())
    }

    /// `GET /v1/me/developer`
    pub fn me_developer(&self, token: &str) -> MockResult<Developer> {
        let slug = self
            .account(token)
            .and_then(|a| a.developer_slug.clone())
            .ok_or("Not found.")?;
        self.developer(&slug)
    }

    fn my_developer_slug(&self, token: &str) -> MockResult<String> {
        self.account(token)
            .ok_or("Sign in to continue.")?
            .developer_slug
            .clone()
            .ok_or_else(|| "create a developer profile first".into())
    }

    fn my_game_mut(&mut self, token: &str, slug: &str) -> MockResult<&mut Game> {
        let dev = self.my_developer_slug(token)?;
        self.games
            .iter_mut()
            .find(|g| g.slug == slug && g.developer.slug == dev)
            .ok_or_else(|| "Not found.".into())
    }

    // ---- developer tooling ---------------------------------------------

    fn user_id(&self, token: &str) -> MockResult<Uuid> {
        self.sessions
            .get(token)
            .copied()
            .ok_or_else(|| "Sign in to continue.".into())
    }

    /// The developer's own game, read-only.
    fn my_game(&self, token: &str, slug: &str) -> MockResult<&Game> {
        let dev = self.my_developer_slug(token)?;
        self.games
            .iter()
            .find(|g| g.slug == slug && g.developer.slug == dev)
            .ok_or_else(|| "Not found.".into())
    }

    /// `GET /v1/auth/tokens`
    pub fn api_tokens(&self, token: &str) -> MockResult<Vec<ApiToken>> {
        let user = self.user_id(token)?;
        Ok(self
            .api_tokens
            .iter()
            .filter(|(owner, _)| *owner == user)
            .map(|(_, t)| t.clone())
            .collect())
    }

    /// `POST /v1/auth/tokens`
    pub fn create_api_token(&mut self, token: &str, name: &str) -> MockResult<CreatedApiToken> {
        let user = self.user_id(token)?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err("name must be 1 to 60 characters".into());
        }
        let info = ApiToken {
            id: new_id(),
            name: name.into(),
            created_at: Utc::now(),
            last_used_at: None,
        };
        self.api_tokens.push((user, info.clone()));
        Ok(CreatedApiToken {
            token: format!("fyp_{}", new_token()),
            info,
        })
    }

    /// `DELETE /v1/auth/tokens/{id}`
    pub fn delete_api_token(&mut self, token: &str, id: Uuid) -> MockResult<()> {
        let user = self.user_id(token)?;
        let before = self.api_tokens.len();
        self.api_tokens
            .retain(|(owner, t)| !(*owner == user && t.id == id));
        if self.api_tokens.len() == before {
            return not_found();
        }
        Ok(())
    }

    /// `GET /v1/games/{slug}/stats`: made-up but stable numbers for
    /// published games, nothing for the rest.
    pub fn stats(&self, token: &str, slug: &str, days: i64) -> MockResult<GameStats> {
        let game = self.my_game(token, slug)?;
        let days = days.clamp(1, 365);
        let today = Utc::now().date_naive();
        let seed = u64::from(placeholder_hue(slug));
        let published = game.status == GameStatus::Published;
        let mut total = DownloadCounts::default();
        let mut platforms: Vec<PlatformDownloads> = game
            .platforms
            .iter()
            .map(|p| PlatformDownloads {
                platform: *p,
                counts: DownloadCounts::default(),
            })
            .collect();
        let daily = (0..days)
            .map(|i| {
                let day = today - chrono::Duration::days(days - 1 - i);
                let n = if published {
                    // A launch spike that tails off, with a weekly wobble.
                    let age = (days - i) as u64;
                    40 * 30 / (age + 10) + (seed + age * 7) % 9
                } else {
                    0
                };
                let counts = DownloadCounts {
                    web: n / 2,
                    client_installs: n / 3,
                    client_updates: n - n / 2 - n / 3,
                };
                total.web += counts.web;
                total.client_installs += counts.client_installs;
                total.client_updates += counts.client_updates;
                if let Some(first) = platforms.first_mut() {
                    first.counts.web += counts.web;
                    first.counts.client_installs += counts.client_installs;
                    first.counts.client_updates += counts.client_updates;
                }
                DailyDownloads { day, counts }
            })
            .collect();
        Ok(GameStats {
            total,
            platforms,
            daily,
        })
    }

    /// `GET /v1/games/{slug}/channels` as the developer sees it.
    pub fn my_channels(&self, token: &str, slug: &str) -> MockResult<Vec<ChannelStatus>> {
        self.my_game(token, slug)?;
        Ok(self.channels(slug))
    }

    /// `PATCH /v1/games/{slug}/channels/{channel}`
    pub fn set_channel_private(
        &mut self,
        token: &str,
        slug: &str,
        channel: &str,
        private: bool,
    ) -> MockResult<ChannelStatus> {
        self.my_game(token, slug)?;
        let found = self
            .channels
            .get_mut(slug)
            .and_then(|list| list.iter_mut().find(|c| c.channel == channel))
            .ok_or("Not found.")?;
        found.private = private;
        Ok(found.clone())
    }

    /// `GET /v1/games/{slug}/tester-keys`
    pub fn tester_keys(&self, token: &str, slug: &str) -> MockResult<Vec<TesterKey>> {
        self.my_game(token, slug)?;
        Ok(self.tester_keys.get(slug).cloned().unwrap_or_default())
    }

    /// `POST /v1/games/{slug}/tester-keys`
    pub fn create_tester_key(
        &mut self,
        token: &str,
        slug: &str,
        req: CreateTesterKey,
    ) -> MockResult<CreatedTesterKey> {
        self.my_game(token, slug)?;
        let days = req.expires_in_days.unwrap_or(30);
        if !(1..=365).contains(&days) {
            return Err("expires_in_days must be 1 to 365".into());
        }
        let secret = format!("fyk_{}", new_token());
        let key = TesterKey {
            id: new_id(),
            label: req.label.trim().into(),
            channels: req.channels,
            prefix: secret.chars().take(10).collect(),
            created_at: Utc::now(),
            expires_at: Some(Utc::now() + chrono::Duration::days(i64::from(days))),
            last_used_at: None,
        };
        self.tester_keys
            .entry(slug.into())
            .or_default()
            .insert(0, key.clone());
        Ok(CreatedTesterKey {
            link: format!("http://127.0.0.1:3000/tester?key={secret}"),
            key,
            secret,
        })
    }

    /// `DELETE /v1/games/{slug}/tester-keys/{id}`
    pub fn delete_tester_key(&mut self, token: &str, slug: &str, id: Uuid) -> MockResult<()> {
        self.my_game(token, slug)?;
        let keys = self.tester_keys.entry(slug.into()).or_default();
        let before = keys.len();
        keys.retain(|k| k.id != id);
        if keys.len() == before {
            return not_found();
        }
        Ok(())
    }

    // ---- developer dashboard -------------------------------------------

    /// `POST /v1/developers`
    pub fn create_developer(&mut self, token: &str, req: CreateDeveloper) -> MockResult<Developer> {
        let user_id = *self.sessions.get(token).ok_or("Sign in to continue.")?;
        check_slug("slug", &req.slug)?;
        if req.display_name.trim().is_empty() {
            return Err("display_name is required".into());
        }
        if req.website_url.as_deref().is_some_and(|u| !is_https_url(u)) {
            return Err("website_url must be an https:// URL".into());
        }
        check_links(&req.support_links)?;
        if self.developers.iter().any(|d| d.slug == req.slug) {
            return Err("that developer slug is taken".into());
        }
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.user.id == user_id)
            .ok_or("Sign in to continue.")?;
        if account.developer_slug.is_some() {
            return Err("you already have a developer profile".into());
        }
        account.developer_slug = Some(req.slug.clone());
        let developer = Developer {
            id: new_id(),
            slug: req.slug,
            display_name: req.display_name.trim().into(),
            bio: req.bio.trim().into(),
            website_url: req.website_url,
            support_links: req.support_links,
            created_at: Utc::now(),
        };
        self.developers.push(developer.clone());
        Ok(developer)
    }

    /// `GET /v1/me/games`
    pub fn my_games(&self, token: &str) -> MockResult<Vec<Game>> {
        let slug = self.my_developer_slug(token)?;
        Ok(self
            .games
            .iter()
            .filter(|g| g.developer.slug == slug)
            .cloned()
            .collect())
    }

    /// `GET /v1/games/{slug}/submissions`, newest first.
    pub fn submissions(&self, token: &str, slug: &str) -> MockResult<Vec<Submission>> {
        let dev = self.my_developer_slug(token)?;
        if !self
            .games
            .iter()
            .any(|g| g.slug == slug && g.developer.slug == dev)
        {
            return not_found();
        }
        let mut subs: Vec<Submission> = self
            .submissions
            .iter()
            .filter(|s| s.game == slug)
            .cloned()
            .collect();
        subs.sort_by_key(|x| std::cmp::Reverse(x.created_at));
        Ok(subs)
    }

    /// `POST /v1/games`
    pub fn create_game(&mut self, token: &str, req: CreateGame) -> MockResult<Game> {
        let dev_slug = self.my_developer_slug(token)?;
        let developer = self.developer(&dev_slug)?;
        check_slug("slug", &req.slug)?;
        if req.title.trim().is_empty() {
            return Err("title is required".into());
        }
        check_tags(&req.tags)?;
        check_links(&req.support_links)?;
        if self.games.iter().any(|g| g.slug == req.slug) {
            return Err("that game slug is taken".into());
        }
        let now = Utc::now();
        let game = Game {
            id: new_id(),
            slug: req.slug,
            developer: DeveloperSummary {
                slug: developer.slug,
                display_name: developer.display_name,
            },
            title: req.title.trim().into(),
            tagline: req.tagline.trim().into(),
            description: req.description.trim().into(),
            tags: req.tags,
            content_descriptors: req.content_descriptors,
            ai_disclosure: req.ai_disclosure.filter(|s| !s.trim().is_empty()),
            support_links: req.support_links,
            platforms: vec![],
            media: vec![],
            status: GameStatus::Draft,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.games.push(game.clone());
        Ok(game)
    }

    /// `PATCH /v1/games/{slug}`
    pub fn update_game(&mut self, token: &str, slug: &str, req: UpdateGame) -> MockResult<Game> {
        if let Some(tags) = &req.tags {
            check_tags(tags)?;
        }
        if let Some(links) = &req.support_links {
            check_links(links)?;
        }
        let game = self.my_game_mut(token, slug)?;
        if game.status == GameStatus::InReview {
            return Err("a game can't be edited while it's in review".into());
        }
        if let Some(v) = req.title {
            if v.trim().is_empty() {
                return Err("title is required".into());
            }
            game.title = v.trim().into();
        }
        if let Some(v) = req.tagline {
            game.tagline = v.trim().into();
        }
        if let Some(v) = req.description {
            game.description = v.trim().into();
        }
        if let Some(v) = req.tags {
            game.tags = v;
        }
        if let Some(v) = req.content_descriptors {
            game.content_descriptors = v;
        }
        if let Some(v) = req.ai_disclosure {
            game.ai_disclosure = Some(v.trim().to_string()).filter(|s| !s.is_empty());
        }
        if let Some(v) = req.support_links {
            game.support_links = v;
        }
        game.updated_at = Utc::now();
        Ok(game.clone())
    }

    /// `POST /v1/games/{slug}/submissions`
    pub fn create_submission(
        &mut self,
        token: &str,
        slug: &str,
        req: CreateSubmission,
    ) -> MockResult<Submission> {
        let game = self.my_game_mut(token, slug)?;
        if !matches!(
            game.status,
            GameStatus::Draft | GameStatus::ChangesRequested
        ) {
            return Err(format!(
                "a game that is {} can't be submitted",
                game.status.as_str().replace('_', " ")
            ));
        }
        if game.description.trim().is_empty() {
            return Err("add a description before submitting".into());
        }
        if game.platforms.is_empty() {
            return Err("push at least one build before submitting".into());
        }
        game.status = GameStatus::InReview;
        game.updated_at = Utc::now();
        let submission = Submission {
            id: new_id(),
            game: slug.into(),
            kind: SubmissionKind::Review,
            status: SubmissionStatus::Pending,
            developer_notes: req.developer_notes.trim().into(),
            reviewer: None,
            decision_notes: None,
            rule: None,
            created_at: Utc::now(),
            decided_at: None,
        };
        self.submissions.push(submission.clone());
        Ok(submission)
    }

    // ---- players ---------------------------------------------------------

    /// `GET /v1/me/wishlist`, newest first.
    pub fn wishlist(&self, token: &str) -> MockResult<Vec<WishlistEntry>> {
        let user = self.user_id(token)?;
        let mut entries: Vec<WishlistEntry> = self
            .wishlist
            .iter()
            .filter(|(u, _, _)| *u == user)
            .filter_map(|(_, slug, added_at)| {
                let game = self.published().find(|g| &g.slug == slug)?.clone();
                Some(WishlistEntry {
                    game,
                    added_at: *added_at,
                })
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.added_at));
        Ok(entries)
    }

    /// `PUT` or `DELETE /v1/me/wishlist/{slug}`.
    pub fn set_wishlisted(&mut self, token: &str, slug: &str, on: bool) -> MockResult<()> {
        let user = self.user_id(token)?;
        if !self.published().any(|g| g.slug == slug) {
            return not_found();
        }
        let has = self
            .wishlist
            .iter()
            .any(|(u, s, _)| *u == user && s == slug);
        if on && !has {
            self.wishlist.push((user, slug.to_string(), Utc::now()));
        } else if !on {
            self.wishlist.retain(|(u, s, _)| !(*u == user && s == slug));
        }
        Ok(())
    }

    /// `GET /v1/me/following`, newest first.
    pub fn following(&self, token: &str) -> MockResult<Vec<Follow>> {
        let user = self.user_id(token)?;
        let mut follows: Vec<Follow> = self
            .follows
            .iter()
            .filter(|(u, _, _)| *u == user)
            .filter_map(|(_, slug, at)| {
                let dev = self.developers.iter().find(|d| &d.slug == slug)?;
                Some(Follow {
                    developer: DeveloperSummary {
                        slug: dev.slug.clone(),
                        display_name: dev.display_name.clone(),
                    },
                    followed_at: *at,
                })
            })
            .collect();
        follows.sort_by_key(|f| std::cmp::Reverse(f.followed_at));
        Ok(follows)
    }

    /// `PUT` or `DELETE /v1/me/following/{slug}`.
    pub fn set_following(&mut self, token: &str, slug: &str, on: bool) -> MockResult<()> {
        let user = self.user_id(token)?;
        if !self.developers.iter().any(|d| d.slug == slug) {
            return not_found();
        }
        if on
            && self
                .account(token)
                .and_then(|a| a.developer_slug.as_deref())
                == Some(slug)
        {
            return Err("you can't follow your own profile".into());
        }
        let has = self.follows.iter().any(|(u, s, _)| *u == user && s == slug);
        if on && !has {
            self.follows.push((user, slug.to_string(), Utc::now()));
        } else if !on {
            self.follows.retain(|(u, s, _)| !(*u == user && s == slug));
        }
        Ok(())
    }

    /// `GET /v1/me/settings`
    pub fn settings(&self, token: &str) -> MockResult<AccountSettings> {
        let user = self.user_id(token)?;
        Ok(AccountSettings {
            release_emails: self.release_emails.contains(&user),
        })
    }

    /// `PATCH /v1/me/settings`
    pub fn set_release_emails(&mut self, token: &str, on: bool) -> MockResult<AccountSettings> {
        let user = self.user_id(token)?;
        if on {
            self.release_emails.insert(user);
        } else {
            self.release_emails.remove(&user);
        }
        self.settings(token)
    }
}

// ---- seed data -----------------------------------------------------------

fn link(kind: SupportKind, url: &str) -> SupportLink {
    SupportLink {
        kind,
        url: url.into(),
        label: None,
    }
}

fn developer(slug: &str, name: &str, bio: &str, links: Vec<SupportLink>) -> Developer {
    Developer {
        id: new_id(),
        slug: slug.into(),
        display_name: name.into(),
        bio: bio.into(),
        website_url: None,
        support_links: links,
        created_at: date(2026, 6, 1),
    }
}

struct Seed<'a> {
    slug: &'a str,
    title: &'a str,
    tagline: &'a str,
    description: &'a str,
    developer: &'a Developer,
    tags: &'a [&'a str],
    platforms: &'a [Platform],
    published: Option<DateTime<Utc>>,
}

fn game(s: Seed) -> Game {
    let ts = s.published.unwrap_or_else(|| date(2026, 10, 20));
    Game {
        id: new_id(),
        slug: s.slug.into(),
        developer: DeveloperSummary {
            slug: s.developer.slug.clone(),
            display_name: s.developer.display_name.clone(),
        },
        title: s.title.into(),
        tagline: s.tagline.into(),
        description: s.description.into(),
        tags: s.tags.iter().map(|t| t.to_string()).collect(),
        content_descriptors: vec![],
        ai_disclosure: None,
        support_links: vec![],
        platforms: s.platforms.to_vec(),
        // Empty URLs: the pages draw placeholder art for these.
        media: (0..4)
            .map(|i| Media {
                id: new_id(),
                kind: MediaKind::Screenshot,
                url: String::new(),
                position: i,
            })
            .collect(),
        status: if s.published.is_some() {
            GameStatus::Published
        } else {
            GameStatus::Draft
        },
        published_at: s.published,
        created_at: ts,
        updated_at: ts,
    }
}

fn seed() -> Store {
    use Platform::*;

    let lantern = developer(
        "lantern-works",
        "Lantern Works",
        "Two people, one cat, and a fondness for small worlds you can hold in your head.",
        vec![link(SupportKind::Patreon, "https://www.patreon.com/")],
    );
    let tidepool = developer(
        "tidepool",
        "Tidepool Games",
        "We make slow, strange puzzle games about the sea.",
        vec![link(SupportKind::Kofi, "https://ko-fi.com/")],
    );
    let night = developer(
        "night-shift",
        "Night Shift Collective",
        "A loose group of game jam regulars who keep finishing the things we start.",
        vec![
            link(SupportKind::Patreon, "https://www.patreon.com/"),
            link(SupportKind::Kofi, "https://ko-fi.com/"),
        ],
    );

    let mut games = vec![
        game(Seed {
            slug: "moss-and-lanterns",
            title: "Moss & Lanterns",
            tagline: "Relight a forest one shrine at a time.",
            description: "A gentle exploration game about a keeper who wakes in an overgrown forest. Find the old shrines, solve the light puzzles around them, and watch the forest come back to life.\n\nNo combat and no timers. About four hours to the end, longer if you chase every lantern.",
            developer: &lantern,
            tags: &["exploration", "puzzle", "cozy"],
            platforms: &[Windows, Macos, Linux],
            published: Some(date(2026, 9, 12)),
        }),
        game(Seed {
            slug: "brine",
            title: "Brine",
            tagline: "A salvage puzzler at the bottom of the sea.",
            description: "Pilot a tiny submersible through sunken wrecks. Every room is a puzzle about pressure, buoyancy and the cable that keeps you tied to the surface.",
            developer: &tidepool,
            tags: &["puzzle", "atmospheric"],
            platforms: &[Windows, Linux],
            published: Some(date(2026, 7, 30)),
        }),
        game(Seed {
            slug: "last-train-to-umber",
            title: "Last Train to Umber",
            tagline: "Keep the night train running. Keep the passengers alive.",
            description: "A management game on rails. Juggle coal, water and a carriage full of passengers with their own problems as you cross a continent that is slowly going dark.",
            developer: &night,
            tags: &["strategy", "management", "roguelite"],
            platforms: &[Windows, Macos, Linux],
            published: Some(date(2026, 10, 2)),
        }),
        game(Seed {
            slug: "kelp-garden",
            title: "Kelp Garden",
            tagline: "Grow a reef, feed a village.",
            description: "A small farming game set in a coastal village where the fields are underwater.",
            developer: &tidepool,
            tags: &["cozy", "farming"],
            platforms: &[Windows, Macos],
            published: Some(date(2026, 10, 21)),
        }),
        game(Seed {
            slug: "signal-fire",
            title: "Signal Fire",
            tagline: "A twin-stick shooter about holding the line.",
            description: "Defend a hilltop beacon through the night against waves of shadow. Short runs, lots of upgrades, one more go.",
            developer: &night,
            tags: &["action", "roguelite", "shooter"],
            platforms: &[Windows, Linux],
            published: Some(date(2026, 8, 18)),
        }),
        game(Seed {
            slug: "paper-orbit",
            title: "Paper Orbit",
            tagline: "Fold paper, launch satellites.",
            description: "A physics puzzle game where every level is a sheet of paper you fold into the right shape to reach orbit.",
            developer: &lantern,
            tags: &["puzzle", "physics"],
            platforms: &[Windows, Macos, Linux],
            published: Some(date(2026, 11, 1)),
        }),
        // Lantern Works' unreleased games, one per review state, so the
        // dashboard has something to show.
        game(Seed {
            slug: "hollow-bell",
            title: "Hollow Bell",
            tagline: "A rhythm game played on a broken church bell.",
            description: "",
            developer: &lantern,
            tags: &["rhythm"],
            platforms: &[Windows],
            published: None,
        }),
        game(Seed {
            slug: "quiet-harbour",
            title: "Quiet Harbour",
            tagline: "Run a lighthouse on a stormy coast.",
            description: "Keep the light burning, log the ships and find out what happened to the last keeper.",
            developer: &lantern,
            tags: &["narrative", "management"],
            platforms: &[Windows, Linux],
            published: None,
        }),
        game(Seed {
            slug: "tin-choir",
            title: "Tin Choir",
            tagline: "Build a robot choir from scrap.",
            description: "A sandbox about wiring together little singing robots.",
            developer: &lantern,
            tags: &["sandbox", "music"],
            platforms: &[Windows, Macos, Linux],
            published: None,
        }),
    ];

    games[0].content_descriptors = vec![ContentDescriptor::Horror];
    games[0].support_links = vec![link(SupportKind::Kofi, "https://ko-fi.com/")];
    games[2].content_descriptors =
        vec![ContentDescriptor::Violence, ContentDescriptor::MatureThemes];
    games[4].content_descriptors = vec![ContentDescriptor::Violence];
    games[6].media.truncate(1);
    games[7].status = GameStatus::ChangesRequested;
    games[7].media.truncate(2);
    games[8].status = GameStatus::InReview;

    let channels = games
        .iter()
        .map(|g| {
            let size = 120 + u64::from(placeholder_hue(&g.slug)) * 4;
            let list = g
                .platforms
                .iter()
                .map(|p| ChannelStatus {
                    channel: p.as_str().into(),
                    private: false,
                    platform: *p,
                    latest: Some(Build {
                        id: new_id(),
                        game: g.slug.clone(),
                        channel: p.as_str().into(),
                        platform: *p,
                        version: Some("1.0.2".into()),
                        state: BuildState::Ready,
                        total_size: size * 1024 * 1024,
                        chunk_count: 1,
                        created_at: g.created_at,
                        launch: None,
                        scan: None,
                        failure: None,
                    }),
                })
                .collect();
            (g.slug.clone(), list)
        })
        .collect();

    let reviewer = UserSummary {
        id: new_id(),
        username: "torsten".into(),
    };
    let submissions = vec![
        Submission {
            id: new_id(),
            game: "quiet-harbour".into(),
            kind: SubmissionKind::Review,
            status: SubmissionStatus::ChangesRequested,
            developer_notes: "First submission. Linux build is new, Windows is well tested.".into(),
            reviewer: Some(reviewer),
            decision_notes: Some("Lovely game. The page needs at least 3 real screenshots, and the Linux build crashed on launch for us. Fix both and resubmit any time.".into()),
            rule: Some(PolicyRule::QualityPage),
            created_at: date(2026, 10, 28),
            decided_at: Some(date(2026, 11, 3)),
        },
        Submission {
            id: new_id(),
            game: "tin-choir".into(),
            kind: SubmissionKind::Review,
            status: SubmissionStatus::Pending,
            developer_notes: String::new(),
            reviewer: None,
            decision_notes: None,
            rule: None,
            created_at: date(2026, 11, 5),
            decided_at: None,
        },
    ];

    let accounts = vec![Account {
        user: User {
            id: new_id(),
            username: "demo".into(),
            email: DEMO_EMAIL.into(),
            role: Role::User,
            email_verified: true,
            created_at: date(2026, 6, 1),
        },
        password: DEMO_PASSWORD.into(),
        developer_slug: Some(lantern.slug.clone()),
    }];

    Store {
        developers: vec![lantern, tidepool, night],
        games,
        channels,
        submissions,
        accounts,
        sessions: HashMap::new(),
        wishlist: Vec::new(),
        follows: Vec::new(),
        release_emails: HashSet::new(),
        api_tokens: Vec::new(),
        tester_keys: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(slug: &str) -> CreateGame {
        CreateGame {
            slug: slug.into(),
            title: "Gearbox".into(),
            tagline: "A test.".into(),
            description: "Something to play.".into(),
            tags: vec!["puzzle".into()],
            content_descriptors: vec![],
            ai_disclosure: None,
            support_links: vec![],
        }
    }

    fn register(store: &mut Store, name: &str) -> String {
        store
            .register(RegisterRequest {
                username: name.into(),
                email: format!("{name}@example.com"),
                password: "long-enough-password".into(),
            })
            .unwrap()
    }

    fn profile(slug: &str) -> CreateDeveloper {
        CreateDeveloper {
            slug: slug.into(),
            display_name: slug.into(),
            bio: String::new(),
            website_url: None,
            support_links: vec![],
        }
    }

    #[test]
    fn catalog_only_shows_published_games() {
        let store = seed();
        let page = store.list_games(None, None, 100);
        assert!(page.items.iter().all(|g| g.status == GameStatus::Published));
        assert!(store.game("hollow-bell", None).is_err());
        assert!(store.game("brine", None).is_ok());
    }

    #[test]
    fn owners_see_their_drafts() {
        let mut store = seed();
        let token = store.login(DEMO_EMAIL, DEMO_PASSWORD).unwrap();
        assert!(store.game("hollow-bell", Some(&token)).is_ok());
    }

    #[test]
    fn filters_by_tag_and_query() {
        let store = seed();
        assert_eq!(store.list_games(Some("puzzle"), None, 100).items.len(), 3);
        let brine = store.list_games(None, Some("SALVAGE"), 100).items;
        assert_eq!(brine.len(), 1);
        assert_eq!(brine[0].slug, "brine");
    }

    #[test]
    fn create_and_submit_flow() {
        let mut store = seed();
        let token = register(&mut store, "ada");
        assert!(store.create_game(&token, create("gearbox")).is_err());
        store
            .create_developer(&token, profile("ada-games"))
            .unwrap();
        let game = store.create_game(&token, create("gearbox")).unwrap();
        assert_eq!(game.status, GameStatus::Draft);

        // Like the API, a game needs a ready build before review.
        let err = store
            .create_submission(&token, "gearbox", CreateSubmission::default())
            .unwrap_err();
        assert!(err.contains("build"));

        store
            .games
            .iter_mut()
            .find(|g| g.slug == "gearbox")
            .unwrap()
            .platforms = vec![Platform::Linux];
        store
            .create_submission(&token, "gearbox", CreateSubmission::default())
            .unwrap();
        assert_eq!(
            store.game("gearbox", Some(&token)).unwrap().status,
            GameStatus::InReview
        );
        assert_eq!(store.submissions(&token, "gearbox").unwrap().len(), 1);
        assert!(store.game("gearbox", None).is_err());
        assert!(
            store
                .update_game(&token, "gearbox", UpdateGame::default())
                .is_err()
        );
    }

    #[test]
    fn developers_cannot_touch_other_games() {
        let mut store = seed();
        let token = register(&mut store, "eve");
        store.create_developer(&token, profile("eve")).unwrap();
        assert!(store.submissions(&token, "hollow-bell").is_err());
        assert!(
            store
                .create_submission(&token, "hollow-bell", CreateSubmission::default())
                .is_err()
        );
    }

    #[test]
    fn rejects_non_https_urls() {
        let mut store = seed();
        let token = register(&mut store, "mal");
        let mut bad = profile("mal");
        bad.website_url = Some("javascript:alert(1)".into());
        assert!(store.create_developer(&token, bad).is_err());
        store.create_developer(&token, profile("mal")).unwrap();
        let mut game = create("trap");
        game.support_links = vec![link(SupportKind::Website, "data:text/html,hi")];
        assert!(store.create_game(&token, game).is_err());
    }

    #[test]
    fn wishlist_and_follows() {
        let mut store = seed();
        let token = register(&mut store, "player");
        let slug = store.list_games(None, None, 1).items[0].slug.clone();
        store.set_wishlisted(&token, &slug, true).unwrap();
        store.set_wishlisted(&token, &slug, true).unwrap();
        assert_eq!(store.wishlist(&token).unwrap().len(), 1);
        store.set_wishlisted(&token, &slug, false).unwrap();
        assert!(store.wishlist(&token).unwrap().is_empty());
        assert!(store.set_wishlisted(&token, "nope", true).is_err());

        store.set_following(&token, "lantern-works", true).unwrap();
        assert_eq!(
            store.following(&token).unwrap()[0].developer.display_name,
            "Lantern Works"
        );
        assert!(!store.settings(&token).unwrap().release_emails);
        assert!(
            store
                .set_release_emails(&token, true)
                .unwrap()
                .release_emails
        );
    }

    #[test]
    fn login_checks_password() {
        let mut store = seed();
        assert!(store.login(DEMO_EMAIL, "wrong").is_err());
        let token = store.login("demo", DEMO_PASSWORD).unwrap();
        assert_eq!(store.me_developer(&token).unwrap().slug, "lantern-works");
        store.logout(&token);
        assert!(store.me(&token).is_err());
    }
}
