use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{GameStatus, UserSummary};

/// A rule from the public content guidelines. Every rejection cites one, and
/// a change request can point at one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PolicyRule {
    /// Hard exclusion: nudity, sexual content or fetish themes.
    SexualContent,
    /// Hard exclusion: mostly clicking through text and images, without
    /// meaningful mechanics or player decisions.
    NoGameplay,
    /// Hard exclusion: content promoting hate against protected groups.
    Hate,
    /// Hard exclusion: cash-out gambling or paid loot boxes.
    Gambling,
    /// Hard exclusion: asset flips, template reskins, mostly unedited
    /// generated assets.
    AssetFlip,
    /// Hard exclusion: duplicates, keyword stuffing, bundles of others' work.
    Spam,
    /// Quality bar: launches and is playable on every claimed platform.
    QualityRuns,
    /// Quality bar: a complete experience or a real Early Access build.
    QualityComplete,
    /// Quality bar: screenshots, description, tags and capsule are in order.
    QualityPage,
    /// Quality bar: the developer owns or licensed the content.
    QualityOriginal,
    /// Quality bar: content descriptors are set and accurate.
    QualityMetadata,
}

impl PolicyRule {
    pub fn as_str(self) -> &'static str {
        match self {
            PolicyRule::SexualContent => "sexual_content",
            PolicyRule::NoGameplay => "no_gameplay",
            PolicyRule::Hate => "hate",
            PolicyRule::Gambling => "gambling",
            PolicyRule::AssetFlip => "asset_flip",
            PolicyRule::Spam => "spam",
            PolicyRule::QualityRuns => "quality_runs",
            PolicyRule::QualityComplete => "quality_complete",
            PolicyRule::QualityPage => "quality_page",
            PolicyRule::QualityOriginal => "quality_original",
            PolicyRule::QualityMetadata => "quality_metadata",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "sexual_content" => PolicyRule::SexualContent,
            "no_gameplay" => PolicyRule::NoGameplay,
            "hate" => PolicyRule::Hate,
            "gambling" => PolicyRule::Gambling,
            "asset_flip" => PolicyRule::AssetFlip,
            "spam" => PolicyRule::Spam,
            "quality_runs" => PolicyRule::QualityRuns,
            "quality_complete" => PolicyRule::QualityComplete,
            "quality_page" => PolicyRule::QualityPage,
            "quality_original" => PolicyRule::QualityOriginal,
            "quality_metadata" => PolicyRule::QualityMetadata,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubmissionStatus {
    /// Waiting in the queue.
    Pending,
    /// A reviewer claimed it.
    InReview,
    Approved,
    ChangesRequested,
    Rejected,
}

impl SubmissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SubmissionStatus::Pending => "pending",
            SubmissionStatus::InReview => "in_review",
            SubmissionStatus::Approved => "approved",
            SubmissionStatus::ChangesRequested => "changes_requested",
            SubmissionStatus::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pending" => SubmissionStatus::Pending,
            "in_review" => SubmissionStatus::InReview,
            "approved" => SubmissionStatus::Approved,
            "changes_requested" => SubmissionStatus::ChangesRequested,
            "rejected" => SubmissionStatus::Rejected,
            _ => return None,
        })
    }

    /// True once a reviewer has recorded a decision.
    pub fn is_decided(self) -> bool {
        matches!(
            self,
            SubmissionStatus::Approved
                | SubmissionStatus::ChangesRequested
                | SubmissionStatus::Rejected
        )
    }
}

/// What a submission record is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubmissionKind {
    /// The developer asked for a review.
    #[default]
    Review,
    /// Staff took a published game down. Always `rejected`.
    Delisting,
}

impl SubmissionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SubmissionKind::Review => "review",
            SubmissionKind::Delisting => "delisting",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "review" => SubmissionKind::Review,
            "delisting" => SubmissionKind::Delisting,
            _ => return None,
        })
    }
}

/// A request to list a game (or to list it again after changes), or a
/// delisting by staff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Submission {
    pub id: Uuid,
    /// The game's slug.
    pub game: String,
    #[serde(default)]
    pub kind: SubmissionKind,
    pub status: SubmissionStatus,
    /// Notes from the developer to the reviewer.
    pub developer_notes: String,
    pub reviewer: Option<UserSummary>,
    /// Written feedback from the reviewer. Visible to the developer.
    pub decision_notes: Option<String>,
    pub rule: Option<PolicyRule>,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
}

/// `POST /v1/games/{slug}/submissions`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateSubmission {
    #[serde(default)]
    pub developer_notes: String,
}

/// A review queue entry: the submission with the game it is for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QueueItem {
    pub submission: Submission,
    pub game: crate::Game,
    /// The game's builds whose malware scan found something or could not
    /// finish, newest first. Infected builds are never served to players.
    #[serde(default)]
    pub flagged_builds: Vec<crate::Build>,
    /// The developer's strikes that still count.
    #[serde(default)]
    pub developer_strikes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DecisionOutcome {
    Approved,
    ChangesRequested,
    Rejected,
}

impl DecisionOutcome {
    pub fn submission_status(self) -> SubmissionStatus {
        match self {
            DecisionOutcome::Approved => SubmissionStatus::Approved,
            DecisionOutcome::ChangesRequested => SubmissionStatus::ChangesRequested,
            DecisionOutcome::Rejected => SubmissionStatus::Rejected,
        }
    }

    pub fn game_status(self) -> GameStatus {
        match self {
            DecisionOutcome::Approved => GameStatus::Published,
            DecisionOutcome::ChangesRequested => GameStatus::ChangesRequested,
            DecisionOutcome::Rejected => GameStatus::Rejected,
        }
    }
}

/// `POST /v1/review/submissions/{id}/decision`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Decision {
    pub outcome: DecisionOutcome,
    /// Required for `changes_requested` and `rejected`.
    #[serde(default)]
    pub notes: String,
    /// Required for `rejected`.
    #[serde(default)]
    pub rule: Option<PolicyRule>,
    /// Record a strike against the developer. Only with `rejected`.
    #[serde(default)]
    pub strike: bool,
}

/// `POST /v1/review/games/{slug}/delist`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Delist {
    pub rule: PolicyRule,
    /// Sent to the developer.
    pub notes: String,
    /// Record a strike against the developer too.
    #[serde(default)]
    pub strike: bool,
}
