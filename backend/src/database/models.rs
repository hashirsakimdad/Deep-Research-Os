//! Read models that join several tables.

use serde::Serialize;
use uuid::Uuid;

use crate::models::{Claim, Evidence, SourceReference};

/// A claim with the evidence and source it traces back to.
/// Powers the claim -> evidence -> source -> URL inspection path.
#[derive(Debug, Clone, Serialize)]
pub struct ClaimRecord {
    pub claim: Claim,
    pub evidence: Vec<Evidence>,
    pub source: SourceReference,
    pub question_id: Option<Uuid>,
    pub question: String,
}

impl ClaimRecord {
    /// The verifier's explanation, if it has run.
    pub fn support_note(&self) -> String {
        self.evidence
            .iter()
            .find_map(|e| e.reasoning.clone())
            .unwrap_or_default()
    }

    pub fn excerpt(&self) -> String {
        self.evidence.first().map(|e| e.excerpt.clone()).unwrap_or_default()
    }
}

/// Aggregate counts shown in the live dashboard; always computed from the database.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct RunStats {
    pub questions: i64,
    pub tasks_total: i64,
    pub tasks_completed: i64,
    pub tasks_failed: i64,
    pub tasks_running: i64,
    pub sources: i64,
    pub claims: i64,
    pub supported: i64,
    pub partially_supported: i64,
    pub unverified: i64,
    pub unsupported: i64,
    pub contradicted: i64,
    pub contradictions: i64,
    pub genuine_contradictions: i64,
}
