//! Data shapes the website renders.
//!
//! The wire types come from `foyer-types`, shared with the API. This module
//! adds display helpers for them (the [`Label`] trait) and a few website-only
//! shapes that bundle several API responses for one page.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use foyer_types::*;

/// Human-readable names for the API's enums.
pub trait Label {
    fn label(self) -> &'static str;
}

impl Label for Platform {
    fn label(self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Macos => "macOS",
            Platform::Linux => "Linux",
            Platform::Web => "Browser",
        }
    }
}

impl Label for GameStatus {
    fn label(self) -> &'static str {
        match self {
            GameStatus::Draft => "Draft",
            GameStatus::InReview => "In review",
            GameStatus::ChangesRequested => "Changes requested",
            GameStatus::Published => "Published",
            GameStatus::Rejected => "Rejected",
            GameStatus::Delisted => "Delisted",
        }
    }
}

impl Label for SupportKind {
    fn label(self) -> &'static str {
        match self {
            SupportKind::Patreon => "Patreon",
            SupportKind::Kofi => "Ko-fi",
            SupportKind::GithubSponsors => "GitHub Sponsors",
            SupportKind::Itchio => "itch.io",
            SupportKind::Website => "Website",
            SupportKind::Other => "Support",
        }
    }
}

impl Label for ContentDescriptor {
    fn label(self) -> &'static str {
        match self {
            ContentDescriptor::Violence => "Violence",
            ContentDescriptor::Blood => "Blood",
            ContentDescriptor::Horror => "Horror",
            ContentDescriptor::StrongLanguage => "Strong language",
            ContentDescriptor::DrugsAlcohol => "Drugs and alcohol",
            ContentDescriptor::MatureThemes => "Mature themes",
        }
    }
}

impl Label for SubmissionStatus {
    fn label(self) -> &'static str {
        match self {
            SubmissionStatus::Pending => "Waiting for a reviewer",
            SubmissionStatus::InReview => "In review",
            SubmissionStatus::Approved => "Approved",
            SubmissionStatus::ChangesRequested => "Changes requested",
            SubmissionStatus::Rejected => "Rejected",
        }
    }
}

impl Label for PolicyRule {
    fn label(self) -> &'static str {
        match self {
            PolicyRule::SexualContent => "Sexual content",
            PolicyRule::NoGameplay => "No-gameplay games",
            PolicyRule::Hate => "Hate and harassment",
            PolicyRule::Gambling => "Real-money gambling",
            PolicyRule::AssetFlip => "Asset flips and AI slop",
            PolicyRule::Spam => "Spam patterns",
            PolicyRule::QualityRuns => "Quality bar: runs",
            PolicyRule::QualityComplete => "Quality bar: complete experience",
            PolicyRule::QualityPage => "Quality bar: presentable page",
            PolicyRule::QualityOriginal => "Quality bar: original work",
            PolicyRule::QualityMetadata => "Quality bar: honest metadata",
        }
    }
}

/// The API refuses edits while a game is in review, rejected or delisted.
pub fn is_editable(status: GameStatus) -> bool {
    matches!(
        status,
        GameStatus::Draft | GameStatus::ChangesRequested | GameStatus::Published
    )
}

/// Statuses the API accepts a new submission from.
pub fn can_submit(status: GameStatus) -> bool {
    matches!(status, GameStatus::Draft | GameStatus::ChangesRequested)
}

pub const CONTENT_DESCRIPTORS: [ContentDescriptor; 6] = [
    ContentDescriptor::Violence,
    ContentDescriptor::Blood,
    ContentDescriptor::Horror,
    ContentDescriptor::StrongLanguage,
    ContentDescriptor::DrugsAlcohol,
    ContentDescriptor::MatureThemes,
];

/// Button text for a support link.
pub fn support_link_text(link: &SupportLink) -> String {
    link.label
        .clone()
        .unwrap_or_else(|| link.kind.label().to_string())
}

pub fn screenshots(game: &Game) -> impl Iterator<Item = &Media> {
    game.media
        .iter()
        .filter(|m| m.kind == MediaKind::Screenshot)
}

pub fn capsule(game: &Game) -> Option<&Media> {
    game.media.iter().find(|m| m.kind == MediaKind::Capsule)
}

