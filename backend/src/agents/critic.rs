use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::agent::Agent;
use crate::{
    error::AgentError,
    llm::{LlmProvider, LlmRequest, generate_structured},
    models::{CritiqueIssue, ReportContent, SupportLevel},
};

const NAME: &str = "critic";

const SYSTEM: &str = r#"You are the Critic of an autonomous research organization. You review a draft research
report against the evidence it was built from. You are skeptical and specific; you do not rewrite the report.

Check for:
- UNSUPPORTED_CLAIM: a statement stronger than its cited claims (check the support level and support_note).
- INSUFFICIENT_EVIDENCE: an important conclusion resting on one weak or low-quality source.
- MISSING_CITATION: a factual statement without claim_ids.
- LOGICAL_INCONSISTENCY: statements that contradict each other or conclusions that do not follow.
- OVERCONFIDENT: certainty or generalization the evidence does not warrant.
- IGNORED_CONTRADICTION: listed contradictions the report does not address honestly.
- UNANSWERED_QUESTION: a research question with no real answer in the report.
- SOURCE_QUALITY: reliance on marketing, opinion or secondary commentary where primary evidence was needed.

For each issue give "type", the offending "claim_id" if one applies (exactly as provided), a concrete
"description" and "severity" (high/medium/low). High means the report should not be accepted as is.
"quality" (0-1) is your overall judgement of rigor and completeness. Set "needs_more_research" true only if
NEW evidence would materially improve the report (not for wording problems), and then list specific
"required_research" questions that a researcher could answer with web search. Otherwise leave it empty.

Reply with ONLY:
{"quality": 0.7, "needs_more_research": false,
 "issues": [{"type": "OVERCONFIDENT", "claim_id": null, "description": "...", "severity": "medium"}],
 "required_research": []}"#;

#[derive(Debug, Clone, Serialize)]
pub struct CritiqueClaim {
    pub id: Uuid,
    pub text: String,
    pub support_level: SupportLevel,
    pub support_note: String,
    pub source_nature: String,
}

#[derive(Debug, Clone)]
pub struct CritiqueInput {
    pub research_id: Uuid,
    pub query: String,
    pub questions: Vec<String>,
    pub report: ReportContent,
    pub claims: Vec<CritiqueClaim>,
    pub contradiction_notes: Vec<String>,
    pub unverified_or_rejected_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CritiqueDraft {
    pub quality: f32,
    #[serde(default)]
    pub needs_more_research: bool,
    #[serde(default)]
    pub issues: Vec<CritiqueIssue>,
    #[serde(default)]
    pub required_research: Vec<String>,
}

pub struct CriticAgent {
    llm: Arc<dyn LlmProvider>,
}

impl CriticAgent {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Agent for CriticAgent {
    type Input = CritiqueInput;
    type Output = CritiqueDraft;

    fn name(&self) -> &'static str {
        NAME
    }

    async fn execute(&self, input: CritiqueInput) -> Result<CritiqueDraft, AgentError> {
        let known: HashSet<Uuid> = input.claims.iter().map(|c| c.id).collect();
        let user = format!(
            "Research question: {}\n\nResearch questions:\n{}\n\nDRAFT REPORT (claim_ids shown as given):\n{}\n\nClaims available to the report:\n{}\n\nContradictions found:\n{}\n\nClaims that failed verification and were excluded: {}",
            input.query,
            input.questions.iter().enumerate().map(|(i, q)| format!("{}. {q}", i + 1)).collect::<Vec<_>>().join("\n"),
            serde_json::to_string_pretty(&input.report).unwrap_or_default(),
            serde_json::to_string_pretty(&input.claims).unwrap_or_default(),
            if input.contradiction_notes.is_empty() { "(none)".to_string() } else { input.contradiction_notes.join("\n") },
            input.unverified_or_rejected_count,
        );
        generate_structured(self.llm.as_ref(), NAME, LlmRequest::json(NAME, SYSTEM, user), |c: &CritiqueDraft| {
            if !(0.0..=1.0).contains(&c.quality) {
                return Err("quality must be between 0 and 1".into());
            }
            if let Some(bad) = c.issues.iter().filter_map(|i| i.claim_id).find(|id| !known.contains(id)) {
                return Err(format!("issue refers to unknown claim_id {bad}"));
            }
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{llm::structured::testing::ScriptedLlm, models::{CitedStatement, IssueType, Severity}};

    fn input(claim_id: Uuid) -> CritiqueInput {
        CritiqueInput {
            research_id: Uuid::new_v4(),
            query: "q".into(),
            questions: vec!["a?".into()],
            report: ReportContent {
                executive_summary: "s".into(),
                methodology: "m".into(),
                key_findings: vec![CitedStatement { text: "f".into(), claim_ids: vec![claim_id] }],
                detailed_findings: vec![],
                contradictory_evidence: vec![],
                limitations: vec![],
                open_questions: vec![],
                conclusion: "c".into(),
            },
            claims: vec![CritiqueClaim { id: claim_id, text: "t".into(), support_level: SupportLevel::Supported, support_note: "n".into(), source_nature: "primary".into() }],
            contradiction_notes: vec![],
            unverified_or_rejected_count: 0,
        }
    }

    #[tokio::test]
    async fn parses_structured_feedback() {
        let id = Uuid::new_v4();
        let reply = format!(
            r#"{{"quality":0.62,"needs_more_research":true,
                 "issues":[{{"type":"INSUFFICIENT_EVIDENCE","claim_id":"{id}","description":"single source","severity":"high"}}],
                 "required_research":["Find independent replication of the 55% result"]}}"#
        );
        let llm = Arc::new(ScriptedLlm::ok(&[&reply]));
        let out = CriticAgent::new(llm).execute(input(id)).await.unwrap();
        assert!(out.needs_more_research);
        assert_eq!(out.issues[0].kind, IssueType::InsufficientEvidence);
        assert_eq!(out.issues[0].severity, Severity::High);
        assert_eq!(out.required_research.len(), 1);
    }

    #[tokio::test]
    async fn rejects_out_of_range_quality_and_unknown_claims() {
        let id = Uuid::new_v4();
        let bad_quality = r#"{"quality":7}"#.to_string();
        let bad_claim = format!(r#"{{"quality":0.5,"issues":[{{"type":"OVERCONFIDENT","claim_id":"{}","description":"d","severity":"low"}}]}}"#, Uuid::new_v4());
        let ok = r#"{"quality":0.9}"#.to_string();
        let llm = Arc::new(ScriptedLlm::ok(&[&bad_quality, &bad_claim, &ok]));
        let out = CriticAgent::new(llm.clone()).execute(input(id)).await.unwrap();
        assert_eq!(out.quality, 0.9);
        assert_eq!(llm.call_count(), 3);
    }
}
