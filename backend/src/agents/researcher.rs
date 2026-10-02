use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use futures::future::join_all;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::agent::{Agent, EventSink};
use crate::{
    config::Limits,
    error::AgentError,
    llm::{LlmProvider, LlmRequest, generate_structured},
    models::{SearchResult, Source, SourceNature, SourceType},
    tools::{Fetcher, SearchProvider},
    util::text::{content_hash, excerpt_in_content, host_of, normalize_url, truncate_chars},
};

const NAME: &str = "researcher";

const QUERY_SYSTEM: &str = r#"You are a Researcher in an autonomous research organization.
Given one research task, write web search queries that would surface high-quality evidence:
primary sources, academic or government publications, official documentation, credible research
organizations and reputable reporting. Vary the angle between queries (e.g. one for empirical studies,
one for criticism or limitations). Queries must be short, concrete keyword strings -- no questions
longer than 12 words, no quotes, no operators.

Reply with ONLY: {"queries": ["...", "..."]}"#;

const EXTRACT_SYSTEM: &str = r#"You are a Researcher extracting evidence from ONE web page for a specific research task.

Rules:
- Extract only claims the page itself states or reports, that are relevant to the task. Never add outside knowledge.
- Each claim is one self-contained, specific statement (include numbers, populations, dates and scope where
  the page gives them). Do not generalize beyond what the page says.
- "excerpt" MUST be a short passage (under 300 characters) copied VERBATIM from the page that backs the claim.
  Claims whose excerpt cannot be found in the page will be discarded.
- "confidence" (0-1) is how clearly the excerpt establishes the claim.
- Judge the page: "source_type" is one of academic_paper, government_document, official_document,
  company_document, news, research_report, technical_blog, other. "nature" is one of primary (original data,
  study, filing or document), secondary (reports on others' work), commentary, opinion, marketing.
- If the page has nothing relevant, return an empty claims list.

Reply with ONLY:
{"title": "...", "publisher": "...", "published_at": "YYYY-MM-DD or empty",
 "source_type": "...", "nature": "...",
 "claims": [{"text": "...", "excerpt": "...", "confidence": 0.8}]}"#;

#[derive(Debug, Clone)]
pub struct ResearchInput {
    pub research_id: Uuid,
    pub task_id: Uuid,
    pub question: String,
    pub objective: String,
    pub required_evidence: Vec<String>,
}

/// A claim extracted by the researcher, not yet persisted.
#[derive(Debug, Clone)]
pub struct NewClaim {
    pub id: Uuid,
    /// Id of the entry in [`ResearchFindings::sources`] this claim came from.
    pub source_id: Uuid,
    pub text: String,
    pub excerpt: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Default)]
pub struct ResearchFindings {
    pub queries: Vec<String>,
    pub sources: Vec<Source>,
    pub claims: Vec<NewClaim>,
    /// Extracted claims discarded because the excerpt was not in the source.
    pub rejected_claims: usize,
}

#[derive(Deserialize)]
struct QueryList {
    queries: Vec<String>,
}

#[derive(Deserialize)]
struct Extraction {
    #[serde(default)]
    title: String,
    #[serde(default)]
    publisher: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    source_type: String,
    #[serde(default)]
    nature: String,
    #[serde(default)]
    claims: Vec<ExtractedClaim>,
}

#[derive(Deserialize)]
struct ExtractedClaim {
    text: String,
    excerpt: String,
    #[serde(default = "default_confidence")]
    confidence: f32,
}

fn default_confidence() -> f32 {
    0.5
}

pub struct ResearcherAgent {
    llm: Arc<dyn LlmProvider>,
    search: Arc<dyn SearchProvider>,
    fetcher: Arc<dyn Fetcher>,
    events: Arc<dyn EventSink>,
    limits: Limits,
}

struct Candidate {
    result: SearchResult,
    normalized: String,
}

