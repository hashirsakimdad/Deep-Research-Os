use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::agent::Agent;
use crate::{
    error::AgentError,
    llm::{LlmProvider, LlmRequest, generate_structured},
    models::ContradictionKind,
};

const NAME: &str = "contradiction_detector";

const SYSTEM: &str = r#"You are the Contradiction Detector of an autonomous research organization.
You receive verified claims that all bear on the SAME research question, each from a numbered source.
Find pairs of claims from DIFFERENT sources that appear to disagree, and classify each pair.

Apparent disagreement is often not real disagreement. Classify as:
- DIRECT_CONTRADICTION: the claims cannot both be true about the same thing, population, period and metric.
- CONTEXT_DIFFERENCE: they hold in different settings or conditions (e.g. lab task vs. production codebase).
- TIME_DIFFERENCE: they describe different time periods or technology generations.
- POPULATION_DIFFERENCE: they describe different groups (e.g. novices vs. experts).
- METRIC_DIFFERENCE: they measure different things (e.g. speed vs. correctness).
- SCOPE_DIFFERENCE: one is broader or narrower than the other.
- NO_CONFLICT: they are compatible. Do NOT list these.

Only list pairs that deserve a reader's attention. Explain in one or two sentences exactly why the pair is
classified as it is, naming the dimension on which they differ. Never decide which claim is right.
Use the claim ids exactly as given. If nothing conflicts, return an empty list.

Reply with ONLY:
{"pairs": [{"claim_a": "...", "claim_b": "...", "kind": "METRIC_DIFFERENCE", "explanation": "..."}]}"#;

#[derive(Debug, Clone, Serialize)]
pub struct ClaimBrief {
    pub id: Uuid,
    pub text: String,
    pub source_id: Uuid,
    pub source_title: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaimGroup {
    pub question: String,
    pub claims: Vec<ClaimBrief>,
}

#[derive(Debug, Clone)]
pub struct ContradictionInput {
    pub research_id: Uuid,
    pub groups: Vec<ClaimGroup>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContradictionFinding {
    pub claim_a: Uuid,
    pub claim_b: Uuid,
    pub kind: ContradictionKind,
    pub explanation: String,
}

#[derive(Deserialize)]
struct RawPairs {
    pairs: Vec<RawPair>,
}

#[derive(Deserialize)]
struct RawPair {
    claim_a: Uuid,
    claim_b: Uuid,
    kind: ContradictionKind,
    #[serde(default)]
    explanation: String,
}

pub struct ContradictionAgent {
    llm: Arc<dyn LlmProvider>,
}

impl ContradictionAgent {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }

    async fn analyze_group(&self, group: &ClaimGroup) -> Vec<ContradictionFinding> {
        if group.claims.len() < 2 {
            return vec![];
        }
        let sources: HashMap<Uuid, Uuid> = group.claims.iter().map(|c| (c.id, c.source_id)).collect();
        let payload = serde_json::to_string(&group.claims).unwrap_or_default();
        let request =
            LlmRequest::json(NAME, SYSTEM, format!("Research question: {}\n\nClaims:\n{payload}", group.question));
        let result: Result<RawPairs, AgentError> =
            generate_structured(self.llm.as_ref(), NAME, request, |r: &RawPairs| {
                for p in &r.pairs {
                    let (Some(sa), Some(sb)) = (sources.get(&p.claim_a), sources.get(&p.claim_b)) else {
                        return Err("a claim id is not in the request".into());
                    };
                    if p.claim_a == p.claim_b || sa == sb {
                        return Err("pairs must be two claims from different sources".into());
                    }
                }
                Ok(())
            })
            .await;

        match result {
            Ok(raw) => raw
                .pairs
                .into_iter()
                .filter(|p| p.kind.is_notable())
                .map(|p| ContradictionFinding {
                    claim_a: p.claim_a,
                    claim_b: p.claim_b,
                    kind: p.kind,
                    explanation: p.explanation.trim().to_string(),
                })
                .collect(),
            Err(e) => {
                tracing::warn!(question = %group.question, error = %e, "contradiction analysis failed for group");
                vec![]
            }
        }
    }
}

#[async_trait]
impl Agent for ContradictionAgent {
    type Input = ContradictionInput;
    type Output = Vec<ContradictionFinding>;

    fn name(&self) -> &'static str {
        NAME
    }

    async fn execute(&self, input: ContradictionInput) -> Result<Vec<ContradictionFinding>, AgentError> {
        let results = join_all(input.groups.iter().map(|g| self.analyze_group(g))).await;
        Ok(results.into_iter().flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::structured::testing::ScriptedLlm;

    fn brief(text: &str) -> ClaimBrief {
        ClaimBrief {
            id: Uuid::new_v4(),
            text: text.into(),
            source_id: Uuid::new_v4(),
            source_title: "S".into(),
            published_at: None,
        }
    }

    fn input(claims: Vec<ClaimBrief>) -> ContradictionInput {
        ContradictionInput { research_id: Uuid::new_v4(), groups: vec![ClaimGroup { question: "q".into(), claims }] }
    }

    #[tokio::test]
    async fn separates_real_conflicts_from_metric_differences() {
        let (a, b, c) = (brief("Faster completion"), brief("More debugging time"), brief("Slower completion"));
        let reply = format!(
            r#"{{"pairs":[
              {{"claim_a":"{a}","claim_b":"{b}","kind":"METRIC_DIFFERENCE","explanation":"speed vs debugging"}},
              {{"claim_a":"{a}","claim_b":"{c}","kind":"DIRECT_CONTRADICTION","explanation":"same metric, opposite sign"}},
              {{"claim_a":"{b}","claim_b":"{c}","kind":"NO_CONFLICT","explanation":"compatible"}}]}}"#,
            a = a.id,
            b = b.id,
            c = c.id
        );
        let llm = Arc::new(ScriptedLlm::ok(&[&reply]));
        let out = ContradictionAgent::new(llm).execute(input(vec![a.clone(), b, c.clone()])).await.unwrap();
        assert_eq!(out.len(), 2, "NO_CONFLICT pairs are dropped");
        assert_eq!(out[0].kind, ContradictionKind::MetricDifference);
        assert!(out[1].kind.is_genuine());
        assert_eq!((out[1].claim_a, out[1].claim_b), (a.id, c.id));
    }

    #[tokio::test]
    async fn rejects_unknown_ids_and_same_source_pairs() {
        let (a, mut b) = (brief("x"), brief("y"));
        b.source_id = a.source_id;
        let same_source = format!(
            r#"{{"pairs":[{{"claim_a":"{}","claim_b":"{}","kind":"DIRECT_CONTRADICTION","explanation":"e"}}]}}"#,
            a.id, b.id
        );
        let unknown = format!(
            r#"{{"pairs":[{{"claim_a":"{}","claim_b":"{}","kind":"DIRECT_CONTRADICTION","explanation":"e"}}]}}"#,
            a.id,
            Uuid::new_v4()
        );
        let llm = Arc::new(ScriptedLlm::ok(&[&same_source, &unknown, r#"{"pairs":[]}"#]));
        let out = ContradictionAgent::new(llm.clone()).execute(input(vec![a, b])).await.unwrap();
        assert!(out.is_empty());
        assert_eq!(llm.call_count(), 3);
    }

    #[tokio::test]
    async fn skips_groups_with_fewer_than_two_claims() {
        let llm = Arc::new(ScriptedLlm::ok(&[]));
        let out = ContradictionAgent::new(llm.clone()).execute(input(vec![brief("only one")])).await.unwrap();
        assert!(out.is_empty());
        assert_eq!(llm.call_count(), 0);
    }
}
