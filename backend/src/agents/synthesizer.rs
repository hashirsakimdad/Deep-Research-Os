use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use serde::Serialize;
use uuid::Uuid;

use super::agent::Agent;
use crate::{
    error::AgentError,
    llm::{LlmProvider, LlmRequest, generate_structured},
    models::{ContradictionKind, ReportContent, SourceNature, SupportLevel},
};

const NAME: &str = "synthesizer";

const SYSTEM: &str = r#"You are the Synthesizer of an autonomous research organization.
You write the research report from VERIFIED claims only. You never see raw search results and you never
invent facts, numbers, sources or URLs.

Rules:
- Every factual statement must list the "claim_ids" (from the provided claims, exactly as given) that back it.
  A statement with no supporting claim_ids may only be framing or an explicit statement of uncertainty.
- Respect each claim's support level. PARTIALLY_SUPPORTED claims may only be stated at the narrow scope given in
  their "support_note". Never upgrade them to general conclusions. Hedge when evidence is thin or comes from
  weaker sources ("source_nature": opinion, marketing, commentary).
- Surface disagreement. For every contradiction provided, describe it in "contradictory_evidence" citing BOTH
  claims; say whether it is a genuine conflict or a difference in context/metric/scope. Do not pick a winner
  unless the evidence clearly warrants it.
- Cover every research question. For questions with little or no verified evidence, say so under limitations and
  open_questions rather than filling the gap.
- If REVIEWER FEEDBACK is provided, fix those problems in this draft.
- Write in plain, precise prose. No markdown headings inside text fields. Do not write citation markers like [1];
  they are added automatically from claim_ids.

Reply with ONLY this JSON:
{"executive_summary": "...",
 "methodology": "how the research was conducted: questions, search, verification, contradiction analysis",
 "key_findings": [{"text": "...", "claim_ids": ["..."]}],
 "detailed_findings": [{"heading": "one per research question", "body": [{"text": "...", "claim_ids": ["..."]}]}],
 "contradictory_evidence": [{"text": "...", "claim_ids": ["...", "..."]}],
 "limitations": ["..."],
 "open_questions": ["..."],
 "conclusion": "..."}"#;

#[derive(Debug, Clone, Serialize)]
pub struct SynthClaim {
    pub id: Uuid,
    pub text: String,
    pub research_question: String,
    pub support_level: SupportLevel,
    /// What the verifier established; mandatory scope for partial claims.
    pub support_note: String,
    pub confidence: f32,
    pub source_title: String,
    pub source_nature: SourceNature,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SynthContradiction {
    pub claim_a: Uuid,
    pub claim_b: Uuid,
    pub kind: ContradictionKind,
    pub explanation: String,
}

#[derive(Debug, Clone)]
pub struct SynthesisInput {
    pub research_id: Uuid,
    pub query: String,
    pub questions: Vec<String>,
    pub claims: Vec<SynthClaim>,
    pub contradictions: Vec<SynthContradiction>,
    /// Questions with no usable evidence and other known gaps.
    pub gaps: Vec<String>,
    /// Problems the critic found in the previous draft, if any.
    pub reviewer_feedback: Vec<String>,
    pub iteration: i32,
}

/// Structural validation: only provided claim ids, findings are cited,
/// non-empty required sections.
pub fn validate_report(report: &ReportContent, allowed: &HashSet<Uuid>) -> Result<(), String> {
    if report.executive_summary.trim().is_empty() || report.conclusion.trim().is_empty() {
        return Err("executive_summary and conclusion must not be empty".into());
    }
    if report.key_findings.is_empty() {
        return Err("key_findings must not be empty".into());
    }
    if let Some(bad) = report.referenced_claim_ids().iter().find(|id| !allowed.contains(id)) {
        return Err(format!("claim id {bad} is not one of the provided verified claims"));
    }
    if report.key_findings.iter().any(|f| f.claim_ids.is_empty()) && !allowed.is_empty() {
        return Err("every key finding needs at least one supporting claim_id".into());
    }
    Ok(())
}

pub struct SynthesizerAgent {
    llm: Arc<dyn LlmProvider>,
}

impl SynthesizerAgent {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Agent for SynthesizerAgent {
    type Input = SynthesisInput;
    type Output = ReportContent;

    fn name(&self) -> &'static str {
        NAME
    }

