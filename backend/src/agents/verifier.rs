use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::agent::Agent;
use crate::{
    config::Limits,
    error::AgentError,
    llm::{LlmProvider, LlmRequest, generate_structured},
    models::{SourceNature, SourceType, SupportLevel, claim::adjust_confidence},
};

const NAME: &str = "verifier";

const SYSTEM: &str = r#"You are the Evidence Verifier of an autonomous research organization.
For each item you receive a CLAIM, the EXCERPT it was extracted from, surrounding CONTEXT from the source and
metadata about the source. Decide whether the source actually supports the claim as worded.

Be conservative. A claim is only SUPPORTED if the source states exactly what the claim states, at the same scope.
- SUPPORTED: the passage directly establishes the whole claim, including its scope, numbers and qualifiers.
- PARTIALLY_SUPPORTED: the passage supports a narrower or weaker statement (e.g. one task, one population,
  one metric, correlation not causation) than the claim as written. State precisely what IS supported.
- CONTRADICTED: the passage says the opposite of, or materially conflicts with, the claim.
- UNSUPPORTED: the passage is unrelated to the claim or does not establish it.
- UNVERIFIED: the excerpt and context are too thin or ambiguous to judge.
Broad conclusions drawn from narrow evidence are PARTIALLY_SUPPORTED at best. Marketing copy and opinion pieces
do not establish factual claims by themselves.

"confidence" (0-1) is your confidence that the claim, as worded, is true given the source.
Return one verdict per claim_id, using exactly the ids provided.

Reply with ONLY:
{"verdicts": [{"claim_id": "...", "support_level": "SUPPORTED", "confidence": 0.8,
               "reasoning": "one or two sentences naming what the source does and does not establish"}]}"#;

#[derive(Debug, Clone, Serialize)]
pub struct VerifyItem {
    pub claim_id: Uuid,
    pub claim: String,
    pub excerpt: String,
    pub context: String,
    pub source_title: String,
    pub source_type: SourceType,
    pub source_nature: SourceNature,
    pub published_at: Option<String>,
    /// Confidence assigned at extraction time.
    pub extraction_confidence: f32,
}

#[derive(Debug, Clone)]
pub struct VerifyInput {
    pub research_id: Uuid,
    pub items: Vec<VerifyItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub claim_id: Uuid,
    pub support_level: SupportLevel,
    pub confidence: f32,
    pub reasoning: String,
}

#[derive(Deserialize)]
struct RawVerdicts {
    verdicts: Vec<RawVerdict>,
}

#[derive(Deserialize)]
struct RawVerdict {
    claim_id: Uuid,
    support_level: SupportLevel,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    reasoning: String,
}

pub struct VerifierAgent {
    llm: Arc<dyn LlmProvider>,
    limits: Limits,
}

impl VerifierAgent {
    pub fn new(llm: Arc<dyn LlmProvider>, limits: Limits) -> Self {
        Self { llm, limits }
    }

    async fn verify_batch(&self, batch: &[VerifyItem]) -> Vec<Verdict> {
        let payload = serde_json::to_string_pretty(batch).unwrap_or_default();
        let ids: Vec<Uuid> = batch.iter().map(|i| i.claim_id).collect();
        let request = LlmRequest::json(NAME, SYSTEM, format!("Verify these claims:\n{payload}"));
        let result: Result<RawVerdicts, AgentError> =
            generate_structured(self.llm.as_ref(), NAME, request, |r: &RawVerdicts| {
                match r.verdicts.iter().find(|v| !ids.contains(&v.claim_id)) {
                    Some(v) => Err(format!("claim_id {} was not in the request", v.claim_id)),
                    None => Ok(()),
                }
            })
            .await;

        let mut by_id: HashMap<Uuid, RawVerdict> = HashMap::new();
        match result {
            Ok(raw) => {
                for v in raw.verdicts {
                    by_id.entry(v.claim_id).or_insert(v);
                }
            }
            Err(e) => tracing::warn!(error = %e, "verification batch failed; claims stay unverified"),
        }

        batch
            .iter()
            .map(|item| match by_id.remove(&item.claim_id) {
                Some(v) => {
                    let cap = item.source_nature.confidence_cap();
                    Verdict {
                        claim_id: item.claim_id,
                        support_level: v.support_level,
                        confidence: adjust_confidence(v.support_level, v.confidence, cap),
                        reasoning: v.reasoning,
                    }
                }
                // Missing verdicts are never assumed to be positive.
                None => Verdict {
                    claim_id: item.claim_id,
                    support_level: SupportLevel::Unverified,
                    confidence: adjust_confidence(SupportLevel::Unverified, item.extraction_confidence, 1.0),
                    reasoning: "The verifier returned no verdict for this claim.".into(),
                },
            })
            .collect()
    }
}

