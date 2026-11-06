//! Server functions: the website's only path to data.
//!
//! Pages call these from resources and forms. On the server they run directly;
//! in the browser they become HTTP calls to the website's server. Today they
//! read the in-memory mock in `mock.rs`; once the API is live each body becomes
//! a call to it, forwarding the session token, and nothing in the pages has to
//! change.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::model::*;

#[cfg(feature = "ssr")]
mod session {
    use axum::http::{HeaderMap, HeaderValue, header};
    use leptos::prelude::*;

    pub const COOKIE: &str = "foyer_session";

    pub async fn token() -> Option<String> {
        let headers: HeaderMap = leptos_axum::extract().await.ok()?;
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(k, _)| *k == COOKIE)
            .map(|(_, v)| v.to_string())
            .filter(|v| !v.is_empty())
    }

    pub async fn require_token() -> Result<String, ServerFnError> {
        token()
            .await
            .ok_or_else(|| ServerFnError::new("Sign in to continue."))
    }

    fn set(value: &str, max_age: u32) {
        let response = expect_context::<leptos_axum::ResponseOptions>();
        // `Secure` is left off so sign-in works over plain http in local dev;
        // add it when the site is served over https.
        let cookie = format!("{COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}");
        if let Ok(v) = HeaderValue::from_str(&cookie) {
            response.append_header(header::SET_COOKIE, v);
        }
    }

    pub fn start(token: &str) {
        set(token, 60 * 60 * 24 * 30);
    }

    pub fn clear() {
        set("", 0);
    }
}

#[cfg(feature = "ssr")]
fn err(e: crate::mock::StoreError) -> ServerFnError {
    ServerFnError::new(e.to_string())
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

// ---- public catalog -------------------------------------------------------

#[server]
pub async fn get_discover() -> Result<Discover, ServerFnError> {
    Ok(crate::mock::store().discover())
}

#[server]
pub async fn list_games(
    tag: Option<String>,
    q: Option<String>,
) -> Result<Page<Game>, ServerFnError> {
    let tag = tag.filter(|t| !t.is_empty());
    let q = q.map(|q| q.trim().to_string()).filter(|q| !q.is_empty());
    Ok(crate::mock::store().list_games(tag.as_deref(), q.as_deref()))
}

#[server]
pub async fn get_game_page(slug: String) -> Result<Option<GamePage>, ServerFnError> {
    Ok(crate::mock::store().game_page(&slug))
}

#[server]
pub async fn get_developer(slug: String) -> Result<Option<DeveloperProfile>, ServerFnError> {
    Ok(crate::mock::store().developer_profile(&slug))
}

// ---- auth -----------------------------------------------------------------

#[server]
pub async fn get_session() -> Result<Option<Session>, ServerFnError> {
    let Some(token) = session::token().await else {
        return Ok(None);
    };
    Ok(crate::mock::store().me(&token))
}

#[server]
pub async fn sign_in(
    email: String,
    password: String,
    next: Option<String>,
) -> Result<(), ServerFnError> {
    let token = crate::mock::store().login(&email, &password).map_err(err)?;
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
    let token = crate::mock::store()
        .register(&username, &email, &password)
        .map_err(err)?;
    session::start(&token);
    leptos_axum::redirect("/dashboard");
    Ok(())
}

#[server]
pub async fn sign_out() -> Result<(), ServerFnError> {
    if let Some(token) = session::token().await {
        crate::mock::store().logout(&token);
    }
    session::clear();
    leptos_axum::redirect("/");
    Ok(())
}

// ---- developer dashboard --------------------------------------------------

#[server]
pub async fn create_developer(
    display_name: String,
    bio: String,
    website_url: Option<String>,
) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    let website_url = website_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    crate::mock::store()
        .create_developer(&token, &display_name, &bio, website_url)
        .map_err(err)?;
    leptos_axum::redirect("/dashboard");
    Ok(())
}

#[server]
pub async fn my_games() -> Result<Vec<DashboardGame>, ServerFnError> {
    let token = session::require_token().await?;
    crate::mock::store().my_games(&token).map_err(err)
}

