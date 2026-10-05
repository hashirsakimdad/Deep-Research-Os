//! The research lifecycle:
//! plan -> research (concurrent) -> verify -> contradictions -> synthesize ->
//! critique -> (more research | final report), bounded by `max_iterations`.

use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use serde_json::json;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    events::EventBus,
    review::{self, FollowUp, ReviewDecision, ReviewInput},
    scheduler::run_tasks,
    state::ResearchState,
};
use crate::{
    agents::{
        Agent, ContradictionAgent, CriticAgent, EventSink, PlannerAgent, ResearcherAgent, SynthesizerAgent,
        VerifierAgent,
        contradiction::{ClaimBrief, ClaimGroup, ContradictionInput},
        critic::{CritiqueClaim, CritiqueDraft, CritiqueInput},
        planner::{PlannerInput, ResearchPlan},
        researcher::{ResearchFindings, ResearchInput},
        synthesizer::{SynthClaim, SynthContradiction, SynthesisInput},
        verifier::{VerifyInput, VerifyItem},
    },
    config::Limits,
    database::{ClaimRecord, Repository},
    error::{AgentError, OrchestratorError},
    llm::{LimitedProvider, LlmProvider},
    models::*,
    tools::{Fetcher, SearchProvider},
    util::text::{context_around, truncate_chars},
};

pub struct Orchestrator {
    repo: Repository,
    events: Arc<EventBus>,
    planner: PlannerAgent,
    researcher: ResearcherAgent,
    verifier: VerifierAgent,
    contradiction: ContradictionAgent,
    synthesizer: SynthesizerAgent,
    critic: CriticAgent,
    limits: Limits,
    run_permits: Semaphore,
    cancel: CancellationToken,
}

fn storage(e: sqlx::Error) -> AgentError {
    AgentError::Storage { agent: "orchestrator", reason: e.to_string() }
}

/// Turns a validated plan into persisted-shape questions and one task each,
/// translating index-based dependencies into task ids.
pub fn build_plan_records(
    run_id: Uuid,
    plan: &ResearchPlan,
    max_tasks: usize,
) -> (Vec<ResearchQuestion>, Vec<ResearchTask>) {
    let planned = &plan.questions[..plan.questions.len().min(max_tasks.max(1))];
    let questions: Vec<ResearchQuestion> = planned
        .iter()
        .enumerate()
        .map(|(i, q)| ResearchQuestion {
            id: Uuid::new_v4(),
            research_run_id: run_id,
            question: q.question.trim().to_string(),
            objective: q.objective.trim().to_string(),
            priority: q.priority,
            position: i as i32,
        })
        .collect();
    let task_ids: Vec<Uuid> = planned.iter().map(|_| Uuid::new_v4()).collect();
    let tasks = planned
        .iter()
        .enumerate()
        .map(|(i, q)| ResearchTask {
            id: task_ids[i],
            research_run_id: run_id,
            question_id: Some(questions[i].id),
            question: questions[i].question.clone(),
            objective: questions[i].objective.clone(),
            priority: q.priority,
            status: TaskStatus::Pending,
            assigned_agent: "researcher".into(),
            required_evidence: q.required_evidence.clone(),
            dependencies: q.depends_on.iter().filter_map(|d| task_ids.get(*d).copied()).collect(),
            iteration: 1,
            error: None,
            created_at: Utc::now(),
            completed_at: None,
        })
        .collect();
    (questions, tasks)
}

pub fn follow_up_task(run_id: Uuid, f: &FollowUp, iteration: i32) -> ResearchTask {
    ResearchTask {
        id: Uuid::new_v4(),
        research_run_id: run_id,
        question_id: f.question_id,
        question: f.question.clone(),
        objective: "Close a gap identified in the previous iteration".into(),
        priority: Priority::High,
        status: TaskStatus::Pending,
        assigned_agent: "researcher".into(),
        required_evidence: vec!["primary source".into(), "methodology and limitations".into()],
        dependencies: vec![],
        iteration,
        error: None,
        created_at: Utc::now(),
        completed_at: None,
    }
}

impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repo: Repository,
        events: Arc<EventBus>,
        llm: Arc<dyn LlmProvider>,
        search: Arc<dyn SearchProvider>,
        fetcher: Arc<dyn Fetcher>,
        limits: Limits,
        cancel: CancellationToken,
    ) -> Self {
        // One shared limiter bounds LLM calls across every agent and run.
        let llm: Arc<dyn LlmProvider> = Arc::new(LimitedProvider::new(llm, limits.max_concurrent_llm_calls));
        let sink: Arc<dyn EventSink> = events.clone();
        Self {
            planner: PlannerAgent::new(llm.clone()),
            researcher: ResearcherAgent::new(llm.clone(), search, fetcher, sink, limits.clone()),
            verifier: VerifierAgent::new(llm.clone(), limits.clone()),
            contradiction: ContradictionAgent::new(llm.clone()),
            synthesizer: SynthesizerAgent::new(llm.clone()),
            critic: CriticAgent::new(llm),
            run_permits: Semaphore::new(limits.max_concurrent_runs.max(1)),
            repo,
            events,
            limits,
            cancel,
        }
    }

    pub fn events(&self) -> &Arc<EventBus> {
        &self.events
    }

    /// Creates the run and continues it in the background.
    pub async fn start(self: &Arc<Self>, query: &str) -> Result<ResearchRun, sqlx::Error> {
        let run = self.repo.create_run(query).await?;
        let this = Arc::clone(self);
        let spawned = run.clone();
        tokio::spawn(async move {
            let Ok(_permit) = this.run_permits.acquire().await else { return };
            this.run(spawned).await;
        });
        Ok(run)
    }

    /// Runs to completion; never returns an error, failures are recorded.
    pub async fn run(&self, run: ResearchRun) {
        let id = run.id;
        match self.execute(&run).await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(research_id = %id, error = %e, "research run failed");
                let iteration = self.repo.get_run(id).await.ok().flatten().map(|r| r.iteration).unwrap_or(1);
                if let Err(db) =
                    self.repo.set_run_status(id, ResearchStatus::Failed, iteration, Some(&e.to_string())).await
                {
                    tracing::error!(error = %db, "could not record run failure");
                }
                self.ev(
                    id,
                    "orchestrator",
                    "research_failed",
                    format!("Research failed: {e}"),
                    json!({"error": e.to_string()}),
                )
                .await;
            }
        }
    }

    async fn ev(&self, id: Uuid, agent: &str, event_type: &str, message: String, metadata: serde_json::Value) {
        self.events.emit(id, agent, event_type, message, metadata).await;
    }

    async fn advance(&self, state: &mut ResearchState, to: ResearchStatus) -> Result<(), OrchestratorError> {
        state.transition(to)?;
        self.repo.set_run_status(state.research_id, to, state.iteration as i32, None).await?;
        Ok(())
    }

    async fn execute(&self, run: &ResearchRun) -> Result<(), OrchestratorError> {
        let id = run.id;
        let mut state = ResearchState::new(id, run.query.clone(), self.limits.max_iterations);
        self.ev(
            id,
            "orchestrator",
            "research_started",
            "Research run started".into(),
            json!({"query": run.query, "max_iterations": state.max_iterations}),
        )
        .await;

        // ---- plan -------------------------------------------------------
        self.ev(id, "planner", "planning_started", "Planning research questions".into(), json!({})).await;
        let plan = self
            .planner
            .execute(PlannerInput {
                research_id: id,
                query: run.query.clone(),
                max_questions: self.limits.max_tasks_per_iteration,
            })
            .await?;
        let (questions, tasks) = build_plan_records(id, &plan, self.limits.max_tasks_per_iteration);
        self.repo.insert_questions(&questions).await?;
        for t in &tasks {
            self.repo.insert_task(t).await?;
            self.ev(
                id,
                "planner",
                "task_created",
                format!("Task: {}", t.question),
                json!({"task_id": t.id, "priority": t.priority, "iteration": 1}),
            )
            .await;
        }
        self.ev(
            id,
            "planner",
            "planning_completed",
            format!("Created {} research questions", questions.len()),
            json!({"questions": questions.len(), "depth": plan.research_depth}),
        )
        .await;
        state.research_questions = questions;
        state.tasks = tasks;
        self.advance(&mut state, ResearchStatus::Researching).await?;

        let mut reviewer_feedback: Vec<String> = Vec::new();
        loop {
            // ---- research ------------------------------------------------
            self.research_phase(&mut state).await?;

            // ---- verify --------------------------------------------------
            self.advance(&mut state, ResearchStatus::Verifying).await?;
            self.verify_phase(&state).await?;

            // ---- contradictions -----------------------------------------
            self.advance(&mut state, ResearchStatus::Analyzing).await?;
            self.contradiction_phase(&state).await;

            // ---- synthesize ---------------------------------------------
            self.advance(&mut state, ResearchStatus::Synthesizing).await?;
            let (report, records) = self.synthesis_phase(&mut state, &reviewer_feedback).await?;

            // ---- critique -----------------------------------------------
            self.advance(&mut state, ResearchStatus::Critiquing).await?;
            let (critique, critic_available) = self.critique_phase(&mut state, &report, &records).await?;

            let uncovered = review::uncovered_questions(
                &state.research_questions.iter().map(|q| (q.id, q.question.clone())).collect::<Vec<_>>(),
                records.iter().map(|r| (r.question_id, r.claim.status)),
            );
            let decision = review::decide(&ReviewInput {
                quality: critique.quality,
                critic_wants_more: critique.needs_more_research,
                required_research: &critique.required_research,
                issues: &critique.issues,
                uncovered_questions: &uncovered,
                iteration: state.iteration,
                max_iterations: state.max_iterations,
                max_follow_ups: self.limits.max_tasks_per_iteration,
            });

            match decision {
                ReviewDecision::Iterate { follow_ups } => {
                    self.ev(
                        id,
                        "orchestrator",
                        "additional_research_required",
                        format!("{} follow-up research task(s) needed", follow_ups.len()),
                        json!({"follow_ups": follow_ups.iter().map(|f| &f.question).collect::<Vec<_>>()}),
                    )
                    .await;
                    reviewer_feedback = critique
                        .issues
                        .iter()
                        .filter(|i| i.severity != Severity::Low)
                        .map(|i| format!("[{}] {}", i.kind, i.description))
                        .collect();
                    state.begin_next_iteration()?;
                    self.repo.set_run_status(id, ResearchStatus::Researching, state.iteration as i32, None).await?;
                    self.ev(
                        id,
                        "orchestrator",
                        "iteration_started",
                        format!("Starting iteration {}", state.iteration),
                        json!({"iteration": state.iteration}),
                    )
                    .await;
                    for f in &follow_ups {
                        let task = follow_up_task(id, f, state.iteration as i32);
                        self.repo.insert_task(&task).await?;
                        self.ev(
                            id,
                            "orchestrator",
                            "task_created",
                            format!("Task: {}", task.question),
                            json!({"task_id": task.id, "iteration": task.iteration, "follow_up": true}),
                        )
                        .await;
                        state.tasks.push(task);
                    }
                }
                decision => {
                    self.finalize(
                        &mut state,
                        &report,
                        &critique,
                        &uncovered,
                        critic_available,
                        decision == ReviewDecision::AcceptWithLimitations,
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    }

    // ---------------------------------------------------------------------

    async fn research_phase(&self, state: &mut ResearchState) -> Result<(), OrchestratorError> {
        let id = state.research_id;
        let iteration = state.iteration as i32;
        let batch: Vec<ResearchTask> = state
            .tasks
            .iter()
            .filter(|t| t.iteration == iteration && t.status == TaskStatus::Pending)
            .cloned()
            .collect();
        self.ev(
            id,
            "orchestrator",
            "parallel_research_started",
            format!(
                "Running {} research task(s) concurrently (limit {})",
                batch.len(),
                self.limits.max_concurrent_tasks
            ),
            json!({"tasks": batch.len(), "iteration": iteration}),
        )
        .await;

        let reports =
            run_tasks(batch, self.limits.max_concurrent_tasks, self.limits.task_timeout, &self.cancel, |task| {
                self.research_task(task)
            })
            .await;

        let (mut ok, mut failed) = (0usize, 0usize);
        for r in reports {
            let new_status = match &r.outcome {
                Ok(_) => {
                    ok += 1;
                    TaskStatus::Completed
                }
                Err(e) => {
                    failed += 1;
                    self.repo.set_task_status(r.task.id, TaskStatus::Failed, Some(&e.to_string())).await?;
                    self.ev(
                        id,
                        "researcher",
                        "task_failed",
                        format!("Task failed: {} ({e})", r.task.question),
                        json!({"task_id": r.task.id, "error": e.to_string()}),
                    )
                    .await;
                    TaskStatus::Failed
                }
            };
            if let Some(t) = state.tasks.iter_mut().find(|t| t.id == r.task.id) {
                t.status = new_status;
            }
        }
        if self.cancel.is_cancelled() {
            return Err(OrchestratorError::Cancelled(id));
        }
        if ok == 0 {
            // A failed first pass leaves nothing to report on. A failed follow-up pass does not:
            // the evidence from earlier iterations stands and the gap is stated in the report.
            if iteration == 1 {
                return Err(OrchestratorError::AllTasksFailed(format!(
                    "{failed} task(s) failed in iteration {iteration}"
                )));
            }
            self.ev(
                id,
                "orchestrator",
                "follow_up_research_failed",
                format!("All {failed} follow-up task(s) failed; continuing with evidence from earlier iterations"),
                json!({"iteration": iteration, "failed": failed}),
            )
            .await;
        }
        state.sources = self.repo.list_source_refs(id).await?;
        Ok(())
    }

    /// Executes one task and persists its sources and claims as it goes.
    async fn research_task(&self, task: ResearchTask) -> Result<(usize, usize), AgentError> {
        let id = task.research_run_id;
        self.repo.set_task_status(task.id, TaskStatus::Running, None).await.map_err(storage)?;
        self.ev(
            id,
            "researcher",
            "task_started",
            format!("Researching: {}", task.question),
            json!({"task_id": task.id}),
        )
        .await;

        let findings: ResearchFindings = self
            .researcher
            .execute(ResearchInput {
                research_id: id,
                task_id: task.id,
                question: task.question.clone(),
                objective: task.objective.clone(),
                required_evidence: task.required_evidence.clone(),
            })
            .await?;

        // Sources: deduplicated per run by normalized URL; remap ids.
        let mut source_ids: HashMap<Uuid, Uuid> = HashMap::new();
        for s in &findings.sources {
            let stored = self.repo.upsert_source(s).await.map_err(storage)?;
            source_ids.insert(s.id, stored);
            self.ev(
                id,
                "researcher",
                "source_found",
                format!("Source: {}", s.title),
                json!({"task_id": task.id, "source_id": stored, "url": s.url, "source_type": s.source_type, "nature": s.nature}),
            )
            .await;
        }

        let mut claims = 0;
        for c in &findings.claims {
            let Some(source_id) = source_ids.get(&c.source_id).copied() else { continue };
            let claim = Claim {
                id: c.id,
                research_run_id: id,
                research_task_id: task.id,
                text: c.text.clone(),
                source_id,
                confidence: c.confidence,
                status: SupportLevel::Unverified,
                iteration: task.iteration,
                created_at: Utc::now(),
            };
            if let Some(claim_id) = self.repo.insert_claim_with_evidence(&claim, &c.excerpt).await.map_err(storage)? {
                claims += 1;
                self.ev(
                    id,
                    "researcher",
                    "claim_extracted",
                    c.text.clone(),
                    json!({"task_id": task.id, "claim_id": claim_id, "source_id": source_id}),
                )
                .await;
            }
        }
        self.repo.set_task_status(task.id, TaskStatus::Completed, None).await.map_err(storage)?;
        self.ev(
            id,
            "researcher",
            "task_completed",
            format!("Task done: {} source(s), {} claim(s)", findings.sources.len(), claims),
            json!({"task_id": task.id, "sources": findings.sources.len(), "claims": claims, "rejected_claims": findings.rejected_claims}),
        )
        .await;
        Ok((findings.sources.len(), claims))
    }

    async fn verify_phase(&self, state: &ResearchState) -> Result<(), OrchestratorError> {
        let id = state.research_id;
        let pending = self.repo.claim_records(id, Some(SupportLevel::Unverified), Some(state.iteration as i32)).await?;
        self.ev(
            id,
            "verifier",
            "verification_started",
            format!("Verifying {} claim(s)", pending.len()),
            json!({"claims": pending.len()}),
        )
        .await;

        let mut contents: HashMap<Uuid, String> = HashMap::new();
        let mut items = Vec::with_capacity(pending.len());
        for r in &pending {
            if !contents.contains_key(&r.source.id)
                && let Some(s) = self.repo.get_source(r.source.id).await?
            {
                contents.insert(s.id, s.content);
            }
            let excerpt = r.excerpt();
            let context =
                contents.get(&r.source.id).map(|c| context_around(c, &excerpt, 500)).unwrap_or_else(|| excerpt.clone());
            items.push(VerifyItem {
                claim_id: r.claim.id,
                claim: r.claim.text.clone(),
                excerpt,
                context,
                source_title: r.source.title.clone(),
                source_type: r.source.source_type,
                source_nature: r.source.nature,
                published_at: r.source.published_at.clone(),
                extraction_confidence: r.claim.confidence,
            });
        }

        let verdicts = self.verifier.execute(VerifyInput { research_id: id, items }).await?;
        for v in &verdicts {
            self.repo.set_claim_verification(v.claim_id, v.support_level, v.confidence, &v.reasoning).await?;
        }
        let stats = self.repo.run_stats(id).await?;
        self.ev(
            id,
            "verifier",
            "verification_completed",
            format!(
                "{} supported, {} partially supported, {} unverified, {} unsupported, {} contradicted",
                stats.supported, stats.partially_supported, stats.unverified, stats.unsupported, stats.contradicted
            ),
            serde_json::to_value(&stats).unwrap_or_default(),
        )
        .await;
        Ok(())
    }

    /// Soft-fails: a failed analysis leaves the run without contradiction
    /// annotations rather than abandoning verified research.
    async fn contradiction_phase(&self, state: &ResearchState) {
        let id = state.research_id;
        let iteration = state.iteration as i32;
        let records = match self.repo.claim_records(id, None, None).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "could not load claims for contradiction analysis");
                return;
            }
        };
        let mut by_question: HashMap<Option<Uuid>, Vec<&ClaimRecord>> = HashMap::new();
        for r in records.iter().filter(|r| r.claim.status.is_citable()) {
            by_question.entry(r.question_id).or_default().push(r);
        }
        // Only groups containing something new are worth another comparison.
        let mut groups = Vec::new();
        for (_, mut claims) in by_question {
            if !claims.iter().any(|c| c.claim.iteration == iteration) {
                continue;
            }
            claims.sort_by(|a, b| {
                (b.claim.iteration == iteration)
                    .cmp(&(a.claim.iteration == iteration))
                    .then(b.claim.confidence.total_cmp(&a.claim.confidence))
            });
            claims.truncate(self.limits.contradiction_group_size);
            groups.push(ClaimGroup {
                question: claims[0].question.clone(),
                claims: claims
                    .iter()
                    .map(|r| ClaimBrief {
                        id: r.claim.id,
                        text: r.claim.text.clone(),
                        source_id: r.source.id,
                        source_title: r.source.title.clone(),
                        published_at: r.source.published_at.clone(),
                    })
                    .collect(),
            });
        }
        self.ev(
            id,
            "contradiction_detector",
            "contradiction_analysis_started",
            format!("Comparing claims in {} group(s)", groups.len()),
            json!({"groups": groups.len()}),
        )
        .await;

        let findings = match self.contradiction.execute(ContradictionInput { research_id: id, groups }).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, "contradiction analysis failed");
                return;
            }
        };
        let mut recorded = 0;
        for f in findings {
            let c = Contradiction {
                id: Uuid::new_v4(),
                research_run_id: id,
                claim_a_id: f.claim_a,
                claim_b_id: f.claim_b,
                kind: f.kind,
                explanation: f.explanation.clone(),
                created_at: Utc::now(),
            };
            match self.repo.insert_contradiction(&c).await {
                Ok(true) => {
                    recorded += 1;
                    self.ev(
                        id,
                        "contradiction_detector",
                        "contradiction_found",
                        format!("{}: {}", f.kind, f.explanation),
                        json!({"claim_a": f.claim_a, "claim_b": f.claim_b, "kind": f.kind, "genuine": f.kind.is_genuine()}),
                    )
                    .await;
                }
                Ok(false) => {}
                Err(e) => tracing::error!(error = %e, "could not store contradiction"),
            }
        }
        self.ev(
            id,
            "contradiction_detector",
            "contradiction_analysis_completed",
            format!("{recorded} notable disagreement(s) recorded"),
            json!({"recorded": recorded}),
        )
        .await;
    }

    async fn synthesis_phase(
        &self,
        state: &mut ResearchState,
        reviewer_feedback: &[String],
    ) -> Result<(Report, Vec<ClaimRecord>), OrchestratorError> {
        let id = state.research_id;
        let mut records: Vec<ClaimRecord> = self.repo.claim_records(id, None, None).await?;
        let excluded = records.iter().filter(|r| !r.claim.status.is_citable()).count();
        records.retain(|r| r.claim.status.is_citable());
        if records.is_empty() {
            return Err(OrchestratorError::Other(
                "no claim survived verification; there is no evidence to build a report on".into(),
            ));
        }
        // Strongest evidence first, then cap to bound prompt size.
        records.sort_by(|a, b| {
            (b.claim.status == SupportLevel::Supported)
                .cmp(&(a.claim.status == SupportLevel::Supported))
                .then(b.claim.confidence.total_cmp(&a.claim.confidence))
        });
        records.truncate(self.limits.report_claims);

        let contradictions: Vec<Contradiction> = self
            .repo
            .list_contradictions(id)
            .await?
            .into_iter()
            .filter(|c| {
                records.iter().any(|r| r.claim.id == c.claim_a_id) && records.iter().any(|r| r.claim.id == c.claim_b_id)
            })
            .collect();

        let uncovered = review::uncovered_questions(
            &state.research_questions.iter().map(|q| (q.id, q.question.clone())).collect::<Vec<_>>(),
            records.iter().map(|r| (r.question_id, r.claim.status)),
        );
        let mut gaps: Vec<String> =
            uncovered.iter().map(|(_, q)| format!("No verified evidence found for: {q}")).collect();
        let failed_tasks = state.tasks.iter().filter(|t| t.status == TaskStatus::Failed).count();
        if failed_tasks > 0 {
            gaps.push(format!("{failed_tasks} research task(s) failed, so their questions may be under-covered"));
        }
        if excluded > 0 {
            gaps.push(format!("{excluded} extracted claim(s) were excluded because they could not be verified"));
        }

        self.ev(
            id,
            "synthesizer",
            "synthesis_started",
            format!("Synthesizing report from {} verified claim(s)", records.len()),
            json!({"claims": records.len(), "contradictions": contradictions.len()}),
        )
        .await;
        let content = self
            .synthesizer
            .execute(SynthesisInput {
                research_id: id,
                query: state.query.clone(),
                questions: state.research_questions.iter().map(|q| q.question.clone()).collect(),
                claims: records
                    .iter()
                    .map(|r| SynthClaim {
                        id: r.claim.id,
                        text: r.claim.text.clone(),
                        research_question: truncate_chars(&r.question, 90).to_string(),
                        support_level: r.claim.status,
                        support_note: truncate_chars(&r.support_note(), 220).to_string(),
                        confidence: r.claim.confidence,
                        source_title: r.source.title.clone(),
                        source_nature: r.source.nature,
                        published_at: r.source.published_at.clone(),
                    })
                    .collect(),
                contradictions: contradictions
                    .iter()
                    .map(|c| SynthContradiction {
                        claim_a: c.claim_a_id,
                        claim_b: c.claim_b_id,
                        kind: c.kind,
                        explanation: c.explanation.clone(),
                    })
                    .collect(),
                gaps,
                reviewer_feedback: reviewer_feedback.to_vec(),
                iteration: state.iteration as i32,
            })
            .await?;

        let report = self.store_report(id, state.iteration as i32, content, &records, false).await?;
        state.draft_report_id = Some(report.id);
        state.verified_claims = records
            .iter()
            .map(|r| VerifiedClaim { id: r.claim.id, support_level: r.claim.status, confidence: r.claim.confidence })
            .collect();
        state.contradiction_ids = contradictions.iter().map(|c| c.id).collect();
        self.ev(
            id,
            "synthesizer",
            "synthesis_completed",
            format!("Draft report written ({} citations)", report.citations.len()),
            json!({"report_id": report.id, "citations": report.citations.len()}),
        )
        .await;
        Ok((report, records))
    }

    async fn store_report(
        &self,
        run_id: Uuid,
        iteration: i32,
        content: ReportContent,
        records: &[ClaimRecord],
        is_final: bool,
    ) -> Result<Report, OrchestratorError> {
        // Citation numbers and URLs come only from the claim -> source mapping.
        let mut index = CitationIndex::new(
            records.iter().map(|r| (r.claim.id, r.source.id, r.source.url.clone(), r.source.title.clone())),
        );
        let query = self.repo.get_run(run_id).await?.map(|r| r.query).unwrap_or_default();
        let (markdown, citations) = render_markdown(&query, &content, &mut index);
        let report = Report {
            id: Uuid::new_v4(),
            research_run_id: run_id,
            iteration,
            is_final,
            content,
            markdown,
            citations,
            created_at: Utc::now(),
        };
        self.repo.insert_report(&report).await?;
        Ok(report)
    }

    async fn critique_phase(
        &self,
        state: &mut ResearchState,
        report: &Report,
        records: &[ClaimRecord],
    ) -> Result<(Critique, bool), OrchestratorError> {
        let id = state.research_id;
        self.ev(id, "critic", "critique_started", "Critiquing draft report".into(), json!({"report_id": report.id}))
            .await;

        let contradictions = self.repo.list_contradictions(id).await?;
        let support: HashMap<Uuid, SupportLevel> = records.iter().map(|r| (r.claim.id, r.claim.status)).collect();
        let stats = self.repo.run_stats(id).await?;
        let excluded = (stats.claims - stats.supported - stats.partially_supported).max(0) as usize;

        let draft = self
            .critic
            .execute(CritiqueInput {
                research_id: id,
                query: state.query.clone(),
                questions: state.research_questions.iter().map(|q| q.question.clone()).collect(),
                report: report.content.clone(),
                claims: records
                    .iter()
                    .map(|r| CritiqueClaim {
                        id: r.claim.id,
                        text: r.claim.text.clone(),
                        support_level: r.claim.status,
                        support_note: truncate_chars(&r.support_note(), 220).to_string(),
                        source_nature: r.source.nature.to_string(),
                    })
                    .collect(),
                contradiction_notes: contradictions.iter().map(|c| format!("{}: {}", c.kind, c.explanation)).collect(),
                unverified_or_rejected_count: excluded,
            })
            .await;

        let (draft, available) = match draft {
            Ok(d) => (d, true),
            Err(e) => {
                tracing::warn!(error = %e, "critic failed; finishing with explicit caveat");
                (
                    CritiqueDraft {
                        quality: 0.0,
                        needs_more_research: false,
                        issues: vec![],
                        required_research: vec![],
                    },
                    false,
                )
            }
        };

        let mut issues = draft.issues;
        issues.extend(review::deterministic_issues(&report.content, &support, &contradictions));
        let critique = Critique {
            id: Uuid::new_v4(),
            research_run_id: id,
            report_id: report.id,
            iteration: state.iteration as i32,
            quality: draft.quality,
            needs_more_research: draft.needs_more_research,
            issues,
            required_research: draft.required_research,
            created_at: Utc::now(),
        };
        self.repo.insert_critique(&critique).await?;
        state.critique = Some(critique.clone());
        self.ev(
            id,
            "critic",
            "critique_completed",
            if available {
                format!("Quality {:.2}, {} issue(s), more research needed: {}", critique.quality, critique.issues.len(), critique.needs_more_research)
            } else {
                "Critic unavailable; report will carry an explicit caveat".into()
            },
            json!({"quality": critique.quality, "issues": critique.issues.len(), "needs_more_research": critique.needs_more_research, "critic_available": available}),
        )
        .await;
        Ok((critique, available))
    }

    async fn finalize(
        &self,
        state: &mut ResearchState,
        report: &Report,
        critique: &Critique,
        uncovered: &[(Uuid, String)],
        critic_available: bool,
        with_limitations: bool,
    ) -> Result<(), OrchestratorError> {
        let id = state.research_id;
        if with_limitations {
            // Outstanding problems are stated in the report, never hidden.
            let mut content = report.content.clone();
            if !critic_available {
                content.limitations.push(
                    "Automated critique of this report was unavailable; it has not been independently reviewed.".into(),
                );
            }
            for i in critique.issues.iter().filter(|i| i.severity != Severity::Low) {
                content.limitations.push(format!("Reviewer-noted weakness ({}): {}", i.kind, i.description));
            }
            for (_, q) in uncovered {
                content.limitations.push(format!("No verified evidence was found for: {q}"));
            }
            for q in &critique.required_research {
                content.open_questions.push(q.clone());
            }
            let records = self.repo.claim_records(id, None, None).await?;
            let citable: Vec<ClaimRecord> = records.into_iter().filter(|r| r.claim.status.is_citable()).collect();
            self.store_report(id, state.iteration as i32, content, &citable, true).await?;
        } else {
            self.repo.mark_report_final(report.id).await?;
        }
        self.advance(state, ResearchStatus::Completed).await?;
        let stats = self.repo.run_stats(id).await?;
        self.ev(
            id,
            "orchestrator",
            "research_completed",
            format!("Research completed after {} iteration(s)", state.iteration),
            serde_json::to_value(&stats).unwrap_or_default(),
        )
        .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::planner::PlannedQuestion;

    fn plan(deps: Vec<Vec<usize>>) -> ResearchPlan {
        ResearchPlan {
            research_depth: "standard".into(),
            questions: deps
                .into_iter()
                .enumerate()
                .map(|(i, d)| PlannedQuestion {
                    question: format!(" question {i} "),
                    objective: "obj".into(),
                    priority: Priority::Medium,
                    required_evidence: vec!["data".into()],
                    depends_on: d,
                })
                .collect(),
        }
    }

    #[test]
    fn plan_becomes_questions_and_linked_tasks() {
        let run = Uuid::new_v4();
        let (questions, tasks) = build_plan_records(run, &plan(vec![vec![], vec![0], vec![0, 1]]), 6);
        assert_eq!(questions.len(), 3);
        assert_eq!(tasks.len(), 3);
        assert_eq!(questions[0].question, "question 0", "text is trimmed");
        assert_eq!(tasks[1].dependencies, vec![tasks[0].id]);
        assert_eq!(tasks[2].dependencies, vec![tasks[0].id, tasks[1].id]);
        assert!(tasks.iter().zip(&questions).all(|(t, q)| t.question_id == Some(q.id) && t.research_run_id == run));
        assert!(tasks.iter().all(|t| t.status == TaskStatus::Pending && t.iteration == 1));
    }

    #[test]
    fn task_count_is_capped_and_dangling_dependencies_dropped() {
        let (questions, tasks) = build_plan_records(Uuid::new_v4(), &plan(vec![vec![], vec![], vec![], vec![2]]), 3);
        assert_eq!(questions.len(), 3);
        assert_eq!(tasks.len(), 3);
        let (_, capped) = build_plan_records(Uuid::new_v4(), &plan(vec![vec![], vec![0]]), 1);
        assert_eq!(capped.len(), 1);
        assert!(capped[0].dependencies.is_empty());
    }

    #[test]
    fn follow_up_tasks_target_the_next_iteration() {
        let q = Uuid::new_v4();
        let t = follow_up_task(Uuid::new_v4(), &FollowUp { question: "gap?".into(), question_id: Some(q) }, 2);
        assert_eq!((t.iteration, t.question_id, t.priority), (2, Some(q), Priority::High));
        assert!(t.dependencies.is_empty());
    }
}
