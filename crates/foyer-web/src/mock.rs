//! In-memory stand-in for the Foyer API, used until the real API lands.
//!
//! Server only. State lives for the life of the process, so accounts and games
//! created in the dashboard survive page loads but not restarts. Each public
//! method maps to one API endpoint, so swapping to the real client is a matter
//! of replacing bodies in `api.rs`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::*;

/// The account the sign-in page offers for trying things out. It owns a
/// developer profile with games in every review state.
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
    downloads: HashMap<String, Vec<Download>>,
    submissions: Vec<Submission>,
    accounts: Vec<Account>,
    /// Session token to user id.
    sessions: HashMap<String, String>,
    staff_picks: Vec<String>,
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| Mutex::new(seed()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1000);

pub fn store() -> MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

fn new_id() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("00000000-0000-4000-8000-{n:012}")
}

fn now() -> String {
    // Good enough for mock data: a date with a fixed time.
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T12:00:00Z")
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn new_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    blake3::hash(format!("{nanos}:{n}").as_bytes())
        .to_hex()
        .to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Not found.")]
    NotFound,
    #[error("That email and password don't match an account.")]
    BadCredentials,
    #[error("An account with that email already exists.")]
    EmailTaken,
    #[error("You need a developer profile first.")]
    NoDeveloper,
    #[error("This game can't be changed right now.")]
    NotEditable,
    #[error("{0}")]
    Invalid(String),
}

pub struct GameInput {
    pub title: String,
    pub tagline: String,
    pub description: String,
    pub tags: Vec<String>,
    pub content_descriptors: Vec<ContentDescriptor>,
    pub ai_disclosure: Option<String>,
    pub platforms: Vec<Platform>,
    pub support_links: Vec<SupportLink>,
}

impl GameInput {
    fn validate(&self) -> Result<(), StoreError> {
        let invalid = |m: &str| Err(StoreError::Invalid(m.into()));
        if self.title.trim().is_empty() {
            return invalid("Give the game a title.");
        }
        if self.title.chars().count() > 80 {
            return invalid("Keep the title under 80 characters.");
        }
        if self.tagline.chars().count() > 140 {
            return invalid("Keep the tagline under 140 characters.");
        }
        if self.tags.len() > MAX_TAGS {
            return invalid("Use at most 10 tags.");
        }
        for link in &self.support_links {
            if !is_http_url(&link.url) {
                return invalid("Support links must start with https://.");
            }
        }
        Ok(())
    }
}

impl Store {
    // ---- public catalog -------------------------------------------------

    fn published(&self) -> impl Iterator<Item = &Game> {
        self.games
            .iter()
            .filter(|g| g.status == GameStatus::Published)
    }

    /// `GET /v1/games?tag=&q=`
    pub fn list_games(&self, tag: Option<&str>, q: Option<&str>) -> Page<Game> {
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
        items.sort_by(|a, b| b.published_at.cmp(&a.published_at));
        Page {
            items,
            next_cursor: None,
        }
    }

    pub fn discover(&self) -> Discover {
        let staff_picks = self
            .staff_picks
            .iter()
            .filter_map(|s| self.published().find(|g| &g.slug == s))
            .cloned()
            .collect();
        let new_and_approved = self
            .list_games(None, None)
            .items
            .into_iter()
            .take(6)
            .collect();
        let mut tags: Vec<String> = self.published().flat_map(|g| g.tags.clone()).collect();
        tags.sort();
        tags.dedup();
        Discover {
            staff_picks,
            new_and_approved,
            tags,
        }
    }

    /// `GET /v1/games/{slug}`, plus the developer and downloads.
    pub fn game_page(&self, slug: &str) -> Option<GamePage> {
        let game = self.published().find(|g| g.slug == slug)?.clone();
        let developer = self.developer(&game.developer.slug)?.clone();
        let more_from_developer = self
            .published()
            .filter(|g| g.developer.slug == developer.slug && g.slug != slug)
            .cloned()
            .collect();
        Some(GamePage {
            downloads: self.downloads.get(slug).cloned().unwrap_or_default(),
            game,
            developer,
            more_from_developer,
        })
    }

    fn developer(&self, slug: &str) -> Option<&Developer> {
        self.developers.iter().find(|d| d.slug == slug)
    }

