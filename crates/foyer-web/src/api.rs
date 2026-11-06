//! Server functions: the website's only path to data.
//!
//! Pages call these from resources and forms. On the server they run directly;
//! in the browser they become HTTP calls to the website's server, which calls
//! the Foyer API (see `backend.rs`) with the visitor's session token. The
//! token lives in an `HttpOnly` cookie and never reaches browser code.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::model::*;

#[cfg(feature = "ssr")]
mod session {
    use axum::http::{HeaderMap, header};
    use leptos::prelude::*;

    use crate::routes::{SESSION_COOKIE, SESSION_MAX_AGE, cookie, set_cookie};

    pub async fn token() -> Option<String> {
        let headers: HeaderMap = leptos_axum::extract().await.ok()?;
        cookie(&headers, SESSION_COOKIE)
    }

    pub async fn require_token() -> Result<String, ServerFnError> {
        token()
            .await
            .ok_or_else(|| ServerFnError::new("Sign in to continue."))
    }

    fn set(value: &str, max_age: u32) {
        let response = expect_context::<leptos_axum::ResponseOptions>();
        if let Some(v) = set_cookie(SESSION_COOKIE, value, "/", max_age) {
            response.append_header(header::SET_COOKIE, v);
        }
    }

    pub fn start(token: &str) {
        set(token, SESSION_MAX_AGE);
    }

    pub fn clear() {
        set("", 0);
    }
}

#[cfg(feature = "ssr")]
fn err(e: crate::backend::BackendError) -> ServerFnError {
    if e.status == 401 {
        session::clear();
    }
    ServerFnError::new(e.message)
}

/// Only relative paths, so a crafted `next` can't send people off-site.
/// Browsers treat `\\` like `/`, so `/\\evil.com` would also leave the site.
#[cfg(any(feature = "ssr", test))]
fn safe_next(next: Option<String>, fallback: &str) -> String {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') => n,
        _ => fallback.to_string(),
    }
}

/// Uses `explicit` if given, otherwise a slug made from `fallback`.
#[cfg(any(feature = "ssr", test))]
fn pick_slug(explicit: Option<&str>, fallback: &str) -> String {
    match explicit.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => slugify(fallback),
    }
}

// ---- public catalog -------------------------------------------------------

#[server]
pub async fn get_discover() -> Result<Discover, ServerFnError> {
    crate::backend::discover().await.map_err(err)
}

#[server]
pub async fn list_games(
    tag: Option<String>,
    q: Option<String>,
) -> Result<Page<Game>, ServerFnError> {
    let tag = tag.filter(|t| !t.is_empty());
    let q = q.map(|q| q.trim().to_string()).filter(|q| !q.is_empty());
    crate::backend::list_games(tag.as_deref(), q.as_deref(), 48)
        .await
        .map_err(err)
}

#[server]
pub async fn get_game_page(slug: String) -> Result<Option<GamePage>, ServerFnError> {
    crate::backend::game_page(&slug).await.map_err(err)
}

#[server]
pub async fn get_developer(slug: String) -> Result<Option<DeveloperProfile>, ServerFnError> {
    crate::backend::developer_profile(&slug).await.map_err(err)
}

// ---- auth -----------------------------------------------------------------

/// Whether the site runs on mock data, so the sign-in page can offer the
/// demo account.
#[server]
pub async fn is_mock() -> Result<bool, ServerFnError> {
    Ok(crate::backend::is_mock())
}

#[server]
pub async fn get_session() -> Result<Option<Session>, ServerFnError> {
    let Some(token) = session::token().await else {
        return Ok(None);
    };
    let s = crate::backend::session(&token).await.map_err(err)?;
    if s.is_none() {
        // Expired or revoked: drop the cookie.
        session::clear();
    }
    Ok(s)
}

#[server]
pub async fn sign_in(
    login: String,
    password: String,
    next: Option<String>,
) -> Result<(), ServerFnError> {
    let token = crate::backend::login(&login, &password)
        .await
        .map_err(err)?;
    session::start(&token);
    leptos_axum::redirect(&safe_next(next, "/dashboard"));
    Ok(())
}

