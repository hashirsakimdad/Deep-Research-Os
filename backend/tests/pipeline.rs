//! End-to-end workflow tests against a real PostgreSQL with scripted LLM,
//! search and fetch providers. They exercise the complete lifecycle (plan,
//! concurrent research, verification, contradictions, synthesis, critique,
//! iteration) without any paid API.
//!
//! Requires `DATABASE_URL`; tests are skipped (with a notice) when it is unset.

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use deep_research_os_backend::{
    config::Limits,
    database::{self, Repository},
    error::{FetchError, LlmError, SearchError},
    llm::{LlmProvider, LlmRequest, LlmResponse},
    models::*,
    orchestrator::{EventBus, Orchestrator},
    tools::{FetchedPage, Fetcher, SearchProvider},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// ---- scripted providers ----------------------------------------------------

fn uuids_in(text: &str) -> Vec<Uuid> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 36 <= b.len() {
        if b[i + 8] == b'-'
            && b[i + 13] == b'-'
            && b[i + 18] == b'-'
            && b[i + 23] == b'-'
            && let Some(Ok(u)) = text.get(i..i + 36).map(Uuid::parse_str)
        {
            if !out.contains(&u) {
                out.push(u);
            }
            i += 36;
            continue;
        }
        i += 1;
    }
    out
}

#[derive(Default)]
struct Behavior {
    /// Critic asks for more research on its first N calls.
    critic_wants_more_times: usize,
    /// Queries containing this text make search fail.
    failing_query_marker: Option<&'static str>,
}

struct ScriptedWorld {
    behavior: Behavior,
    critic_calls: AtomicUsize,
    llm_calls: AtomicUsize,
    synth_feedback: Mutex<Vec<String>>,
}

impl ScriptedWorld {
    fn new(behavior: Behavior) -> Arc<Self> {
        Arc::new(Self {
            behavior,
            critic_calls: AtomicUsize::new(0),
            llm_calls: AtomicUsize::new(0),
            synth_feedback: Mutex::new(vec![]),
        })
    }
}

struct MockLlm(Arc<ScriptedWorld>);

const PAGE_TEXT: &str = "A randomized controlled trial of professional developers found that those using an AI coding assistant completed the benchmark task 55 percent faster than the control group. The authors note the result applies to a single well-specified task and did not measure code quality over time.";

#[async_trait]
impl LlmProvider for MockLlm {
    async fn generate(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.0.llm_calls.fetch_add(1, Ordering::SeqCst);
        let user = req.messages.last().map(|m| m.content.clone()).unwrap_or_default();
        let reply = match req.label {
            "planner" => r#"{"research_depth":"standard","questions":[
                {"question":"What productivity effects have been measured for AI coding agents?","objective":"empirical measurements","priority":"high","required_evidence":["sample size"],"depends_on":[]},
                {"question":"What security risks do AI coding agents introduce?","objective":"risk evidence","priority":"medium","required_evidence":["incident reports"],"depends_on":[]}]}"#
                .to_string(),
            "researcher" if req.system.contains("write web search queries") => {
                let topic = if user.contains("security") { "security" } else { "productivity" };
                format!(r#"{{"queries":["ai coding agents {topic} evidence"]}}"#)
            }
            "researcher" => {
                let url_line = user.lines().find(|l| l.starts_with("Page URL:")).unwrap_or_default();
                let slug = url_line.rsplit('/').next().unwrap_or("x");
                format!(
                    r#"{{"title":"Study {slug}","publisher":"Example Univ","published_at":"2025-03-01","source_type":"academic_paper","nature":"primary",
                        "claims":[{{"text":"Developers with an AI assistant finished the benchmark task 55% faster ({slug}).","excerpt":"completed the benchmark task 55 percent faster than the control group","confidence":0.9}},
                                  {{"text":"AI coding agents eliminate all bugs.","excerpt":"this sentence is not on the page","confidence":0.9}}]}}"#
                )
            }
            "verifier" => {
                let ids = uuids_in(&user);
                let v: Vec<String> = ids
                    .iter()
                    .map(|id| format!(r#"{{"claim_id":"{id}","support_level":"SUPPORTED","confidence":0.85,"reasoning":"The excerpt directly states the result for the studied task."}}"#))
                    .collect();
                format!(r#"{{"verdicts":[{}]}}"#, v.join(","))
            }
            "contradiction_detector" => r#"{"pairs":[]}"#.to_string(),
            "synthesizer" => {
                if let Some(idx) = user.find("REVIEWER FEEDBACK:") {
                    self.0.synth_feedback.lock().unwrap().push(user[idx..].to_string());
                }
                // claims block precedes "Contradictions:"
                let claims_part = user.split("Contradictions:").next().unwrap_or_default();
                let ids = uuids_in(claims_part);
                let first = ids.first().map(|i| i.to_string()).unwrap_or_default();
                let all: Vec<String> = ids.iter().map(|i| format!("\"{i}\"")).collect();
                format!(
                    r#"{{"executive_summary":"Evidence shows faster task completion in a controlled trial.","methodology":"Questions were planned, researched in parallel, verified and cross-checked.",
                        "key_findings":[{{"text":"Assistants sped up a benchmark task.","claim_ids":["{first}"]}}],
                        "detailed_findings":[{{"heading":"Productivity","body":[{{"text":"Controlled evidence exists for one task.","claim_ids":[{}]}}]}}],
                        "contradictory_evidence":[],"limitations":["Single-task evidence."],"open_questions":["Long-term quality effects."],
                        "conclusion":"Gains are demonstrated for a narrow task."}}"#,
                    all.join(",")
                )
            }
            "critic" => {
                let n = self.0.critic_calls.fetch_add(1, Ordering::SeqCst);
                if n < self.0.behavior.critic_wants_more_times {
                    r#"{"quality":0.6,"needs_more_research":true,
                        "issues":[{"type":"INSUFFICIENT_EVIDENCE","claim_id":null,"description":"Only one study cited.","severity":"medium"}],
                        "required_research":["Find independent replications of the productivity result"]}"#
                        .to_string()
                } else {
                    r#"{"quality":0.9,"needs_more_research":false,"issues":[],"required_research":[]}"#.to_string()
                }
            }
            other => return Err(LlmError::InvalidResponse(format!("unexpected agent label {other}"))),
        };
        Ok(LlmResponse { content: reply, model: "mock".into(), usage: None })
    }
}