    /// `GET /v1/developers/{slug}` and `GET /v1/developers/{slug}/games`.
    pub fn developer_profile(&self, slug: &str) -> Option<DeveloperProfile> {
        let developer = self.developer(slug)?.clone();
        let games = self
            .published()
            .filter(|g| g.developer.slug == slug)
            .cloned()
            .collect();
        Some(DeveloperProfile { developer, games })
    }

    // ---- auth -----------------------------------------------------------

    /// `POST /v1/auth/login`. Returns a session token.
    pub fn login(&mut self, email: &str, password: &str) -> Result<String, StoreError> {
        let email = email.trim().to_lowercase();
        let account = self
            .accounts
            .iter()
            .find(|a| a.user.email == email && a.password == password)
            .ok_or(StoreError::BadCredentials)?;
        let token = new_token();
        self.sessions.insert(token.clone(), account.user.id.clone());
        Ok(token)
    }

    /// `POST /v1/auth/register`. Returns a session token.
    pub fn register(
        &mut self,
        username: &str,
        email: &str,
        password: &str,
    ) -> Result<String, StoreError> {
        let email = email.trim().to_lowercase();
        let username = username.trim();
        if username.is_empty() {
            return Err(StoreError::Invalid("Pick a username.".into()));
        }
        if !email.contains('@') {
            return Err(StoreError::Invalid("Enter a valid email address.".into()));
        }
        if password.chars().count() < 8 {
            return Err(StoreError::Invalid(
                "Use a password of at least 8 characters.".into(),
            ));
        }
        if self.accounts.iter().any(|a| a.user.email == email) {
            return Err(StoreError::EmailTaken);
        }
        let user = User {
            id: new_id(),
            username: username.into(),
            email,
            role: Role::User,
            created_at: now(),
        };
        let token = new_token();
        self.sessions.insert(token.clone(), user.id.clone());
        self.accounts.push(Account {
            user,
            password: password.into(),
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
    pub fn me(&self, token: &str) -> Option<Session> {
        let account = self.account(token)?;
        Some(Session {
            user: account.user.clone(),
            developer: account
                .developer_slug
                .as_deref()
                .and_then(|s| self.developer(s))
                .cloned(),
        })
    }

    fn my_developer_slug(&self, token: &str) -> Result<String, StoreError> {
        self.account(token)
            .ok_or(StoreError::NotFound)?
            .developer_slug
            .clone()
            .ok_or(StoreError::NoDeveloper)
    }

    // ---- developer dashboard -------------------------------------------

    /// Creates the signed-in account's developer profile.
    pub fn create_developer(
        &mut self,
        token: &str,
        display_name: &str,
        bio: &str,
        website_url: Option<String>,
    ) -> Result<Developer, StoreError> {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(StoreError::Invalid(
                "Enter a studio or developer name.".into(),
            ));
        }
        if website_url.as_deref().is_some_and(|u| !is_http_url(u)) {
            return Err(StoreError::Invalid(
                "Your website must start with https://.".into(),
            ));
        }
        let user_id = self
            .sessions
            .get(token)
            .cloned()
            .ok_or(StoreError::NotFound)?;
        let base = match slugify(display_name) {
            s if s.is_empty() => "developer".to_string(),
            s => s,
        };
        let slug = unique(&base, |s| self.developer(s).is_some());
        let developer = Developer {
            id: new_id(),
            slug: slug.clone(),
            display_name: display_name.into(),
            bio: bio.trim().into(),
            website_url,
            support_links: vec![],
            created_at: now(),
        };
        self.developers.push(developer.clone());
        if let Some(a) = self.accounts.iter_mut().find(|a| a.user.id == user_id) {
            a.developer_slug = Some(slug);
        }
        Ok(developer)
    }

    /// `GET /v1/me/games`
    pub fn my_games(&self, token: &str) -> Result<Vec<DashboardGame>, StoreError> {
        let slug = self.my_developer_slug(token)?;
        Ok(self
            .games
            .iter()
            .filter(|g| g.developer.slug == slug)
            .map(|g| DashboardGame {
                game: g.clone(),
                submissions: self.submissions_for(&g.slug),
            })
            .collect())
    }

    /// One of the signed-in developer's games, any status, with its
    /// submissions (`GET /v1/games/{slug}/submissions`).
    pub fn my_game(&self, token: &str, slug: &str) -> Result<DashboardGame, StoreError> {
        let dev = self.my_developer_slug(token)?;
        let game = self
            .games
            .iter()
            .find(|g| g.slug == slug && g.developer.slug == dev)
            .ok_or(StoreError::NotFound)?;
        Ok(DashboardGame {
            game: game.clone(),
            submissions: self.submissions_for(slug),
        })
    }

    fn submissions_for(&self, slug: &str) -> Vec<Submission> {
        let mut subs: Vec<Submission> = self
            .submissions
            .iter()
            .filter(|s| s.game == slug)
            .cloned()
            .collect();
        subs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        subs
    }

    /// `POST /v1/games`. Returns the new game's slug.
    pub fn create_game(&mut self, token: &str, input: GameInput) -> Result<String, StoreError> {
        input.validate()?;
        let dev_slug = self.my_developer_slug(token)?;
        let developer = self
            .developer(&dev_slug)
            .ok_or(StoreError::NoDeveloper)?
            .summary();
        let base = match slugify(&input.title) {
            s if s.is_empty() => "game".to_string(),
            s => s,
        };
        let slug = unique(&base, |s| self.games.iter().any(|g| g.slug == s));
        let ts = now();
        self.games.push(Game {
            id: new_id(),
            slug: slug.clone(),
            developer,
            title: input.title.trim().into(),
            tagline: input.tagline.trim().into(),
            description: input.description.trim().into(),
            tags: input.tags,
            content_descriptors: input.content_descriptors,
            ai_disclosure: input.ai_disclosure,
            support_links: input.support_links,
            platforms: input.platforms,
            media: vec![],
            status: GameStatus::Draft,
            published_at: None,
            created_at: ts.clone(),
            updated_at: ts,
        });
        Ok(slug)
    }

    /// `PATCH /v1/games/{slug}`
    pub fn update_game(
        &mut self,
        token: &str,
        slug: &str,
        input: GameInput,
    ) -> Result<(), StoreError> {
        input.validate()?;
        let dev = self.my_developer_slug(token)?;
        let game = self
            .games
            .iter_mut()
            .find(|g| g.slug == slug && g.developer.slug == dev)
            .ok_or(StoreError::NotFound)?;
        if !game.status.is_editable() {
            return Err(StoreError::NotEditable);
        }
        game.title = input.title.trim().into();
        game.tagline = input.tagline.trim().into();
        game.description = input.description.trim().into();
        game.tags = input.tags;
        game.content_descriptors = input.content_descriptors;
        game.ai_disclosure = input.ai_disclosure;
        game.platforms = input.platforms;
        game.support_links = input.support_links;
        game.updated_at = now();
        Ok(())
    }

    /// `POST /v1/games/{slug}/submissions`
    pub fn submit_for_review(
        &mut self,
        token: &str,
        slug: &str,
        developer_notes: &str,
    ) -> Result<(), StoreError> {
        let dev = self.my_developer_slug(token)?;
        let game = self
            .games
            .iter_mut()
            .find(|g| g.slug == slug && g.developer.slug == dev)
            .ok_or(StoreError::NotFound)?;
        if !game.status.can_submit() {
            return Err(StoreError::NotEditable);
        }
        if game.description.trim().is_empty() {
            return Err(StoreError::Invalid(
                "Add a description before submitting.".into(),
            ));
        }
        if game.platforms.is_empty() {
            return Err(StoreError::Invalid(
                "Pick at least one platform before submitting.".into(),
            ));
        }
        game.status = GameStatus::InReview;
        game.updated_at = now();
        self.submissions.push(Submission {
            id: new_id(),
            game: slug.into(),
            status: SubmissionStatus::Pending,
            developer_notes: developer_notes.trim().into(),
            reviewer: None,
            decision_notes: None,
            rule: None,
            created_at: now(),
            decided_at: None,
        });
        Ok(())
    }
}

fn is_http_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut slug = base.to_string();
    let mut n = 2;
    while taken(&slug) {
        slug = format!("{base}-{n}");
        n += 1;
    }
    slug
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
        created_at: "2026-06-01T12:00:00Z".into(),
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
    published: Option<&'a str>,
}

fn game(s: Seed) -> Game {
    let ts = s
        .published
        .map(|d| format!("{d}T12:00:00Z"))
        .unwrap_or_else(|| "2026-10-20T12:00:00Z".into());
    Game {
        id: new_id(),
        slug: s.slug.into(),
        developer: s.developer.summary(),
        title: s.title.into(),
        tagline: s.tagline.into(),
        description: s.description.into(),
        tags: s.tags.iter().map(|t| t.to_string()).collect(),
        content_descriptors: vec![],
        ai_disclosure: None,
        support_links: vec![],
        platforms: s.platforms.to_vec(),
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
        published_at: s.published.map(|_| ts.clone()),
        created_at: ts.clone(),
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
            published: Some("2026-09-12"),
        }),
        game(Seed {
            slug: "brine",
            title: "Brine",
            tagline: "A salvage puzzler at the bottom of the sea.",
            description: "Pilot a tiny submersible through sunken wrecks. Every room is a puzzle about pressure, buoyancy and the cable that keeps you tied to the surface.",
            developer: &tidepool,
            tags: &["puzzle", "atmospheric"],
            platforms: &[Windows, Linux],
            published: Some("2026-07-30"),
        }),
        game(Seed {
            slug: "last-train-to-umber",
            title: "Last Train to Umber",
            tagline: "Keep the night train running. Keep the passengers alive.",
            description: "A management game on rails. Juggle coal, water and a carriage full of passengers with their own problems as you cross a continent that is slowly going dark.",
            developer: &night,
            tags: &["strategy", "management", "roguelite"],
            platforms: &[Windows, Macos, Linux],
            published: Some("2026-10-02"),
        }),
        game(Seed {
            slug: "kelp-garden",
            title: "Kelp Garden",
            tagline: "Grow a reef, feed a village.",
            description: "A small farming game set in a coastal village where the fields are underwater.",
            developer: &tidepool,
            tags: &["cozy", "farming"],
            platforms: &[Windows, Macos],
            published: Some("2026-10-21"),
        }),
        game(Seed {
            slug: "signal-fire",
            title: "Signal Fire",
            tagline: "A twin-stick shooter about holding the line.",
            description: "Defend a hilltop beacon through the night against waves of shadow. Short runs, lots of upgrades, one more go.",
            developer: &night,
            tags: &["action", "roguelite", "shooter"],
            platforms: &[Windows, Linux],
            published: Some("2026-08-18"),
        }),
        game(Seed {
            slug: "paper-orbit",
            title: "Paper Orbit",
            tagline: "Fold paper, launch satellites.",
            description: "A physics puzzle game where every level is a sheet of paper you fold into the right shape to reach orbit.",
            developer: &lantern,
            tags: &["puzzle", "physics"],
            platforms: &[Windows, Macos, Linux],
            published: Some("2026-11-01"),
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

    let downloads = games
        .iter()
        .filter(|g| g.status == GameStatus::Published)
        .map(|g| {
            let size = 120 + u64::from(placeholder_hue(&g.slug)) * 4;
            let dls = g
                .platforms
                .iter()
                .map(|p| Download {
                    platform: *p,
                    version: "1.0.2".into(),
                    size_bytes: size * 1024 * 1024,
                })
                .collect();
            (g.slug.clone(), dls)
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
            status: SubmissionStatus::ChangesRequested,
            developer_notes: "First submission. Linux build is new, Windows is well tested.".into(),
            reviewer: Some(reviewer),
            decision_notes: Some("Lovely game. The page needs at least 3 real screenshots, and the Linux build crashed on launch for us. Fix both and resubmit any time.".into()),
            rule: Some(PolicyRule::QualityPage),
            created_at: "2026-10-28T09:00:00Z".into(),
            decided_at: Some("2026-11-03T15:30:00Z".into()),
        },
        Submission {
            id: new_id(),
            game: "tin-choir".into(),
            status: SubmissionStatus::Pending,
            developer_notes: String::new(),
            reviewer: None,
            decision_notes: None,
            rule: None,
            created_at: "2026-11-05T10:00:00Z".into(),
            decided_at: None,
        },
    ];

    let accounts = vec![Account {
        user: User {
            id: new_id(),
            username: "demo".into(),
            email: DEMO_EMAIL.into(),
            role: Role::User,
            created_at: "2026-06-01T12:00:00Z".into(),
        },
        password: DEMO_PASSWORD.into(),
        developer_slug: Some(lantern.slug.clone()),
    }];

    Store {
        developers: vec![lantern, tidepool, night],
        games,
        downloads,
        submissions,
        accounts,
        sessions: HashMap::new(),
        staff_picks: vec![
            "moss-and-lanterns".into(),
            "last-train-to-umber".into(),
            "brine".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(title: &str) -> GameInput {
        GameInput {
            title: title.into(),
            tagline: "A test.".into(),
            description: "Something to play.".into(),
            tags: vec!["puzzle".into()],
            content_descriptors: vec![],
            ai_disclosure: None,
            platforms: vec![Platform::Linux],
            support_links: vec![],
        }
    }

    #[test]
    fn catalog_only_shows_published_games() {
        let store = seed();
        let page = store.list_games(None, None);
        assert!(page.items.iter().all(|g| g.status == GameStatus::Published));
        assert!(store.game_page("hollow-bell").is_none());
        assert!(store.game_page("brine").is_some());
    }

    #[test]
    fn filters_by_tag_and_query() {
        let store = seed();
        let puzzles = store.list_games(Some("puzzle"), None).items;
        assert_eq!(puzzles.len(), 3);
        let brine = store.list_games(None, Some("SALVAGE")).items;
        assert_eq!(brine.len(), 1);
        assert_eq!(brine[0].slug, "brine");
    }

    #[test]
    fn create_and_submit_flow() {
        let mut store = seed();
        let token = store
            .register("ada", "ada@example.com", "hunter22!")
            .unwrap();
        assert!(matches!(
            store.create_game(&token, input("Gearbox")),
            Err(StoreError::NoDeveloper)
        ));
        store
            .create_developer(&token, "Ada Games", "", None)
            .unwrap();
        let slug = store.create_game(&token, input("Gearbox")).unwrap();
        assert_eq!(slug, "gearbox");
        assert_eq!(
            store.my_game(&token, &slug).unwrap().game.status,
            GameStatus::Draft
        );

        store.submit_for_review(&token, &slug, "hi").unwrap();
        let game = store.my_game(&token, &slug).unwrap();
        assert_eq!(game.game.status, GameStatus::InReview);
        assert_eq!(game.submissions.len(), 1);
        // Not public until a reviewer approves it.
        assert!(store.game_page(&slug).is_none());
        // And locked while in review.
        assert!(
            store
                .update_game(&token, &slug, input("Gearbox 2"))
                .is_err()
        );
    }

    #[test]
    fn rejects_non_http_urls() {
        let mut store = seed();
        let token = store
            .register("mal", "mal@example.com", "password1")
            .unwrap();
        let bad = Some("javascript:alert(1)".to_string());
        assert!(store.create_developer(&token, "Mal", "", bad).is_err());
        store.create_developer(&token, "Mal", "", None).unwrap();
        let mut game = input("Trap");
        game.support_links = vec![SupportLink {
            kind: SupportKind::Website,
            url: "data:text/html,hi".into(),
            label: None,
        }];
        assert!(store.create_game(&token, game).is_err());
    }

    #[test]
    fn developers_cannot_touch_other_games() {
        let mut store = seed();
        let token = store
            .register("eve", "eve@example.com", "password1")
            .unwrap();
        store.create_developer(&token, "Eve", "", None).unwrap();
        assert!(store.my_game(&token, "hollow-bell").is_err());
        assert!(store.submit_for_review(&token, "hollow-bell", "").is_err());
    }

    #[test]
    fn login_checks_password() {
        let mut store = seed();
        assert!(store.login(DEMO_EMAIL, "wrong").is_err());
        let token = store.login(DEMO_EMAIL, DEMO_PASSWORD).unwrap();
        let me = store.me(&token).unwrap();
        assert_eq!(me.developer.unwrap().slug, "lantern-works");
        store.logout(&token);
        assert!(store.me(&token).is_none());
    }

    #[test]
    fn civil_date() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_398), (2025, 11, 6));
    }
}
