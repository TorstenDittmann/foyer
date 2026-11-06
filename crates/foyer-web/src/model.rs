//! Data shapes the website renders.
//!
//! Provisional copies of the API's `foyer-types` crate, mirroring its field
//! names and enum variants so that swapping to the shared crate is mostly a
//! matter of deleting this file and fixing imports. Ids are UUID strings and
//! timestamps are RFC 3339 strings, as in the API.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Macos,
    Linux,
    Web,
}

impl Platform {
    /// Platforms a developer can pick in the dashboard. Browser games are
    /// out of the MVP.
    pub const DESKTOP: [Platform; 3] = [Platform::Windows, Platform::Macos, Platform::Linux];

    pub fn label(self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Macos => "macOS",
            Platform::Linux => "Linux",
            Platform::Web => "Browser",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Macos => "macos",
            Platform::Linux => "linux",
            Platform::Web => "web",
        }
    }
}

/// Where a game is in its lifecycle. Only `Published` games are public.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    Draft,
    InReview,
    ChangesRequested,
    Published,
    Rejected,
    Delisted,
}

impl GameStatus {
    pub fn label(self) -> &'static str {
        match self {
            GameStatus::Draft => "Draft",
            GameStatus::InReview => "In review",
            GameStatus::ChangesRequested => "Changes requested",
            GameStatus::Published => "Published",
            GameStatus::Rejected => "Rejected",
            GameStatus::Delisted => "Delisted",
        }
    }

    /// A developer can edit the page in these states.
    pub fn is_editable(self) -> bool {
        matches!(
            self,
            GameStatus::Draft | GameStatus::ChangesRequested | GameStatus::Published
        )
    }

    /// A developer can send the game to review in these states.
    pub fn can_submit(self) -> bool {
        matches!(self, GameStatus::Draft | GameStatus::ChangesRequested)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    Patreon,
    Kofi,
    GithubSponsors,
    Itchio,
    Website,
    Other,
}

impl SupportKind {
    pub const ALL: [SupportKind; 6] = [
        SupportKind::Patreon,
        SupportKind::Kofi,
        SupportKind::GithubSponsors,
        SupportKind::Itchio,
        SupportKind::Website,
        SupportKind::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SupportKind::Patreon => "Patreon",
            SupportKind::Kofi => "Ko-fi",
            SupportKind::GithubSponsors => "GitHub Sponsors",
            SupportKind::Itchio => "itch.io",
            SupportKind::Website => "Website",
            SupportKind::Other => "Support",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SupportKind::Patreon => "patreon",
            SupportKind::Kofi => "kofi",
            SupportKind::GithubSponsors => "github_sponsors",
            SupportKind::Itchio => "itchio",
            SupportKind::Website => "website",
            SupportKind::Other => "other",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupportLink {
    pub kind: SupportKind,
    pub url: String,
    pub label: Option<String>,
}

impl SupportLink {
    pub fn display(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| self.kind.label().to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentDescriptor {
    Violence,
    Blood,
    Horror,
    StrongLanguage,
    DrugsAlcohol,
    MatureThemes,
}

impl ContentDescriptor {
    pub const ALL: [ContentDescriptor; 6] = [
        ContentDescriptor::Violence,
        ContentDescriptor::Blood,
        ContentDescriptor::Horror,
        ContentDescriptor::StrongLanguage,
        ContentDescriptor::DrugsAlcohol,
        ContentDescriptor::MatureThemes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ContentDescriptor::Violence => "Violence",
            ContentDescriptor::Blood => "Blood",
            ContentDescriptor::Horror => "Horror",
            ContentDescriptor::StrongLanguage => "Strong language",
            ContentDescriptor::DrugsAlcohol => "Drugs and alcohol",
            ContentDescriptor::MatureThemes => "Mature themes",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ContentDescriptor::Violence => "violence",
            ContentDescriptor::Blood => "blood",
            ContentDescriptor::Horror => "horror",
            ContentDescriptor::StrongLanguage => "strong_language",
            ContentDescriptor::DrugsAlcohol => "drugs_alcohol",
            ContentDescriptor::MatureThemes => "mature_themes",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeveloperSummary {
    pub slug: String,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Developer {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub bio: String,
    pub website_url: Option<String>,
    pub support_links: Vec<SupportLink>,
    pub created_at: String,
}

impl Developer {
    pub fn summary(&self) -> DeveloperSummary {
        DeveloperSummary {
            slug: self.slug.clone(),
            display_name: self.display_name.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Capsule,
    Screenshot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub id: String,
    pub kind: MediaKind,
    pub url: String,
    pub position: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub slug: String,
    pub developer: DeveloperSummary,
    pub title: String,
    pub tagline: String,
    pub description: String,
    pub tags: Vec<String>,
    pub content_descriptors: Vec<ContentDescriptor>,
    pub ai_disclosure: Option<String>,
    pub support_links: Vec<SupportLink>,
    pub platforms: Vec<Platform>,
    pub media: Vec<Media>,
    pub status: GameStatus,
    pub published_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Game {
    pub fn screenshots(&self) -> impl Iterator<Item = &Media> {
        self.media
            .iter()
            .filter(|m| m.kind == MediaKind::Screenshot)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionStatus {
    Pending,
    InReview,
    Approved,
    ChangesRequested,
    Rejected,
}

impl SubmissionStatus {
    pub fn label(self) -> &'static str {
        match self {
            SubmissionStatus::Pending => "Waiting for a reviewer",
            SubmissionStatus::InReview => "In review",
            SubmissionStatus::Approved => "Approved",
            SubmissionStatus::ChangesRequested => "Changes requested",
            SubmissionStatus::Rejected => "Rejected",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyRule {
    SexualContent,
    NoGameplay,
    Hate,
    Gambling,
    AssetFlip,
    Spam,
    QualityRuns,
    QualityComplete,
    QualityPage,
    QualityOriginal,
    QualityMetadata,
}

impl PolicyRule {
    pub fn label(self) -> &'static str {
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: String,
    pub username: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Submission {
    pub id: String,
    /// The game's slug.
    pub game: String,
    pub status: SubmissionStatus,
    pub developer_notes: String,
    pub reviewer: Option<UserSummary>,
    pub decision_notes: Option<String>,
    pub rule: Option<PolicyRule>,
    pub created_at: String,
    pub decided_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Reviewer,
    Admin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub role: Role,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Website-only shapes (not part of the API types).

/// One download per platform for the current stable build. The API exposes
/// this through its builds module; the shape here is a guess until then.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Download {
    pub platform: Platform,
    pub version: String,
    pub size_bytes: u64,
}

/// Everything the public game page needs in one round trip.
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
    pub staff_picks: Vec<Game>,
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

pub fn slugify(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

pub fn format_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

/// The date part of an RFC 3339 timestamp.
pub fn date_only(ts: &str) -> &str {
    ts.get(..10).unwrap_or(ts)
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
    }

    #[test]
    fn status_serializes_like_the_api() {
        let json = serde_json::to_string(&GameStatus::ChangesRequested).unwrap();
        assert_eq!(json, "\"changes_requested\"");
    }
}
