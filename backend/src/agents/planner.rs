use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::agent::Agent;
use crate::{
    error::AgentError,
    llm::{LlmProvider, LlmRequest, generate_structured},
    models::Priority,
};

const NAME: &str = "planner";

const SYSTEM: &str = r#"You are the Planner of an autonomous research organization.
Your only job is to turn a broad research question into a structured research plan.
You do NOT research, answer, or speculate about the answer.

Produce 3 to {MAX} focused research questions that together cover the topic: capabilities/state of the art,
empirical evidence, risks or downsides, limitations, economic or social effects, and open debates -- whichever
apply. Questions must be answerable with web sources and must not overlap heavily.

For each question state:
- "question": a specific, searchable question.
- "objective": what finding would answer it.
- "priority": "high", "medium" or "low".
- "required_evidence": 2-4 kinds of evidence needed (e.g. "measurement with sample size", "methodology",
  "primary document").
- "depends_on": zero-based indices of EARLIER questions whose results this one needs. Almost always [].
Also give "research_depth": "shallow", "standard" or "deep".

Reply with ONLY this JSON object:
{"research_depth": "standard",
 "questions": [{"question": "...", "objective": "...", "priority": "high",
                "required_evidence": ["..."], "depends_on": []}]}"#;

#[derive(Debug, Clone)]
pub struct PlannerInput {
    pub research_id: Uuid,
    pub query: String,
    pub max_questions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedQuestion {
    pub question: String,
    pub objective: String,
    pub priority: Priority,
    #[serde(default)]
    pub required_evidence: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchPlan {
    #[serde(default)]
    pub research_depth: String,
    pub questions: Vec<PlannedQuestion>,
}

impl ResearchPlan {
    /// Structural checks: bounded size, non-empty text, acyclic dependencies
    /// (dependencies may only point at earlier questions).
    pub fn validate(&self, max_questions: usize) -> Result<(), String> {
        if self.questions.is_empty() {
            return Err("the plan has no questions".into());
        }
        if self.questions.len() > max_questions {
            return Err(format!("the plan has more than {max_questions} questions"));
        }
        for (i, q) in self.questions.iter().enumerate() {
            if q.question.trim().is_empty() || q.objective.trim().is_empty() {
                return Err(format!("question {i} has an empty question or objective"));
            }
            if let Some(bad) = q.depends_on.iter().find(|d| **d >= i) {
                return Err(format!("question {i} depends on {bad}, which is not an earlier question"));
            }
        }
        Ok(())
    }
}

pub struct PlannerAgent {
    llm: Arc<dyn LlmProvider>,
}

impl PlannerAgent {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Agent for PlannerAgent {
    type Input = PlannerInput;
    type Output = ResearchPlan;

    fn name(&self) -> &'static str {
        NAME
    }

    async fn execute(&self, input: PlannerInput) -> Result<ResearchPlan, AgentError> {
        if input.query.trim().len() < 8 {
            return Err(AgentError::InvalidInput {
                agent: NAME,
                reason: "query is too short to plan".into(),
            });
        }
        let max = input.max_questions.max(3);
        let request = LlmRequest::json(
            NAME,
            SYSTEM.replace("{MAX}", &max.to_string()),
            format!("Research question:\n{}", input.query.trim()),
        );
        generate_structured(self.llm.as_ref(), NAME, request, |p: &ResearchPlan| p.validate(max)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::structured::testing::ScriptedLlm;

    const GOOD: &str = r#"{"research_depth":"standard","questions":[
        {"question":"What can agents do?","objective":"capabilities","priority":"high","required_evidence":["benchmarks"],"depends_on":[]},
        {"question":"What are the risks?","objective":"risks","priority":"medium","depends_on":[0]}]}"#;

    fn input() -> PlannerInput {
        PlannerInput { research_id: Uuid::new_v4(), query: "How will AI coding agents change software?".into(), max_questions: 6 }
    }

    #[tokio::test]
    async fn produces_a_valid_plan() {
        let llm = Arc::new(ScriptedLlm::ok(&[GOOD]));
        let plan = PlannerAgent::new(llm.clone()).execute(input()).await.unwrap();
        assert_eq!(plan.questions.len(), 2);
        assert_eq!(plan.questions[1].depends_on, vec![0]);
        assert!(llm.requests.lock().unwrap()[0].system.contains("Planner"));
    }

    #[tokio::test]
    async fn rejects_forward_dependencies_and_repairs() {
        let bad = r#"{"questions":[{"question":"a","objective":"b","priority":"low","depends_on":[1]},
                                   {"question":"c","objective":"d","priority":"low"}]}"#;
        let llm = Arc::new(ScriptedLlm::ok(&[bad, GOOD]));
        let plan = PlannerAgent::new(llm.clone()).execute(input()).await.unwrap();
        assert_eq!(plan.questions.len(), 2);
        assert_eq!(llm.call_count(), 2);
    }

    #[tokio::test]
    async fn refuses_trivial_queries_without_calling_the_llm() {
        let llm = Arc::new(ScriptedLlm::ok(&[]));
        let mut i = input();
        i.query = "hi".into();
        assert!(matches!(PlannerAgent::new(llm.clone()).execute(i).await, Err(AgentError::InvalidInput { .. })));
        assert_eq!(llm.call_count(), 0);
    }

    #[test]
    fn plan_size_is_bounded() {
        let q = |_| PlannedQuestion { question: "q".into(), objective: "o".into(), priority: Priority::Low, required_evidence: vec![], depends_on: vec![] };
        let plan = ResearchPlan { research_depth: String::new(), questions: (0..5).map(q).collect() };
        assert!(plan.validate(4).is_err());
        assert!(plan.validate(5).is_ok());
    }
}