    async fn execute(&self, input: SynthesisInput) -> Result<ReportContent, AgentError> {
        if input.claims.is_empty() {
            return Err(AgentError::InvalidInput {
                agent: NAME,
                reason: "no citable claims available to synthesize".into(),
            });
        }
        let allowed: HashSet<Uuid> = input.claims.iter().map(|c| c.id).collect();
        let user = format!(
            "Research question: {}\nIteration: {}\n\nResearch questions:\n{}\n\nVerified claims:\n{}\n\nContradictions:\n{}\n\nKnown gaps:\n{}\n\nREVIEWER FEEDBACK:\n{}",
            input.query,
            input.iteration,
            numbered(&input.questions),
            serde_json::to_string_pretty(&input.claims).unwrap_or_default(),
            serde_json::to_string_pretty(&input.contradictions).unwrap_or_default(),
            numbered(&input.gaps),
            numbered(&input.reviewer_feedback),
        );
        let mut request = LlmRequest::json(NAME, SYSTEM, user);
        request.temperature = 0.3;
        generate_structured(self.llm.as_ref(), NAME, request, |r: &ReportContent| validate_report(r, &allowed)).await
    }
}

fn numbered(items: &[String]) -> String {
    if items.is_empty() {
        return "(none)".into();
    }
    items.iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::structured::testing::ScriptedLlm;

    fn claim() -> SynthClaim {
        SynthClaim {
            id: Uuid::new_v4(),
            text: "Agents completed the task 55% faster.".into(),
            research_question: "productivity?".into(),
            support_level: SupportLevel::PartiallySupported,
            support_note: "one task".into(),
            confidence: 0.7,
            source_title: "Study".into(),
            source_nature: SourceNature::Primary,
            published_at: None,
        }
    }

    fn input(claims: Vec<SynthClaim>) -> SynthesisInput {
        SynthesisInput {
            research_id: Uuid::new_v4(),
            query: "q".into(),
            questions: vec!["productivity?".into()],
            claims,
            contradictions: vec![],
            gaps: vec![],
            reviewer_feedback: vec![],
            iteration: 1,
        }
    }

    fn report_json(claim_id: &str) -> String {
        format!(
            r#"{{"executive_summary":"s","methodology":"m",
              "key_findings":[{{"text":"f","claim_ids":["{claim_id}"]}}],
              "detailed_findings":[{{"heading":"h","body":[{{"text":"b","claim_ids":["{claim_id}"]}}]}}],
              "limitations":["l"],"conclusion":"c"}}"#
        )
    }

    #[tokio::test]
    async fn accepts_reports_citing_only_provided_claims() {
        let c = claim();
        let llm = Arc::new(ScriptedLlm::ok(&[&report_json(&c.id.to_string())]));
        let out = SynthesizerAgent::new(llm).execute(input(vec![c.clone()])).await.unwrap();
        assert_eq!(out.key_findings[0].claim_ids, vec![c.id]);
        assert!(out.contradictory_evidence.is_empty());
    }

    #[tokio::test]
    async fn rejects_invented_claim_ids_and_repairs() {
        let c = claim();
        let invented = report_json(&Uuid::new_v4().to_string());
        let llm = Arc::new(ScriptedLlm::ok(&[&invented, &report_json(&c.id.to_string())]));
        let out = SynthesizerAgent::new(llm.clone()).execute(input(vec![c.clone()])).await.unwrap();
        assert_eq!(out.key_findings[0].claim_ids, vec![c.id]);
        assert_eq!(llm.call_count(), 2);
    }

    #[tokio::test]
    async fn refuses_to_write_without_evidence() {
        let llm = Arc::new(ScriptedLlm::ok(&[]));
        let err = SynthesizerAgent::new(llm.clone()).execute(input(vec![])).await.unwrap_err();
        assert!(matches!(err, AgentError::InvalidInput { .. }));
        assert_eq!(llm.call_count(), 0);
    }

    #[tokio::test]
    async fn prompt_carries_reviewer_feedback_and_support_notes() {
        let c = claim();
        let llm = Arc::new(ScriptedLlm::ok(&[&report_json(&c.id.to_string())]));
        let mut i = input(vec![c]);
        i.reviewer_feedback = vec!["Key finding 2 is overconfident".into()];
        SynthesizerAgent::new(llm.clone()).execute(i).await.unwrap();
        let req = llm.requests.lock().unwrap();
        let user = &req[0].messages[0].content;
        assert!(user.contains("Key finding 2 is overconfident"));
        assert!(user.contains("one task"));
    }
}