struct MockSearch(Arc<ScriptedWorld>);

#[async_trait]
impl SearchProvider for MockSearch {
    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, SearchError> {
        if self.0.behavior.failing_query_marker.is_some_and(|m| query.contains(m)) {
            return Err(SearchError::Provider("scripted failure".into()));
        }
        let slug: String = query.chars().filter(|c| c.is_ascii_alphanumeric()).take(24).collect();
        Ok((1..=2)
            .map(|n| SearchResult {
                title: format!("Result {n}"),
                url: format!("https://university.edu/{slug}/{n}"),
                snippet: String::new(),
                source: "university.edu".into(),
                published_at: None,
                content: None,
            })
            .collect())
    }
}

struct MockFetcher;

#[async_trait]
impl Fetcher for MockFetcher {
    async fn fetch(&self, _url: &str) -> Result<FetchedPage, FetchError> {
        Ok(FetchedPage { title: Some("Page".into()), text: PAGE_TEXT.into() })
    }
}

// ---- harness ---------------------------------------------------------------

async fn harness(world: Arc<ScriptedWorld>) -> Option<(Repository, Orchestrator)> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping database integration test");
        return None;
    };
    let pool = database::connect(&url).await.expect("database connection");
    let repo = Repository::new(pool);
    let events = Arc::new(EventBus::new(repo.clone()));
    let limits = Limits { sources_per_task: 2, ..Limits::default() };
    let orch = Orchestrator::new(
        repo.clone(),
        events,
        Arc::new(MockLlm(world.clone())),
        Arc::new(MockSearch(world)),
        Arc::new(MockFetcher),
        limits,
        CancellationToken::new(),
    );
    Some((repo, orch))
}

async fn event_types(repo: &Repository, id: Uuid) -> Vec<String> {
    repo.events_after(id, 0, 5000).await.unwrap().into_iter().map(|e| e.event_type).collect()
}

// ---- tests -----------------------------------------------------------------