// ---------------------------------------------------------------------------
// Website-only shapes (not part of the API).

/// The current build for one platform, from `GET /v1/games/{slug}/channels`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Download {
    pub platform: Platform,
    pub channel: String,
    pub version: Option<String>,
    pub size_bytes: u64,
}

impl Download {
    /// The stable build per platform: channels named after the platform win
    /// over others such as `windows-beta`.
    pub fn from_channels(channels: Vec<ChannelStatus>) -> Vec<Download> {
        let mut out: Vec<Download> = Vec::new();
        for c in channels {
            let Some(build) = c.latest.filter(|b| b.state == BuildState::Ready) else {
                continue;
            };
            let stable = c.channel == c.platform.as_str();
            let item = Download {
                platform: c.platform,
                channel: c.channel,
                version: build.version,
                size_bytes: build.total_size,
            };
            match out.iter_mut().find(|d| d.platform == item.platform) {
                Some(existing) if stable => *existing = item,
                Some(_) => {}
                None => out.push(item),
            }
        }
        out.sort_by_key(|d| d.platform);
        out
    }
}

/// Everything the public game page needs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GamePage {
    pub game: Game,
    pub developer: Developer,
    pub downloads: Vec<Download>,
    pub more_from_developer: Vec<Game>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeveloperProfile {
    pub developer: Developer,
    pub games: Vec<Game>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Discover {
    pub new_and_approved: Vec<Game>,
    pub tags: Vec<String>,
}

/// The signed-in account and its developer profile, if it has one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub user: User,
    pub developer: Option<Developer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DashboardGame {
    pub game: Game,
    pub submissions: Vec<Submission>,
}

pub const MAX_TAGS: usize = 10;

pub fn split_tags(raw: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in raw.split(',').map(slugify).filter(|s| !s.is_empty()) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// Lowercase letters, digits and single dashes, at most 40 characters: the
/// API's slug rules.
pub fn slugify(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(40);
    out.trim_end_matches('-').to_string()
}

pub fn format_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 1.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{:.0} KB", (bytes as f64 / 1024.0).max(1.0))
    }
}

pub fn date_only(ts: &DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d").to_string()
}

pub fn is_https_url(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
        && url.len() > 8
}

/// A stable hue for placeholder art, until real media is uploaded.
pub fn placeholder_hue(seed: &str) -> u32 {
    seed.bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
        % 360
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_collapses_punctuation() {
        assert_eq!(slugify("Moss & Lanterns: Redux!"), "moss-lanterns-redux");
        assert_eq!(slugify("  --  "), "");
        assert_eq!(slugify(&"a".repeat(60)).len(), 40);
    }

    #[test]
    fn split_tags_dedupes_and_slugs() {
        assert_eq!(
            split_tags(" Puzzle, ,Cozy Games, puzzle "),
            vec!["puzzle", "cozy-games"]
        );
    }

    #[test]
    fn format_size_picks_unit() {
        assert_eq!(format_size(300 * 1024 * 1024), "300 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(format_size(2048), "2 KB");
    }

    #[test]
    fn https_only() {
        assert!(is_https_url("HTTPS://ko-fi.com/x"));
        assert!(!is_https_url("http://ko-fi.com/x"));
        assert!(!is_https_url("javascript:alert(1)"));
        assert!(!is_https_url("https://"));
    }

    fn channel(name: &str, platform: Platform, version: &str, state: BuildState) -> ChannelStatus {
        ChannelStatus {
            channel: name.into(),
            platform,
            latest: Some(Build {
                id: uuid::Uuid::nil(),
                game: "g".into(),
                channel: name.into(),
                platform,
                version: Some(version.into()),
                state,
                total_size: 10,
                chunk_count: 1,
                created_at: Utc::now(),
            }),
        }
    }

    #[test]
    fn downloads_prefer_the_stable_channel() {
        let downloads = Download::from_channels(vec![
            channel("linux-beta", Platform::Linux, "2.0-beta", BuildState::Ready),
            channel("linux", Platform::Linux, "1.0", BuildState::Ready),
            channel("windows", Platform::Windows, "1.0", BuildState::Processing),
        ]);
        assert_eq!(downloads.len(), 1);
        assert_eq!(downloads[0].version.as_deref(), Some("1.0"));
    }
}