#[server]
pub async fn register(
    username: String,
    email: String,
    password: String,
) -> Result<(), ServerFnError> {
    let token = crate::backend::register(RegisterRequest {
        username: username.trim().to_lowercase(),
        email: email.trim().to_string(),
        password,
    })
    .await
    .map_err(err)?;
    session::start(&token);
    leptos_axum::redirect("/dashboard");
    Ok(())
}

#[server]
pub async fn sign_out() -> Result<(), ServerFnError> {
    if let Some(token) = session::token().await {
        // Signing out locally matters more than telling the API.
        let _ = crate::backend::logout(&token).await;
    }
    session::clear();
    leptos_axum::redirect("/");
    Ok(())
}

/// Confirms the email address from the link in the verification email.
#[server]
pub async fn verify_email(token: String) -> Result<(), ServerFnError> {
    crate::backend::verify_email(&token).await.map_err(err)?;
    Ok(())
}

#[server]
pub async fn resend_verification() -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    crate::backend::resend_verification(&token)
        .await
        .map_err(err)
}

/// Always succeeds for well-formed input, so nobody can probe which emails
/// have accounts.
#[server]
pub async fn request_password_reset(email: String) -> Result<(), ServerFnError> {
    crate::backend::request_password_reset(&email)
        .await
        .map_err(err)
}

#[server]
pub async fn reset_password(token: String, password: String) -> Result<(), ServerFnError> {
    crate::backend::reset_password(&token, &password)
        .await
        .map_err(err)?;
    leptos_axum::redirect("/signin?reset=1");
    Ok(())
}

// ---- staff review ---------------------------------------------------------

#[server]
pub async fn review_queue() -> Result<Vec<QueueItem>, ServerFnError> {
    let token = session::require_token().await?;
    crate::backend::review_queue(&token).await.map_err(err)
}

#[server]
pub async fn get_review_game(slug: String) -> Result<ReviewGame, ServerFnError> {
    let token = session::require_token().await?;
    crate::backend::review_game(&token, &slug)
        .await
        .map_err(err)
}

#[server]
pub async fn claim_submission(slug: String, submission: String) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    crate::backend::claim(&token, &submission)
        .await
        .map_err(err)?;
    leptos_axum::redirect(&format!("/review/{slug}"));
    Ok(())
}

#[server]
pub async fn release_submission(submission: String) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    crate::backend::release(&token, &submission)
        .await
        .map_err(err)?;
    leptos_axum::redirect("/review");
    Ok(())
}

/// The decision form. `outcome` and `rule` are the API's snake_case names;
/// an empty `rule` means none.
#[server]
pub async fn decide_submission(
    slug: String,
    submission: String,
    outcome: String,
    notes: String,
    rule: Option<String>,
) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    let decision =
        decision_from_form(&outcome, notes, rule.as_deref()).map_err(ServerFnError::new)?;
    crate::backend::decide(&token, &submission, &decision)
        .await
        .map_err(err)?;
    leptos_axum::redirect(&format!("/review?decided={slug}"));
    Ok(())
}

#[cfg(any(feature = "ssr", test))]
fn decision_from_form(
    outcome: &str,
    notes: String,
    rule: Option<&str>,
) -> Result<Decision, &'static str> {
    let outcome = match outcome {
        "approved" => DecisionOutcome::Approved,
        "changes_requested" => DecisionOutcome::ChangesRequested,
        "rejected" => DecisionOutcome::Rejected,
        _ => return Err("Pick an outcome."),
    };
    let rule = rule.filter(|r| !r.is_empty()).map(PolicyRule::parse);
    let rule = match rule {
        Some(Some(rule)) => Some(rule),
        Some(None) => return Err("Unknown guideline."),
        None => None,
    };
    let notes = notes.trim().to_string();
    if outcome != DecisionOutcome::Approved && notes.is_empty() {
        return Err("Write a note for the developer.");
    }
    if outcome == DecisionOutcome::Rejected && rule.is_none() {
        return Err("A rejection has to cite a guideline.");
    }
    Ok(Decision {
        outcome,
        notes,
        rule,
    })
}

// ---- developer dashboard --------------------------------------------------