#[server]
pub async fn my_game(slug: String) -> Result<DashboardGame, ServerFnError> {
    let token = session::require_token().await?;
    crate::mock::store().my_game(&token, &slug).map_err(err)
}

/// The dashboard's game form, as the browser posts it. Checkboxes arrive only
/// when ticked, so each is its own optional field.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameForm {
    /// Set when editing an existing game.
    pub slug: Option<String>,
    pub title: String,
    pub tagline: String,
    pub description: String,
    /// Comma separated.
    pub tags: String,
    pub ai_disclosure: String,
    /// One URL per line; the kind is worked out from the address.
    pub support_links: String,
    pub windows: Option<String>,
    pub macos: Option<String>,
    pub linux: Option<String>,
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
        let has_p = |p| game.platforms.contains(&p).then(|| "on".to_string());
        let has_c = |c| {
            game.content_descriptors
                .contains(&c)
                .then(|| "on".to_string())
        };
        Self {
            slug: Some(game.slug.clone()),
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
            windows: has_p(Platform::Windows),
            macos: has_p(Platform::Macos),
            linux: has_p(Platform::Linux),
            violence: has_c(ContentDescriptor::Violence),
            blood: has_c(ContentDescriptor::Blood),
            horror: has_c(ContentDescriptor::Horror),
            strong_language: has_c(ContentDescriptor::StrongLanguage),
            drugs_alcohol: has_c(ContentDescriptor::DrugsAlcohol),
            mature_themes: has_c(ContentDescriptor::MatureThemes),
        }
    }

    pub fn platforms(&self) -> Vec<Platform> {
        [
            (Platform::Windows, &self.windows),
            (Platform::Macos, &self.macos),
            (Platform::Linux, &self.linux),
        ]
        .into_iter()
        .filter(|(_, on)| on.is_some())
        .map(|(p, _)| p)
        .collect()
    }

    pub fn content_descriptors(&self) -> Vec<ContentDescriptor> {
        use ContentDescriptor::*;
        [
            (Violence, &self.violence),
            (Blood, &self.blood),
            (Horror, &self.horror),
            (StrongLanguage, &self.strong_language),
            (DrugsAlcohol, &self.drugs_alcohol),
            (MatureThemes, &self.mature_themes),
        ]
        .into_iter()
        .filter(|(_, on)| on.is_some())
        .map(|(c, _)| c)
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
    fn into_input(self) -> crate::mock::GameInput {
        crate::mock::GameInput {
            platforms: self.platforms(),
            content_descriptors: self.content_descriptors(),
            support_links: self.support_links(),
            tags: split_tags(&self.tags),
            ai_disclosure: Some(self.ai_disclosure.trim().to_string()).filter(|s| !s.is_empty()),
            title: self.title,
            tagline: self.tagline,
            description: self.description,
        }
    }
}

#[server]
pub async fn save_game(#[server(flatten)] form: GameForm) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    let slug = form.slug.clone().filter(|s| !s.is_empty());
    let input = form.into_input();
    let mut store = crate::mock::store();
    let slug = match slug {
        Some(slug) => {
            store.update_game(&token, &slug, input).map_err(err)?;
            slug
        }
        None => store.create_game(&token, input).map_err(err)?,
    };
    leptos_axum::redirect(&format!("/dashboard/games/{slug}?saved=1"));
    Ok(())
}

#[server]
pub async fn submit_for_review(slug: String, notes: String) -> Result<(), ServerFnError> {
    let token = session::require_token().await?;
    crate::mock::store()
        .submit_for_review(&token, &slug, &notes)
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
            windows: Some("on".into()),
            linux: Some("on".into()),
            horror: Some("on".into()),
            tags: "Puzzle, cozy".into(),
            ..Default::default()
        };
        assert_eq!(form.platforms(), vec![Platform::Windows, Platform::Linux]);
        assert_eq!(form.content_descriptors(), vec![ContentDescriptor::Horror]);
        assert_eq!(form.into_input().tags, vec!["puzzle", "cozy"]);
    }
}