#[tokio::test]
async fn full_lifecycle_produces_a_traceable_cited_report() {
    let world = ScriptedWorld::new(Behavior::default());
    let Some((repo, orch)) = harness(world.clone()).await else { return };
    let run = repo.create_run("What are the productivity and security effects of AI coding agents?").await.unwrap();
    orch.run(run.clone()).await;

    let done = repo.get_run(run.id).await.unwrap().unwrap();
    assert_eq!(done.status, ResearchStatus::Completed, "error: {:?}", done.error);
    assert!(done.completed_at.is_some());

    // plan -> tasks
    assert_eq!(repo.list_questions(run.id).await.unwrap().len(), 2);
    let tasks = repo.list_tasks(run.id).await.unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|t| t.status == TaskStatus::Completed));

    // sources: deduplicated by URL; claims: fabricated excerpts were dropped
    let sources = repo.list_source_refs(run.id).await.unwrap();
    assert_eq!(sources.len(), 4, "two tasks x two distinct URLs");
    let claims = repo.claim_records(run.id, None, None).await.unwrap();
    assert_eq!(claims.len(), 4, "one grounded claim per source; ungrounded claims discarded");
    assert!(claims.iter().all(|c| c.claim.status == SupportLevel::Supported));
    assert!(claims.iter().all(|c| !c.excerpt().is_empty()), "every claim has evidence");

    // report: final, cited, and every citation is a real stored source URL
    let report = repo.latest_report(run.id).await.unwrap().expect("report");
    assert!(report.is_final);
    assert!(!report.citations.is_empty());
    let known: HashSet<_> = sources.iter().map(|s| s.url.clone()).collect();
    assert!(report.citations.iter().all(|c| known.contains(&c.url)));
    assert!(report.markdown.contains("[1]"));
    assert!(report.markdown.contains("## Sources"));

    // critique persisted; one iteration was enough
    assert_eq!(repo.list_critiques(run.id).await.unwrap().len(), 1);
    assert!(repo.list_tasks(run.id).await.unwrap().iter().all(|t| t.iteration == 1));

    // events describe real operations in order
    let types = event_types(&repo, run.id).await;
    for expected in [
        "research_started",
        "planning_started",
        "planning_completed",
        "task_created",
        "search_started",
        "source_found",
        "claim_extracted",
        "verification_started",
        "verification_completed",
        "synthesis_started",
        "synthesis_completed",
        "critique_started",
        "critique_completed",
        "research_completed",
    ] {
        assert!(types.iter().any(|t| t == expected), "missing event {expected}; got {types:?}");
    }
    let pos = |name: &str| types.iter().position(|t| t == name).unwrap();
    assert!(pos("planning_completed") < pos("source_found"));
    assert!(pos("verification_completed") < pos("synthesis_started"));
    assert_eq!(*types.last().unwrap(), "research_completed");

    let stats = repo.run_stats(run.id).await.unwrap();
    assert_eq!((stats.sources, stats.claims, stats.supported), (4, 4, 4));
}