#[async_trait]
impl Agent for VerifierAgent {
    type Input = VerifyInput;
    type Output = Vec<Verdict>;

    fn name(&self) -> &'static str {
        NAME
    }

    async fn execute(&self, input: VerifyInput) -> Result<Vec<Verdict>, AgentError> {
        let batches = input.items.chunks(self.limits.verify_batch_size.max(1));
        let results = join_all(batches.map(|b| self.verify_batch(b))).await;
        Ok(results.into_iter().flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::structured::testing::ScriptedLlm;

    fn item(nature: SourceNature) -> VerifyItem {
        VerifyItem {
            claim_id: Uuid::new_v4(),
            claim: "AI agents raise developer productivity by 40%.".into(),
            excerpt: "completed one task faster".into(),
            context: "completed one task faster".into(),
            source_title: "Study".into(),
            source_type: SourceType::AcademicPaper,
            source_nature: nature,
            published_at: None,
            extraction_confidence: 0.9,
        }
    }

    fn verdicts(items: &[(&VerifyItem, &str, f32)]) -> String {
        let v: Vec<String> = items
            .iter()
            .map(|(i, level, c)| {
                format!(r#"{{"claim_id":"{}","support_level":"{level}","confidence":{c},"reasoning":"r"}}"#, i.claim_id)
            })
            .collect();
        format!(r#"{{"verdicts":[{}]}}"#, v.join(","))
    }

    #[tokio::test]
    async fn applies_deterministic_confidence_caps() {
        let (a, b) = (item(SourceNature::Primary), item(SourceNature::Marketing));
        let llm =
            Arc::new(ScriptedLlm::ok(&[&verdicts(&[(&a, "PARTIALLY_SUPPORTED", 0.95), (&b, "SUPPORTED", 0.95)])]));
        let out = VerifierAgent::new(llm, Limits::default())
            .execute(VerifyInput { research_id: Uuid::new_v4(), items: vec![a.clone(), b.clone()] })
            .await
            .unwrap();
        assert_eq!(out[0].support_level, SupportLevel::PartiallySupported);
        assert_eq!(out[0].confidence, 0.7);
        assert_eq!(out[1].support_level, SupportLevel::Supported);
        assert_eq!(out[1].confidence, 0.5, "marketing sources cap confidence");
    }

    #[tokio::test]
    async fn missing_verdicts_become_unverified() {
        let (a, b) = (item(SourceNature::Primary), item(SourceNature::Primary));
        let llm = Arc::new(ScriptedLlm::ok(&[&verdicts(&[(&a, "SUPPORTED", 0.9)])]));
        let out = VerifierAgent::new(llm, Limits::default())
            .execute(VerifyInput { research_id: Uuid::new_v4(), items: vec![a, b.clone()] })
            .await
            .unwrap();
        assert_eq!(out[1].claim_id, b.claim_id);
        assert_eq!(out[1].support_level, SupportLevel::Unverified);
        assert!(out[1].confidence <= 0.4);
    }

    #[tokio::test]
    async fn llm_failure_degrades_to_unverified_instead_of_failing_the_run() {
        let a = item(SourceNature::Primary);
        let llm = Arc::new(ScriptedLlm::new(vec![Err(crate::error::LlmError::Timeout)]));
        let out = VerifierAgent::new(llm, Limits::default())
            .execute(VerifyInput { research_id: Uuid::new_v4(), items: vec![a] })
            .await
            .unwrap();
        assert_eq!(out[0].support_level, SupportLevel::Unverified);
    }

    #[tokio::test]
    async fn batches_by_configured_size() {
        let limits = Limits { verify_batch_size: 2, ..Limits::default() };
        let items: Vec<_> = (0..3).map(|_| item(SourceNature::Primary)).collect();
        let r1 = verdicts(&[(&items[0], "SUPPORTED", 0.8), (&items[1], "SUPPORTED", 0.8)]);
        let r2 = verdicts(&[(&items[2], "UNSUPPORTED", 0.1)]);
        let llm = Arc::new(ScriptedLlm::ok(&[&r1, &r2]));
        let out = VerifierAgent::new(llm.clone(), limits)
            .execute(VerifyInput { research_id: Uuid::new_v4(), items })
            .await
            .unwrap();
        assert_eq!(llm.call_count(), 2);
        assert_eq!(out.len(), 3);
    }
}
