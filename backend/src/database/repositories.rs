use std::str::FromStr;

use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow};
use uuid::Uuid;

use super::models::{ClaimRecord, RunStats};
use crate::models::*;

/// All SQL lives here. Queries are runtime-checked so the crate builds
/// without a live database.
#[derive(Clone)]
pub struct Repository {
    pool: PgPool,
}

fn decode_err(msg: String) -> sqlx::Error {
    sqlx::Error::Decode(msg.into())
}

fn get_enum<T: FromStr<Err = String>>(row: &PgRow, col: &str) -> Result<T, sqlx::Error> {
    row.try_get::<String, _>(col)?.parse().map_err(decode_err)
}

fn json_vec<T: serde::de::DeserializeOwned>(row: &PgRow, col: &str) -> Result<Vec<T>, sqlx::Error> {
    let v: Value = row.try_get(col)?;
    serde_json::from_value(v).map_err(|e| decode_err(e.to_string()))
}

fn to_json<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Whitespace/case-normalized claim text used for per-source claim dedup.
pub fn claim_dedupe_key(text: &str) -> String {
    crate::util::text::normalize_ws(text)
}

fn run_from_row(row: &PgRow) -> Result<ResearchRun, sqlx::Error> {
    Ok(ResearchRun {
        id: row.try_get("id")?,
        query: row.try_get("query")?,
        status: get_enum(row, "status")?,
        iteration: row.try_get("iteration")?,
        error: row.try_get("error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}

fn question_from_row(row: &PgRow) -> Result<ResearchQuestion, sqlx::Error> {
    Ok(ResearchQuestion {
        id: row.try_get("id")?,
        research_run_id: row.try_get("research_run_id")?,
        question: row.try_get("question")?,
        objective: row.try_get("objective")?,
        priority: get_enum(row, "priority")?,
        position: row.try_get("position")?,
    })
}

fn task_from_row(row: &PgRow) -> Result<ResearchTask, sqlx::Error> {
    Ok(ResearchTask {
        id: row.try_get("id")?,
        research_run_id: row.try_get("research_run_id")?,
        question_id: row.try_get("question_id")?,
        question: row.try_get("question")?,
        objective: row.try_get("objective")?,
        priority: get_enum(row, "priority")?,
        status: get_enum(row, "status")?,
        assigned_agent: row.try_get("assigned_agent")?,
        required_evidence: json_vec(row, "required_evidence")?,
        dependencies: json_vec(row, "dependencies")?,
        iteration: row.try_get("iteration")?,
        error: row.try_get("error")?,
        created_at: row.try_get("created_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}

fn source_from_row(row: &PgRow) -> Result<Source, sqlx::Error> {
    Ok(Source {
        id: row.try_get("id")?,
        research_run_id: row.try_get("research_run_id")?,
        url: row.try_get("url")?,
        normalized_url: row.try_get("normalized_url")?,
        title: row.try_get("title")?,
        publisher: row.try_get("publisher")?,
        published_at: row.try_get("published_at")?,
        retrieved_at: row.try_get("retrieved_at")?,
        content: row.try_get("content")?,
        content_hash: row.try_get("content_hash")?,
        source_type: get_enum(row, "source_type")?,
        nature: get_enum(row, "nature")?,
    })
}

fn source_ref_from_row(row: &PgRow) -> Result<SourceReference, sqlx::Error> {
    Ok(SourceReference {
        id: row.try_get("id")?,
        url: row.try_get("url")?,
        title: row.try_get("title")?,
        publisher: row.try_get("publisher")?,
        published_at: row.try_get("published_at")?,
        source_type: get_enum(row, "source_type")?,
        nature: get_enum(row, "nature")?,
    })
}

fn claim_from_row(row: &PgRow) -> Result<Claim, sqlx::Error> {
    Ok(Claim {
        id: row.try_get("id")?,
        research_run_id: row.try_get("research_run_id")?,
        research_task_id: row.try_get("research_task_id")?,
        text: row.try_get("text")?,
        source_id: row.try_get("source_id")?,
        confidence: row.try_get("confidence")?,
        status: get_enum(row, "status")?,
        iteration: row.try_get("iteration")?,
        created_at: row.try_get("created_at")?,
    })
}

fn evidence_from_row(row: &PgRow) -> Result<Evidence, sqlx::Error> {
    Ok(Evidence {
        id: row.try_get("id")?,
        claim_id: row.try_get("claim_id")?,
        source_id: row.try_get("source_id")?,
        excerpt: row.try_get("excerpt")?,
        relevance: row.try_get("relevance")?,
        support_level: get_enum(row, "support_level")?,
        reasoning: row.try_get("reasoning")?,
        created_at: row.try_get("created_at")?,
    })
}

fn report_from_row(row: &PgRow) -> Result<Report, sqlx::Error> {
    let content: Value = row.try_get("content")?;
    let citations: Value = row.try_get("citations")?;
    Ok(Report {
        id: row.try_get("id")?,
        research_run_id: row.try_get("research_run_id")?,
        iteration: row.try_get("iteration")?,
        is_final: row.try_get("is_final")?,
        content: serde_json::from_value(content).map_err(|e| decode_err(e.to_string()))?,
        markdown: row.try_get("markdown")?,
        citations: serde_json::from_value(citations).map_err(|e| decode_err(e.to_string()))?,
        created_at: row.try_get("created_at")?,
    })
}

fn critique_from_row(row: &PgRow) -> Result<Critique, sqlx::Error> {
    Ok(Critique {
        id: row.try_get("id")?,
        research_run_id: row.try_get("research_run_id")?,
        report_id: row.try_get("report_id")?,
        iteration: row.try_get("iteration")?,
        quality: row.try_get("quality")?,
        needs_more_research: row.try_get("needs_more_research")?,
        issues: json_vec(row, "issues")?,
        required_research: json_vec(row, "required_research")?,
        created_at: row.try_get("created_at")?,
    })
}

fn contradiction_from_row(row: &PgRow) -> Result<Contradiction, sqlx::Error> {
    Ok(Contradiction {
        id: row.try_get("id")?,
        research_run_id: row.try_get("research_run_id")?,
        claim_a_id: row.try_get("claim_a_id")?,
        claim_b_id: row.try_get("claim_b_id")?,
        kind: get_enum(row, "kind")?,
        explanation: row.try_get("explanation")?,
        created_at: row.try_get("created_at")?,
    })
}

fn event_from_row(row: &PgRow) -> Result<AgentEvent, sqlx::Error> {
    Ok(AgentEvent {
        id: row.try_get("id")?,
        research_id: row.try_get("research_id")?,
        agent: row.try_get("agent")?,
        event_type: row.try_get("event_type")?,
        message: row.try_get("message")?,
        metadata: row.try_get("metadata")?,
        timestamp: row.try_get("created_at")?,
    })
}

impl Repository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ----- runs -----------------------------------------------------------

    pub async fn create_run(&self, query: &str) -> Result<ResearchRun, sqlx::Error> {
        let row = sqlx::query("INSERT INTO research_runs (id, query, status) VALUES ($1, $2, $3) RETURNING *")
            .bind(Uuid::new_v4())
            .bind(query)
            .bind(ResearchStatus::Planning.as_str())
            .fetch_one(&self.pool)
            .await?;
        run_from_row(&row)
    }

    pub async fn get_run(&self, id: Uuid) -> Result<Option<ResearchRun>, sqlx::Error> {
        sqlx::query("SELECT * FROM research_runs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| run_from_row(&r))
            .transpose()
    }

    pub async fn list_runs(&self, limit: i64) -> Result<Vec<ResearchRun>, sqlx::Error> {
        sqlx::query("SELECT * FROM research_runs ORDER BY created_at DESC LIMIT $1")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(run_from_row)
            .collect()
    }

    pub async fn set_run_status(
        &self,
        id: Uuid,
        status: ResearchStatus,
        iteration: i32,
        error: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE research_runs
             SET status = $2, iteration = $3, error = $4, updated_at = now(),
                 completed_at = CASE WHEN $5 THEN now() ELSE completed_at END
             WHERE id = $1",
        )
        .bind(id)
        .bind(status.as_str())
        .bind(iteration)
        .bind(error)
        .bind(status.is_terminal())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Runs left in a non-terminal state by a previous process cannot resume;
    /// mark them failed so they are not shown as in progress forever.
    pub async fn fail_interrupted_runs(&self) -> Result<u64, sqlx::Error> {
        let r = sqlx::query(
            "UPDATE research_runs
             SET status = 'failed', error = 'interrupted by server restart',
                 updated_at = now(), completed_at = now()
             WHERE status NOT IN ('completed', 'failed')",
        )
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected())
    }

    // ----- questions & tasks ---------------------------------------------

    pub async fn insert_questions(&self, questions: &[ResearchQuestion]) -> Result<(), sqlx::Error> {
        for q in questions {
            sqlx::query(
                "INSERT INTO research_questions (id, research_run_id, question, objective, priority, position)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(q.id)
            .bind(q.research_run_id)
            .bind(&q.question)
            .bind(&q.objective)
            .bind(q.priority.as_str())
            .bind(q.position)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn list_questions(&self, run: Uuid) -> Result<Vec<ResearchQuestion>, sqlx::Error> {
        sqlx::query("SELECT * FROM research_questions WHERE research_run_id = $1 ORDER BY position")
            .bind(run)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(question_from_row)
            .collect()
    }

    pub async fn insert_task(&self, t: &ResearchTask) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO research_tasks
               (id, research_run_id, question_id, question, objective, priority, status,
                assigned_agent, required_evidence, dependencies, iteration)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(t.id)
        .bind(t.research_run_id)
        .bind(t.question_id)
        .bind(&t.question)
        .bind(&t.objective)
        .bind(t.priority.as_str())
        .bind(t.status.as_str())
        .bind(&t.assigned_agent)
        .bind(to_json(&t.required_evidence))
        .bind(to_json(&t.dependencies))
        .bind(t.iteration)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_task_status(&self, id: Uuid, status: TaskStatus, error: Option<&str>) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE research_tasks
             SET status = $2, error = $3,
                 completed_at = CASE WHEN $2 IN ('completed','failed') THEN now() ELSE completed_at END
             WHERE id = $1",
        )
        .bind(id)
        .bind(status.as_str())
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_tasks(&self, run: Uuid) -> Result<Vec<ResearchTask>, sqlx::Error> {
        sqlx::query("SELECT * FROM research_tasks WHERE research_run_id = $1 ORDER BY created_at, id")
            .bind(run)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(task_from_row)
            .collect()
    }

    // ----- sources ----------------------------------------------------------

    /// Stores a source, or returns the id of the one already stored for the
    /// same normalized URL in this run.
    pub async fn upsert_source(&self, s: &Source) -> Result<Uuid, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO sources
               (id, research_run_id, url, normalized_url, title, publisher, published_at,
                retrieved_at, content, content_hash, source_type, nature)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
             ON CONFLICT (research_run_id, normalized_url)
             DO UPDATE SET retrieved_at = sources.retrieved_at
             RETURNING id",
        )
        .bind(s.id)
        .bind(s.research_run_id)
        .bind(&s.url)
        .bind(&s.normalized_url)
        .bind(&s.title)
        .bind(&s.publisher)
        .bind(&s.published_at)
        .bind(s.retrieved_at)
        .bind(&s.content)
        .bind(&s.content_hash)
        .bind(s.source_type.as_str())
        .bind(s.nature.as_str())
        .fetch_one(&self.pool)
        .await?;
        row.try_get("id")
    }

    pub async fn get_source(&self, id: Uuid) -> Result<Option<Source>, sqlx::Error> {
        sqlx::query("SELECT * FROM sources WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| source_from_row(&r))
            .transpose()
    }

    pub async fn list_source_refs(&self, run: Uuid) -> Result<Vec<SourceReference>, sqlx::Error> {
        sqlx::query(
            "SELECT id, url, title, publisher, published_at, source_type, nature
             FROM sources WHERE research_run_id = $1 ORDER BY retrieved_at, id",
        )
        .bind(run)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(source_ref_from_row)
        .collect()
    }

    // ----- claims & evidence ---------------------------------------------

    /// Inserts a claim and its evidence atomically. Returns the stored claim id,
    /// or `None` if the same statement from the same source already exists.
    pub async fn insert_claim_with_evidence(&self, claim: &Claim, excerpt: &str) -> Result<Option<Uuid>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO claims
               (id, research_run_id, research_task_id, source_id, text, confidence, status, iteration, dedupe_key)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (research_run_id, source_id, dedupe_key) DO NOTHING
             RETURNING id",
        )
        .bind(claim.id)
        .bind(claim.research_run_id)
        .bind(claim.research_task_id)
        .bind(claim.source_id)
        .bind(&claim.text)
        .bind(claim.confidence)
        .bind(claim.status.as_str())
        .bind(claim.iteration)
        .bind(claim_dedupe_key(&claim.text))
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = inserted else {
            tx.rollback().await?;
            return Ok(None);
        };
        let id: Uuid = row.try_get("id")?;
        sqlx::query(
            "INSERT INTO evidence (id, claim_id, source_id, excerpt, relevance, support_level)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(claim.source_id)
        .bind(excerpt)
        .bind(claim.confidence)
        .bind(SupportLevel::Unverified.as_str())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(id))
    }

    pub async fn set_claim_verification(
        &self,
        claim_id: Uuid,
        level: SupportLevel,
        confidence: f32,
        reasoning: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE claims SET status = $2, confidence = $3 WHERE id = $1")
            .bind(claim_id)
            .bind(level.as_str())
            .bind(confidence)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE evidence SET support_level = $2, reasoning = $3 WHERE claim_id = $1")
            .bind(claim_id)
            .bind(level.as_str())
            .bind(reasoning)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    /// Claims of a run with evidence, source and originating question.
    pub async fn claim_records(
        &self,
        run: Uuid,
        only_status: Option<SupportLevel>,
        only_iteration: Option<i32>,
    ) -> Result<Vec<ClaimRecord>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT c.id, c.research_run_id, c.research_task_id, c.source_id, c.text, c.confidence,
                    c.status, c.iteration, c.created_at,
                    t.question_id AS question_id, t.question AS task_question
             FROM claims c
             JOIN research_tasks t ON t.id = c.research_task_id
             WHERE c.research_run_id = $1
               AND ($2::text IS NULL OR c.status = $2)
               AND ($3::int IS NULL OR c.iteration = $3)
             ORDER BY c.created_at, c.id",
        )
        .bind(run)
        .bind(only_status.map(|s| s.as_str()))
        .bind(only_iteration)
        .fetch_all(&self.pool)
        .await?;

        let evidence: Vec<Evidence> = sqlx::query(
            "SELECT e.* FROM evidence e JOIN claims c ON c.id = e.claim_id
             WHERE c.research_run_id = $1 ORDER BY e.created_at",
        )
        .bind(run)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(evidence_from_row)
        .collect::<Result<_, _>>()?;
        let sources = self.list_source_refs(run).await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let claim = claim_from_row(row)?;
            let Some(source) = sources.iter().find(|s| s.id == claim.source_id).cloned() else {
                continue;
            };
            out.push(ClaimRecord {
                evidence: evidence.iter().filter(|e| e.claim_id == claim.id).cloned().collect(),
                question_id: row.try_get("question_id")?,
                question: row.try_get("task_question")?,
                source,
                claim,
            });
        }
        Ok(out)
    }

    // ----- contradictions -------------------------------------------------

    /// Returns true if newly stored. Pairs are stored in canonical order so the
    /// same pair is never recorded twice.
    pub async fn insert_contradiction(&self, c: &Contradiction) -> Result<bool, sqlx::Error> {
        let (a, b) =
            if c.claim_a_id <= c.claim_b_id { (c.claim_a_id, c.claim_b_id) } else { (c.claim_b_id, c.claim_a_id) };
        let r = sqlx::query(
            "INSERT INTO contradictions (id, research_run_id, claim_a_id, claim_b_id, kind, explanation)
             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (claim_a_id, claim_b_id) DO NOTHING",
        )
        .bind(c.id)
        .bind(c.research_run_id)
        .bind(a)
        .bind(b)
        .bind(c.kind.as_str())
        .bind(&c.explanation)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn list_contradictions(&self, run: Uuid) -> Result<Vec<Contradiction>, sqlx::Error> {
        sqlx::query("SELECT * FROM contradictions WHERE research_run_id = $1 ORDER BY created_at, id")
            .bind(run)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(contradiction_from_row)
            .collect()
    }

    // ----- reports & critiques ---------------------------------------------

    pub async fn insert_report(&self, r: &Report) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO reports (id, research_run_id, iteration, is_final, content, markdown, citations)
             VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(r.id)
        .bind(r.research_run_id)
        .bind(r.iteration)
        .bind(r.is_final)
        .bind(to_json(&r.content))
        .bind(&r.markdown)
        .bind(to_json(&r.citations))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_report_final(&self, id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE reports SET is_final = TRUE WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// The final report if there is one, otherwise the newest draft.
    pub async fn latest_report(&self, run: Uuid) -> Result<Option<Report>, sqlx::Error> {
        sqlx::query(
            "SELECT * FROM reports WHERE research_run_id = $1
             ORDER BY is_final DESC, created_at DESC LIMIT 1",
        )
        .bind(run)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| report_from_row(&r))
        .transpose()
    }

    pub async fn insert_critique(&self, c: &Critique) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO critiques
               (id, research_run_id, report_id, iteration, quality, needs_more_research, issues, required_research)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(c.id)
        .bind(c.research_run_id)
        .bind(c.report_id)
        .bind(c.iteration)
        .bind(c.quality)
        .bind(c.needs_more_research)
        .bind(to_json(&c.issues))
        .bind(to_json(&c.required_research))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_critiques(&self, run: Uuid) -> Result<Vec<Critique>, sqlx::Error> {
        sqlx::query("SELECT * FROM critiques WHERE research_run_id = $1 ORDER BY created_at")
            .bind(run)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(critique_from_row)
            .collect()
    }

    // ----- events -----------------------------------------------------------

    pub async fn insert_event(
        &self,
        research_id: Uuid,
        agent: &str,
        event_type: &str,
        message: &str,
        metadata: &Value,
    ) -> Result<AgentEvent, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO agent_events (research_id, agent, event_type, message, metadata)
             VALUES ($1,$2,$3,$4,$5) RETURNING *",
        )
        .bind(research_id)
        .bind(agent)
        .bind(event_type)
        .bind(message)
        .bind(metadata)
        .fetch_one(&self.pool)
        .await?;
        event_from_row(&row)
    }

    pub async fn events_after(
        &self,
        research_id: Uuid,
        after_id: i64,
        limit: i64,
    ) -> Result<Vec<AgentEvent>, sqlx::Error> {
        sqlx::query("SELECT * FROM agent_events WHERE research_id = $1 AND id > $2 ORDER BY id LIMIT $3")
            .bind(research_id)
            .bind(after_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(event_from_row)
            .collect()
    }

    // ----- stats ------------------------------------------------------------

    pub async fn run_stats(&self, run: Uuid) -> Result<RunStats, sqlx::Error> {
        let row = sqlx::query(
            "SELECT
               (SELECT COUNT(*) FROM research_questions WHERE research_run_id = $1) AS questions,
               (SELECT COUNT(*) FROM research_tasks WHERE research_run_id = $1) AS tasks_total,
               (SELECT COUNT(*) FROM research_tasks WHERE research_run_id = $1 AND status = 'completed') AS tasks_completed,
               (SELECT COUNT(*) FROM research_tasks WHERE research_run_id = $1 AND status = 'failed') AS tasks_failed,
               (SELECT COUNT(*) FROM research_tasks WHERE research_run_id = $1 AND status = 'running') AS tasks_running,
               (SELECT COUNT(*) FROM sources WHERE research_run_id = $1) AS sources,
               (SELECT COUNT(*) FROM claims WHERE research_run_id = $1) AS claims,
               (SELECT COUNT(*) FROM claims WHERE research_run_id = $1 AND status = 'SUPPORTED') AS supported,
               (SELECT COUNT(*) FROM claims WHERE research_run_id = $1 AND status = 'PARTIALLY_SUPPORTED') AS partially_supported,
               (SELECT COUNT(*) FROM claims WHERE research_run_id = $1 AND status = 'UNVERIFIED') AS unverified,
               (SELECT COUNT(*) FROM claims WHERE research_run_id = $1 AND status = 'UNSUPPORTED') AS unsupported,
               (SELECT COUNT(*) FROM claims WHERE research_run_id = $1 AND status = 'CONTRADICTED') AS contradicted,
               (SELECT COUNT(*) FROM contradictions WHERE research_run_id = $1) AS contradictions,
               (SELECT COUNT(*) FROM contradictions WHERE research_run_id = $1 AND kind = 'DIRECT_CONTRADICTION') AS genuine_contradictions",
        )
        .bind(run)
        .fetch_one(&self.pool)
        .await?;
        Ok(RunStats {
            questions: row.try_get("questions")?,
            tasks_total: row.try_get("tasks_total")?,
            tasks_completed: row.try_get("tasks_completed")?,
            tasks_failed: row.try_get("tasks_failed")?,
            tasks_running: row.try_get("tasks_running")?,
            sources: row.try_get("sources")?,
            claims: row.try_get("claims")?,
            supported: row.try_get("supported")?,
            partially_supported: row.try_get("partially_supported")?,
            unverified: row.try_get("unverified")?,
            unsupported: row.try_get("unsupported")?,
            contradicted: row.try_get("contradicted")?,
            contradictions: row.try_get("contradictions")?,
            genuine_contradictions: row.try_get("genuine_contradictions")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_dedupe_ignores_case_and_spacing() {
        assert_eq!(claim_dedupe_key("AI  agents\nhelp."), claim_dedupe_key("ai agents help."));
        assert_ne!(claim_dedupe_key("a"), claim_dedupe_key("b"));
    }
}