#[tokio::test]
async fn critic_feedback_triggers_one_more_iteration_and_stops_at_the_limit() {
    // The critic keeps asking for more research, but MAX_ITERATIONS = 2 must hold.
    let world = ScriptedWorld::new(Behavior { critic_wants_more_times: 99, ..Behavior::default() });
    let Some((repo, orch)) = harness(world.clone()).await else { return };
    let run = repo.create_run("What are the productivity and security effects of AI coding agents?").await.unwrap();
    orch.run(run.clone()).await;

    let done = repo.get_run(run.id).await.unwrap().unwrap();
    assert_eq!(done.status, ResearchStatus::Completed, "error: {:?}", done.error);
    assert_eq!(done.iteration, 2);

    let tasks = repo.list_tasks(run.id).await.unwrap();
    assert!(tasks.iter().any(|t| t.iteration == 2), "follow-up task created");
    assert!(tasks.iter().all(|t| t.iteration <= 2));
    assert_eq!(world.critic_calls.load(Ordering::SeqCst), 2, "critiqued exactly once per iteration");
    assert!(
        world.synth_feedback.lock().unwrap().iter().any(|f| f.contains("Only one study cited")),
        "critique fed into the next draft"
    );

    let types = event_types(&repo, run.id).await;
    assert!(types.contains(&"additional_research_required".to_string()));
    assert!(types.contains(&"iteration_started".to_string()));

    // unresolved concern at the iteration limit is stated in the final report
    let report = repo.latest_report(run.id).await.unwrap().unwrap();
    assert!(report.is_final);
    assert!(report.content.limitations.iter().any(|l| l.contains("Reviewer-noted weakness")));
    assert_eq!(repo.list_critiques(run.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_failed_task_does_not_destroy_the_run() {
    let world = ScriptedWorld::new(Behavior { failing_query_marker: Some("security"), ..Behavior::default() });
    let Some((repo, orch)) = harness(world).await else { return };
    let run = repo.create_run("What are the productivity and security effects of AI coding agents?").await.unwrap();
    orch.run(run.clone()).await;

    let done = repo.get_run(run.id).await.unwrap().unwrap();
    assert_eq!(done.status, ResearchStatus::Completed, "error: {:?}", done.error);
    let tasks = repo.list_tasks(run.id).await.unwrap();
    let failed: Vec<_> = tasks.iter().filter(|t| t.status == TaskStatus::Failed).collect();
    // the failed question is retried once as a follow-up, and fails again
    assert_eq!(failed.len(), 2);
    assert_eq!(failed.iter().map(|t| t.iteration).collect::<std::collections::BTreeSet<_>>().len(), 2);
    assert!(failed.iter().all(|t| t.error.is_some()));
    assert!(tasks.iter().any(|t| t.status == TaskStatus::Completed));
    let types = event_types(&repo, run.id).await;
    assert!(types.contains(&"task_failed".to_string()));
    assert!(types.contains(&"follow_up_research_failed".to_string()));

    // the uncovered question is surfaced honestly in the final report
    let report = repo.latest_report(run.id).await.unwrap().unwrap();
    assert!(report.content.limitations.iter().any(|l| l.contains("No verified evidence was found for")));
}

#[tokio::test]
async fn run_fails_cleanly_when_every_task_fails() {
    let world = ScriptedWorld::new(Behavior { failing_query_marker: Some("evidence"), ..Behavior::default() });
    let Some((repo, orch)) = harness(world).await else { return };
    let run = repo.create_run("What are the productivity and security effects of AI coding agents?").await.unwrap();
    orch.run(run.clone()).await;

    let done = repo.get_run(run.id).await.unwrap().unwrap();
    assert_eq!(done.status, ResearchStatus::Failed);
    assert!(done.error.unwrap().contains("failed"));
    assert!(event_types(&repo, run.id).await.contains(&"research_failed".to_string()));
    assert!(repo.latest_report(run.id).await.unwrap().is_none());
}

#[tokio::test]
async fn repository_deduplicates_sources_claims_and_contradictions() {
    let world = ScriptedWorld::new(Behavior::default());
    let Some((repo, _)) = harness(world).await else { return };
    let run = repo.create_run("Repository dedup behavior check query").await.unwrap();

    let mk_source = |url: &str| Source {
        id: Uuid::new_v4(),
        research_run_id: run.id,
        url: url.into(),
        normalized_url: deep_research_os_backend::util::text::normalize_url(url).unwrap(),
        title: "T".into(),
        publisher: None,
        published_at: None,
        retrieved_at: chrono::Utc::now(),
        content: "content".into(),
        content_hash: "h".into(),
        source_type: SourceType::Other,
        nature: SourceNature::Unknown,
    };
    let first = repo.upsert_source(&mk_source("https://www.example.com/a/?utm_source=x")).await.unwrap();
    let again = repo.upsert_source(&mk_source("https://example.com/a")).await.unwrap();
    assert_eq!(first, again, "same normalized URL -> same source row");
    assert_eq!(repo.list_source_refs(run.id).await.unwrap().len(), 1);

    // tasks are needed for claims
    let q = ResearchQuestion {
        id: Uuid::new_v4(),
        research_run_id: run.id,
        question: "q".into(),
        objective: "o".into(),
        priority: Priority::Low,
        position: 0,
    };
    repo.insert_questions(std::slice::from_ref(&q)).await.unwrap();
    let task = ResearchTask {
        id: Uuid::new_v4(),
        research_run_id: run.id,
        question_id: Some(q.id),
        question: "q".into(),
        objective: "o".into(),
        priority: Priority::Low,
        status: TaskStatus::Pending,
        assigned_agent: "researcher".into(),
        required_evidence: vec![],
        dependencies: vec![],
        iteration: 1,
        error: None,
        created_at: chrono::Utc::now(),
        completed_at: None,
    };
    repo.insert_task(&task).await.unwrap();

    let mk_claim = |text: &str| Claim {
        id: Uuid::new_v4(),
        research_run_id: run.id,
        research_task_id: task.id,
        text: text.into(),
        source_id: first,
        confidence: 0.8,
        status: SupportLevel::Unverified,
        iteration: 1,
        created_at: chrono::Utc::now(),
    };
    let c1 = mk_claim("Agents are faster.");
    assert!(repo.insert_claim_with_evidence(&c1, "agents are faster").await.unwrap().is_some());
    assert!(
        repo.insert_claim_with_evidence(&mk_claim("agents  ARE faster."), "agents are faster").await.unwrap().is_none(),
        "duplicate claim text from the same source"
    );
    let c2 = mk_claim("Agents are slower at debugging.");
    repo.insert_claim_with_evidence(&c2, "slower debugging").await.unwrap();

    repo.set_claim_verification(c1.id, SupportLevel::PartiallySupported, 0.6, "narrow scope").await.unwrap();
    let records = repo.claim_records(run.id, Some(SupportLevel::PartiallySupported), None).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].support_note(), "narrow scope");
    assert_eq!(records[0].source.url, "https://www.example.com/a/?utm_source=x");

    let contradiction = |a, b| Contradiction {
        id: Uuid::new_v4(),
        research_run_id: run.id,
        claim_a_id: a,
        claim_b_id: b,
        kind: ContradictionKind::MetricDifference,
        explanation: "e".into(),
        created_at: chrono::Utc::now(),
    };
    assert!(repo.insert_contradiction(&contradiction(c1.id, c2.id)).await.unwrap());
    assert!(
        !repo.insert_contradiction(&contradiction(c2.id, c1.id)).await.unwrap(),
        "pair order does not create duplicates"
    );

    let stats = repo.run_stats(run.id).await.unwrap();
    assert_eq!(
        (stats.sources, stats.claims, stats.partially_supported, stats.unverified, stats.contradictions),
        (1, 2, 1, 1, 1)
    );
}
