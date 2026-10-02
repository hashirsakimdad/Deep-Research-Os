use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::string_enum;

string_enum! {
    /// How well the cited source supports a claim.
    pub enum SupportLevel {
        Supported => "SUPPORTED",
        PartiallySupported => "PARTIALLY_SUPPORTED",
        Contradicted => "CONTRADICTED",
        Unsupported => "UNSUPPORTED",
        Unverified => "UNVERIFIED",
    }
}

impl SupportLevel {
    /// Whether the claim may be used as a basis for report conclusions.
    pub fn is_citable(&self) -> bool {
        matches!(self, Self::Supported | Self::PartiallySupported)
    }
}

string_enum! {
    pub enum ContradictionKind {
        DirectContradiction => "DIRECT_CONTRADICTION",
        ContextDifference => "CONTEXT_DIFFERENCE",
        TimeDifference => "TIME_DIFFERENCE",
        PopulationDifference => "POPULATION_DIFFERENCE",
        MetricDifference => "METRIC_DIFFERENCE",
        ScopeDifference => "SCOPE_DIFFERENCE",
        NoConflict => "NO_CONFLICT",
    }
}

impl ContradictionKind {
    /// Genuine disagreement, as opposed to a difference in what was measured.
    pub fn is_genuine(&self) -> bool {
        matches!(self, Self::DirectContradiction)
    }

    /// Worth recording at all.
    pub fn is_notable(&self) -> bool {
        !matches!(self, Self::NoConflict)
    }
}

/// An assertion extracted from a source. The `evidence` excerpt is verbatim
/// text found in the source content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: Uuid,
    pub research_run_id: Uuid,
    pub research_task_id: Uuid,
    pub text: String,
    pub source_id: Uuid,
    pub confidence: f32,
    pub status: SupportLevel,
    pub iteration: i32,
    pub created_at: DateTime<Utc>,
}

/// Links a claim to the passage of a source that is claimed to support it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: Uuid,
    pub claim_id: Uuid,
    pub source_id: Uuid,
    pub excerpt: String,
    pub relevance: f32,
    pub support_level: SupportLevel,
    pub reasoning: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimReference {
    pub id: Uuid,
    pub text: String,
    pub source_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedClaim {
    pub id: Uuid,
    pub support_level: SupportLevel,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contradiction {
    pub id: Uuid,
    pub research_run_id: Uuid,
    pub claim_a_id: Uuid,
    pub claim_b_id: Uuid,
    pub kind: ContradictionKind,
    pub explanation: String,
    pub created_at: DateTime<Utc>,
}

/// Applies the conservative adjustments that must hold regardless of what the
/// verifier model said: weak support never carries high confidence, and
/// claims that are not citable are capped hard.
pub fn adjust_confidence(level: SupportLevel, proposed: f32, source_cap: f32) -> f32 {
    let level_cap = match level {
        SupportLevel::Supported => 1.0,
        SupportLevel::PartiallySupported => 0.7,
        SupportLevel::Unverified => 0.4,
        SupportLevel::Contradicted | SupportLevel::Unsupported => 0.2,
    };
    proposed.clamp(0.0, 1.0).min(level_cap).min(source_cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_levels_round_trip() {
        for level in [
            SupportLevel::Supported,
            SupportLevel::PartiallySupported,
            SupportLevel::Contradicted,
            SupportLevel::Unsupported,
            SupportLevel::Unverified,
        ] {
            assert_eq!(level.as_str().parse::<SupportLevel>().unwrap(), level);
            let json = serde_json::to_string(&level).unwrap();
            assert_eq!(serde_json::from_str::<SupportLevel>(&json).unwrap(), level);
        }
        assert_eq!("partially_supported".parse::<SupportLevel>().unwrap(), SupportLevel::PartiallySupported);
        assert!("MAYBE".parse::<SupportLevel>().is_err());
    }

    #[test]
    fn only_supported_levels_are_citable() {
        assert!(SupportLevel::Supported.is_citable());
        assert!(SupportLevel::PartiallySupported.is_citable());
        assert!(!SupportLevel::Unverified.is_citable());
        assert!(!SupportLevel::Contradicted.is_citable());
        assert!(!SupportLevel::Unsupported.is_citable());
    }

    #[test]
    fn contradiction_classification() {
        assert!(ContradictionKind::DirectContradiction.is_genuine());
        assert!(!ContradictionKind::MetricDifference.is_genuine());
        assert!(ContradictionKind::ScopeDifference.is_notable());
        assert!(!ContradictionKind::NoConflict.is_notable());
    }

    #[test]
    fn confidence_is_capped_conservatively() {
        assert_eq!(adjust_confidence(SupportLevel::Supported, 0.95, 1.0), 0.95);
        assert_eq!(adjust_confidence(SupportLevel::PartiallySupported, 0.95, 1.0), 0.7);
        assert_eq!(adjust_confidence(SupportLevel::Supported, 0.95, 0.5), 0.5);
        assert_eq!(adjust_confidence(SupportLevel::Unsupported, 0.9, 1.0), 0.2);
        assert_eq!(adjust_confidence(SupportLevel::Supported, 7.0, 1.0), 1.0);
    }
}