impl ResearcherAgent {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        search: Arc<dyn SearchProvider>,
        fetcher: Arc<dyn Fetcher>,
        events: Arc<dyn EventSink>,
        limits: Limits,
    ) -> Self {
        Self { llm, search, fetcher, events, limits }
    }

    async fn plan_queries(&self, input: &ResearchInput) -> Result<Vec<String>, AgentError> {
        let n = self.limits.queries_per_task;
        let user = format!(
            "Task question: {}\nObjective: {}\nRequired evidence: {}\nWrite {} queries.",
            input.question,
            input.objective,
            input.required_evidence.join("; "),
            n
        );
        let list: QueryList = generate_structured(
            self.llm.as_ref(),
            NAME,
            LlmRequest::json(NAME, QUERY_SYSTEM, user),
            |l: &QueryList| {
                if l.queries.iter().any(|q| !q.trim().is_empty()) { Ok(()) } else { Err("no non-empty queries".into()) }
            },
        )
        .await?;
        let mut seen = HashSet::new();
        Ok(list
            .queries
            .into_iter()
            .map(|q| q.trim().to_string())
            .filter(|q| !q.is_empty() && seen.insert(q.to_lowercase()))
            .take(n)
            .collect())
    }

    /// Runs the queries and returns deduplicated candidates, best-looking
    /// source types first, capped at the per-task source budget.
    async fn find_candidates(&self, input: &ResearchInput, queries: &[String]) -> Vec<Candidate> {
        let searches = queries.iter().map(|q| async move {
            self.events
                .emit(
                    input.research_id,
                    NAME,
                    "search_started",
                    format!("Searching: {q}"),
                    json!({"task_id": input.task_id, "query": q}),
                )
                .await;
            (q, self.search.search(q).await)
        });
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for (query, outcome) in join_all(searches).await {
            match outcome {
                Ok(results) => {
                    for r in results {
                        if let Some(normalized) = normalize_url(&r.url)
                            && seen.insert(normalized.clone())
                        {
                            candidates.push(Candidate { result: r, normalized });
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(query = %query, error = %e, "search failed");
                    self.events
                        .emit(
                            input.research_id,
                            NAME,
                            "search_failed",
                            format!("Search failed for '{query}': {e}"),
                            json!({"task_id": input.task_id}),
                        )
                        .await;
                }
            }
        }
        // Stable sort keeps provider ranking within a quality tier.
        candidates.sort_by_key(|c| quality_tier(&c.result));
        candidates.truncate(self.limits.sources_per_task);
        candidates
    }

    async fn load_content(&self, c: &Candidate) -> Option<(String, Option<String>)> {
        if let Some(text) = &c.result.content {
            return Some((text.clone(), None));
        }
        match self.fetcher.fetch(&c.result.url).await {
            Ok(page) => Some((page.text, page.title)),
            Err(e) => {
                tracing::warn!(url = %c.result.url, error = %e, "fetch failed");
                None
            }
        }
    }

    async fn mine_source(&self, input: &ResearchInput, candidate: Candidate) -> Option<(Source, Vec<NewClaim>, usize)> {
        let (content, page_title) = self.load_content(&candidate).await?;
        let r = &candidate.result;
        let host = host_of(&r.url).unwrap_or_default();

        let user = format!(
            "Task question: {}\nObjective: {}\nPage URL: {}\nPage title: {}\n\n--- PAGE TEXT ---\n{}",
            input.question,
            input.objective,
            r.url,
            r.title,
            truncate_chars(&content, self.limits.source_char_limit)
        );
        let max_claims = self.limits.claims_per_source;
        let extraction: Extraction = match generate_structured(
            self.llm.as_ref(),
            NAME,
            LlmRequest::json(NAME, EXTRACT_SYSTEM, user),
            |e: &Extraction| {
                if e.claims.len() > max_claims * 2 {
                    Err(format!("return at most {max_claims} claims"))
                } else {
                    Ok(())
                }
            },
        )
        .await
        {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(url = %r.url, error = %err, "claim extraction failed");
                return None;
            }
        };

        let source_id = Uuid::new_v4();
        let source = Source {
            id: source_id,
            research_run_id: input.research_id,
            url: r.url.clone(),
            normalized_url: candidate.normalized.clone(),
            title: first_non_empty([extraction.title.as_str(), page_title.as_deref().unwrap_or(""), r.title.as_str()])
                .unwrap_or_else(|| r.url.clone()),
            publisher: first_non_empty([extraction.publisher.as_str(), host.as_str()]),
            published_at: first_non_empty([r.published_at.as_deref().unwrap_or(""), extraction.published_at.as_str()]),
            retrieved_at: Utc::now(),
            content_hash: content_hash(&content),
            content,
            source_type: extraction.source_type.parse().unwrap_or_else(|_| SourceType::guess_from_host(&host)),
            nature: extraction.nature.parse().unwrap_or(SourceNature::Unknown),
        };

        let mut claims = Vec::new();
        let mut rejected = 0;
        let mut seen = HashSet::new();
        for c in extraction.claims.into_iter().take(max_claims) {
            let text = c.text.trim().to_string();
            // Provenance guard: a claim must be backed by text actually on the page.
            if text.is_empty() || !excerpt_in_content(&c.excerpt, &source.content) || !seen.insert(text.to_lowercase())
            {
                rejected += 1;
                continue;
            }
            claims.push(NewClaim {
                id: Uuid::new_v4(),
                source_id,
                text,
                excerpt: c.excerpt.trim().to_string(),
                confidence: c.confidence.clamp(0.0, 1.0),
            });
        }
        Some((source, claims, rejected))
    }
}

