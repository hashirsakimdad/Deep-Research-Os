use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::string_enum;

string_enum! {
    /// Lifecycle phase of a research run.
    pub enum ResearchStatus {
        Planning => "planning",
        Researching => "researching",
        Verifying => "verifying",
        Analyzing => "analyzing",
        Synthesizing => "synthesizing",
        Critiquing => "critiquing",
        Completed => "completed",
        Failed => "failed",
    }
}

impl ResearchStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

string_enum! {
    pub enum Priority {
        High => "high",
        Medium => "medium",
        Low => "low",
    }
}

string_enum! {
    pub enum TaskStatus {
        Pending => "pending",
        Running => "running",
        Completed => "completed",
        Failed => "failed",
    }
}

/// A sub-question produced by the planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchQuestion {
    pub id: Uuid,
    pub research_run_id: Uuid,
    pub question: String,
    pub objective: String,
    pub priority: Priority,
    pub position: i32,
}

/// A unit of work executed by a researcher agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchTask {
    pub id: Uuid,
    pub research_run_id: Uuid,
    pub question_id: Option<Uuid>,
    pub question: String,
    pub objective: String,
    pub priority: Priority,
    pub status: TaskStatus,
    pub assigned_agent: String,
    pub required_evidence: Vec<String>,
    /// Task ids that must complete before this one starts.
    pub dependencies: Vec<Uuid>,
    pub iteration: i32,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchRun {
    pub id: Uuid,
    pub query: String,
    pub status: ResearchStatus,
    pub iteration: i32,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// A persisted, user-visible record of something an agent actually did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub id: i64,
    pub research_id: Uuid,
    pub agent: String,
    pub event_type: String,
    pub message: String,
    pub metadata: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}
