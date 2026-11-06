//! Moderation after review: player reports, developer appeals and strikes.
//!
//! Every step is a person's call. Reports only put a game in front of a
//! reviewer; nothing is hidden or delisted automatically.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Game, PolicyRule, Submission, UserSummary};

/// Why a player reported a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReportReason {
    /// The download contains malware or does something harmful.
    Malware,
    /// The game doesn't start or can't be played.
    Broken,
    /// It uses someone else's work without permission.
    Stolen,
    /// It breaks the content guidelines (sexual content, hate, gambling...).
    Guidelines,
    /// The page doesn't match the game, or content warnings are missing.
    Misleading,
    Spam,
    Other,
}

impl ReportReason {
    pub const ALL: [ReportReason; 7] = [
        ReportReason::Malware,
        ReportReason::Broken,
        ReportReason::Stolen,
        ReportReason::Guidelines,
        ReportReason::Misleading,
        ReportReason::Spam,
        ReportReason::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ReportReason::Malware => "malware",
            ReportReason::Broken => "broken",
            ReportReason::Stolen => "stolen",
            ReportReason::Guidelines => "guidelines",
            ReportReason::Misleading => "misleading",
            ReportReason::Spam => "spam",
            ReportReason::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == value)
    }
}

/// `POST /v1/games/{slug}/reports`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateReport {
    pub reason: ReportReason,
    /// What the player saw. Required for `other`.
    #[serde(default)]
    pub details: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    /// Waiting for a reviewer.
    Open,
    /// A reviewer looked and found nothing to act on.
    Dismissed,
    /// The game was delisted.
    Actioned,
}

impl ReportStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ReportStatus::Open => "open",
            ReportStatus::Dismissed => "dismissed",
            ReportStatus::Actioned => "actioned",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "open" => ReportStatus::Open,
            "dismissed" => ReportStatus::Dismissed,
            "actioned" => ReportStatus::Actioned,
            _ => return None,
        })
    }
}

/// A player's report, as staff see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Report {
    pub id: Uuid,
    /// The game's slug.
    pub game: String,
    pub reason: ReportReason,
    pub details: String,
    /// `None` for anonymous reports.
    pub reporter: Option<UserSummary>,
    pub status: ReportStatus,
    pub resolved_by: Option<UserSummary>,
    pub resolution_notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

/// How many open reports give one reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReasonCount {
    pub reason: ReportReason,
    pub count: u64,
}

/// A report queue entry: a game with open reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReportedGame {
    pub game: Game,
    pub open_reports: u64,
    /// Most common first.
    pub reasons: Vec<ReasonCount>,
    pub first_reported_at: DateTime<Utc>,
    pub last_reported_at: DateTime<Utc>,
    /// The developer's strikes that still count.
    pub developer_strikes: u64,
}

/// `POST /v1/review/games/{slug}/reports/dismiss`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DismissReports {
    /// Why nothing needs doing. Staff only; reporters aren't told.
    pub notes: String,
}

/// How many reports a staff action closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResolvedReports {
    pub resolved: u64,
}

// ---------------------------------------------------------------- strikes

/// A recorded violation against a developer. Strikes are a record for staff
/// to weigh; they don't trigger anything on their own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Strike {
    pub id: Uuid,
    /// The developer's slug.
    pub developer: String,
    /// The game it was for, if any (its slug).
    pub game: Option<String>,
    pub rule: PolicyRule,
    /// What happened. Visible to the developer.
    pub notes: String,
    pub issued_by: Option<UserSummary>,
    pub created_at: DateTime<Utc>,
    /// Set when staff took it back, e.g. after an appeal.
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoke_reason: Option<String>,
}

impl Strike {
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}

/// `POST /v1/review/developers/{slug}/strikes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateStrike {
    pub rule: PolicyRule,
    pub notes: String,
    /// The slug of the developer's game it is for.
    #[serde(default)]
    pub game: Option<String>,
}

/// `POST /v1/review/strikes/{id}/revoke`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RevokeStrike {
    pub reason: String,
}

// ---------------------------------------------------------------- appeals

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AppealStatus {
    /// Waiting for a reviewer.
    Pending,
    /// The decision stands.
    Upheld,
    /// The decision was wrong; the game is published.
    Overturned,
    /// The game goes back to the developer to change and resubmit.
    Reopened,
}

impl AppealStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AppealStatus::Pending => "pending",
            AppealStatus::Upheld => "upheld",
            AppealStatus::Overturned => "overturned",
            AppealStatus::Reopened => "reopened",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pending" => AppealStatus::Pending,
            "upheld" => AppealStatus::Upheld,
            "overturned" => AppealStatus::Overturned,
            "reopened" => AppealStatus::Reopened,
            _ => return None,
        })
    }
}

/// A developer asking staff to look again at a rejection or a delisting.
/// Each decision can be appealed once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Appeal {
    pub id: Uuid,
    /// The game's slug.
    pub game: String,
    /// The decision being appealed.
    pub submission: Submission,
    /// The developer's case.
    pub statement: String,
    pub status: AppealStatus,
    pub reviewer: Option<UserSummary>,
    /// The reviewer's answer. Visible to the developer.
    pub decision_notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
}

/// `POST /v1/games/{slug}/appeals`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateAppeal {
    pub statement: String,
}

/// An appeal queue entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AppealItem {
    pub appeal: Appeal,
    pub game: Game,
    pub developer_strikes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AppealOutcome {
    Upheld,
    Overturned,
    Reopened,
}

impl AppealOutcome {
    pub fn status(self) -> AppealStatus {
        match self {
            AppealOutcome::Upheld => AppealStatus::Upheld,
            AppealOutcome::Overturned => AppealStatus::Overturned,
            AppealOutcome::Reopened => AppealStatus::Reopened,
        }
    }
}

/// `POST /v1/review/appeals/{id}/decision`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AppealDecision {
    pub outcome: AppealOutcome,
    /// Required. The developer sees these.
    pub notes: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_round_trip() {
        for reason in ReportReason::ALL {
            assert_eq!(ReportReason::parse(reason.as_str()), Some(reason));
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(json, format!("\"{}\"", reason.as_str()));
        }
    }
}