fn first_non_empty<'a>(options: impl IntoIterator<Item = &'a str>) -> Option<String> {
    options.into_iter().map(str::trim).find(|s| !s.is_empty()).map(str::to_string)
}

/// Lower is better: primary-looking institutional sources first.
fn quality_tier(r: &SearchResult) -> u8 {
    match SourceType::guess_from_host(&r.source) {
        SourceType::GovernmentDocument | SourceType::AcademicPaper => 0,
        SourceType::OfficialDocument | SourceType::ResearchReport => 1,
        SourceType::News => 2,
        _ => 3,
    }
}

#[async_trait]
impl Agent for ResearcherAgent {
    type Input = ResearchInput;
    type Output = ResearchFindings;

    fn name(&self) -> &'static str {
        NAME
    }

    async fn execute(&self, input: ResearchInput) -> Result<ResearchFindings, AgentError> {
        if input.question.trim().is_empty() {
            return Err(AgentError::InvalidInput { agent: NAME, reason: "empty task question".into() });
        }
        let queries = self.plan_queries(&input).await?;
        let candidates = self.find_candidates(&input, &queries).await;
        if candidates.is_empty() {
            return Err(AgentError::NoSources { agent: NAME });
        }

        let mined = join_all(candidates.into_iter().map(|c| self.mine_source(&input, c))).await;
        let mut findings = ResearchFindings { queries, ..Default::default() };
        for (source, claims, rejected) in mined.into_iter().flatten() {
            findings.rejected_claims += rejected;
            findings.sources.push(source);
            findings.claims.extend(claims);
        }
        if findings.sources.is_empty() {
            return Err(AgentError::NoSources { agent: NAME });
        }
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agents::agent::NoopSink,
        llm::structured::testing::ScriptedLlm,
        tools::{
            fetch::{FetchedPage, testing::MockFetcher},
            search::testing::MockSearch,
        },
    };

    const PAGE: &str = "In a controlled experiment, developers using an AI assistant completed the task 55% faster than the control group. The study included 95 professional developers recruited online and measured completion time only.";

    fn hit(url: &str) -> SearchResult {
        SearchResult {
            title: "T".into(),
            url: url.into(),
            snippet: String::new(),
            source: host_of(url).unwrap(),
            published_at: None,
            content: None,
        }
    }

    fn agent(llm: Arc<ScriptedLlm>, search: MockSearch, fetcher: MockFetcher) -> ResearcherAgent {
        ResearcherAgent::new(llm, Arc::new(search), Arc::new(fetcher), Arc::new(NoopSink), Limits::default())
    }

    fn input() -> ResearchInput {
        ResearchInput {
            research_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            question: "What is the productivity effect of AI coding assistants?".into(),
            objective: "find measurements".into(),
            required_evidence: vec!["sample size".into()],
        }
    }

    fn extraction(claims: &str) -> String {
        format!(
            r#"{{"title":"Study","publisher":"Univ","published_at":"2024-05-01","source_type":"academic_paper","nature":"primary","claims":{claims}}}"#
        )
    }

    #[tokio::test]
    async fn extracts_claims_and_drops_ones_without_a_verbatim_excerpt() {
        let good = r#"{"text":"AI assistant users finished the task 55% faster.","excerpt":"completed the task 55% faster than the control group","confidence":0.9}"#;
        let fabricated =
            r#"{"text":"Quality improved 30%.","excerpt":"code quality improved by 30 percent","confidence":0.9}"#;
        let llm = Arc::new(ScriptedLlm::ok(&[
            r#"{"queries":["ai coding assistant productivity study"]}"#,
            &extraction(&format!("[{good},{fabricated}]")),
        ]));
        let search = MockSearch::with_default(vec![hit("https://arxiv.org/abs/1")]);
        let fetcher = MockFetcher::default();
        fetcher
            .pages
            .lock()
            .unwrap()
            .insert("https://arxiv.org/abs/1".into(), FetchedPage { title: None, text: PAGE.into() });

        let out = agent(llm, search, fetcher).execute(input()).await.unwrap();
        assert_eq!(out.sources.len(), 1);
        assert_eq!(out.sources[0].source_type, SourceType::AcademicPaper);
        assert_eq!(out.sources[0].nature, SourceNature::Primary);
        assert_eq!(out.claims.len(), 1);
        assert_eq!(out.rejected_claims, 1);
        assert_eq!(out.claims[0].source_id, out.sources[0].id);
    }

    #[tokio::test]
    async fn deduplicates_urls_across_queries_and_prefers_institutional_sources() {
        let llm = Arc::new(ScriptedLlm::ok(&[r#"{"queries":["a b","c d"]}"#, &extraction("[]"), &extraction("[]")]));
        let search = MockSearch::with_default(vec![
            hit("https://blog.example.com/post"),
            hit("https://www.nist.gov/doc/?utm_source=x"),
            hit("https://nist.gov/doc"),
        ]);
        let fetcher = MockFetcher::default();
        for u in ["https://blog.example.com/post", "https://www.nist.gov/doc/?utm_source=x"] {
            fetcher.pages.lock().unwrap().insert(u.into(), FetchedPage { title: None, text: PAGE.into() });
        }
        let fetcher = Arc::new(fetcher);
        let a = ResearcherAgent::new(llm, Arc::new(search), fetcher.clone(), Arc::new(NoopSink), Limits::default());
        let out = a.execute(input()).await.unwrap();
        // 2 unique pages despite 2 queries x 3 results
        assert_eq!(out.sources.len(), 2);
        assert_eq!(fetcher.calls.lock().unwrap().len(), 2);
        assert!(out.sources.iter().any(|s| s.url.contains("nist.gov")));
    }

    #[tokio::test]
    async fn fails_cleanly_when_nothing_can_be_fetched() {
        let llm = Arc::new(ScriptedLlm::ok(&[r#"{"queries":["q"]}"#]));
        let search = MockSearch::with_default(vec![hit("https://example.com/x")]);
        let err = agent(llm, search, MockFetcher::default()).execute(input()).await.unwrap_err();
        assert!(matches!(err, AgentError::NoSources { .. }));
    }

    #[tokio::test]
    async fn uses_provider_supplied_content_without_fetching() {
        let llm = Arc::new(ScriptedLlm::ok(&[r#"{"queries":["q"]}"#, &extraction("[]")]));
        let mut h = hit("https://example.com/x");
        h.content = Some(PAGE.into());
        let fetcher = Arc::new(MockFetcher::default());
        let a = ResearcherAgent::new(
            llm,
            Arc::new(MockSearch::with_default(vec![h])),
            fetcher.clone(),
            Arc::new(NoopSink),
            Limits::default(),
        );
        let out = a.execute(input()).await.unwrap();
        assert_eq!(out.sources.len(), 1);
        assert!(fetcher.calls.lock().unwrap().is_empty());
    }
}