#[server]
pub async fn create_developer(
    display_name: String,
    slug: Option<String>,
    bio: String,
    website_url: Option<String>,
) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    let website_url = website_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    let req = foyer_types::CreateDeveloper {
        slug: pick_slug(slug.as_deref(), &display_name),
        display_name: display_name.trim().to_string(),
        bio: bio.trim().to_string(),
        website_url,
        support_links: vec![],
    };
    crate::backend::create_developer(&token, req)
        .await
        .map_err(err)?;
    leptos_axum::redirect("/dashboard");
    Ok(())
}

#[server]
pub async fn my_games() -> Result<Vec<DashboardGame>, ServerFnError> {
    let token = session::require_token().await?;
    crate::backend::my_games(&token).await.map_err(err)
}

#[server]
pub async fn my_game(slug: String) -> Result<DashboardGame, ServerFnError> {
    let token = session::require_token().await?;
    crate::backend::my_game(&token, &slug).await.map_err(err)
}

/// The dashboard's game form, as the browser posts it. Checkboxes arrive only
/// when ticked, so each is its own optional field. Platforms aren't here:
/// the API derives them from the builds a developer pushes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameForm {
    /// Set when editing an existing game.
    pub slug: Option<String>,
    /// The page address for a new game; made from the title when empty.
    pub new_slug: Option<String>,
    pub title: String,
    pub tagline: String,
    pub description: String,
    /// Comma separated.
    pub tags: String,
    pub ai_disclosure: String,
    /// One URL per line; the kind is worked out from the address.
    pub support_links: String,
    pub violence: Option<String>,
    pub blood: Option<String>,
    pub horror: Option<String>,
    pub strong_language: Option<String>,
    pub drugs_alcohol: Option<String>,
    pub mature_themes: Option<String>,
}

/// Guesses what kind of support link a URL is from its host.
pub fn support_link_from_url(url: &str) -> SupportLink {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let kind = if host == "patreon.com" {
        SupportKind::Patreon
    } else if host == "ko-fi.com" {
        SupportKind::Kofi
    } else if host == "github.com" && url.contains("/sponsors/") {
        SupportKind::GithubSponsors
    } else if host == "itch.io" || host.ends_with(".itch.io") {
        SupportKind::Itchio
    } else {
        SupportKind::Website
    };
    SupportLink {
        kind,
        url: url.to_string(),
        label: None,
    }
}

impl GameForm {
    pub fn from_game(game: &Game) -> Self {
        let has = |c| {
            game.content_descriptors
                .contains(&c)
                .then(|| "on".to_string())
        };
        Self {
            slug: Some(game.slug.clone()),
            new_slug: None,
            title: game.title.clone(),
            tagline: game.tagline.clone(),
            description: game.description.clone(),
            tags: game.tags.join(", "),
            ai_disclosure: game.ai_disclosure.clone().unwrap_or_default(),
            support_links: game
                .support_links
                .iter()
                .map(|l| l.url.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            violence: has(ContentDescriptor::Violence),
            blood: has(ContentDescriptor::Blood),
            horror: has(ContentDescriptor::Horror),
            strong_language: has(ContentDescriptor::StrongLanguage),
            drugs_alcohol: has(ContentDescriptor::DrugsAlcohol),
            mature_themes: has(ContentDescriptor::MatureThemes),
        }
    }

    pub fn is_checked(&self, c: ContentDescriptor) -> bool {
        use ContentDescriptor::*;
        match c {
            Violence => self.violence.is_some(),
            Blood => self.blood.is_some(),
            Horror => self.horror.is_some(),
            StrongLanguage => self.strong_language.is_some(),
            DrugsAlcohol => self.drugs_alcohol.is_some(),
            MatureThemes => self.mature_themes.is_some(),
        }
    }

    pub fn content_descriptors(&self) -> Vec<ContentDescriptor> {
        CONTENT_DESCRIPTORS
            .into_iter()
            .filter(|c| self.is_checked(*c))
            .collect()
    }

    pub fn support_links(&self) -> Vec<SupportLink> {
        self.support_links
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(support_link_from_url)
            .collect()
    }

    #[cfg(any(feature = "ssr", test))]
    fn into_create(self) -> CreateGame {
        CreateGame {
            slug: pick_slug(self.new_slug.as_deref(), &self.title),
            content_descriptors: self.content_descriptors(),
            support_links: self.support_links(),
            tags: split_tags(&self.tags),
            ai_disclosure: Some(self.ai_disclosure.trim().to_string()).filter(|s| !s.is_empty()),
            title: self.title.trim().to_string(),
            tagline: self.tagline.trim().to_string(),
            description: self.description.trim().to_string(),
        }
    }

    #[cfg(feature = "ssr")]
    fn into_update(self) -> UpdateGame {
        UpdateGame {
            content_descriptors: Some(self.content_descriptors()),
            support_links: Some(self.support_links()),
            tags: Some(split_tags(&self.tags)),
            // "" clears it.
            ai_disclosure: Some(self.ai_disclosure.trim().to_string()),
            title: Some(self.title.trim().to_string()),
            tagline: Some(self.tagline.trim().to_string()),
            description: Some(self.description.trim().to_string()),
        }
    }
}

#[server]
pub async fn save_game(#[server(flatten)] form: GameForm) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    let slug = match form.slug.clone().filter(|s| !s.is_empty()) {
        Some(slug) => {
            crate::backend::update_game(&token, &slug, form.into_update())
                .await
                .map_err(err)?;
            slug
        }
        None => {
            crate::backend::create_game(&token, form.into_create())
                .await
                .map_err(err)?
                .slug
        }
    };
    leptos_axum::redirect(&format!("/dashboard/games/{slug}?saved=1"));
    Ok(())
}

#[server]
pub async fn submit_for_review(slug: String, notes: String) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    let req = foyer_types::CreateSubmission {
        developer_notes: notes.trim().to_string(),
    };
    crate::backend::submit_for_review(&token, &slug, req)
        .await
        .map_err(err)?;
    leptos_axum::redirect(&format!("/dashboard/games/{slug}?submitted=1"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_links_are_classified_by_host() {
        let kinds: Vec<SupportKind> = [
            "https://www.patreon.com/lantern",
            "https://ko-fi.com/lantern",
            "https://github.com/sponsors/lantern",
            "https://lantern.itch.io/moss",
            "https://lantern.example",
        ]
        .iter()
        .map(|u| support_link_from_url(u).kind)
        .collect();
        assert_eq!(
            kinds,
            vec![
                SupportKind::Patreon,
                SupportKind::Kofi,
                SupportKind::GithubSponsors,
                SupportKind::Itchio,
                SupportKind::Website,
            ]
        );
    }

    #[test]
    fn safe_next_only_allows_same_site_paths() {
        let next = |n: &str| safe_next(Some(n.to_string()), "/fallback");
        assert_eq!(next("/dashboard"), "/dashboard");
        assert_eq!(next("//evil.com"), "/fallback");
        assert_eq!(next("/\\evil.com"), "/fallback");
        assert_eq!(next("https://evil.com"), "/fallback");
    }

    #[test]
    fn form_reads_checkboxes() {
        let form = GameForm {
            title: "Moss & Lanterns".into(),
            horror: Some("on".into()),
            tags: "Puzzle, cozy".into(),
            ..Default::default()
        };
        assert_eq!(form.content_descriptors(), vec![ContentDescriptor::Horror]);
        let create = form.into_create();
        assert_eq!(create.slug, "moss-lanterns");
        assert_eq!(create.tags, vec!["puzzle", "cozy"]);
        assert_eq!(create.ai_disclosure, None);
    }

    #[test]
    fn decision_form_checks_notes_and_rule() {
        assert!(decision_from_form("approved", " ".into(), Some("")).is_ok());
        assert!(decision_from_form("rejected", "no".into(), None).is_err());
        assert!(decision_from_form("changes_requested", "".into(), None).is_err());
        let d = decision_from_form("rejected", " spam ".into(), Some("spam")).unwrap();
        assert_eq!(d.rule, Some(PolicyRule::Spam));
        assert_eq!(d.notes, "spam");
        assert!(decision_from_form("maybe", "x".into(), None).is_err());
    }

    #[test]
    fn explicit_slug_wins() {
        assert_eq!(pick_slug(Some(" my-game "), "Other"), "my-game");
        assert_eq!(pick_slug(Some(""), "Other Title"), "other-title");
    }
}
