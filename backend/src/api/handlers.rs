use std::{convert::Infallible, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures::{Stream, StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use super::AppState;
use crate::{
    database::{ClaimRecord, RunStats},
    error::ApiError,
    models::*,
};

const MIN_QUERY_CHARS: usize = 8;
const MAX_QUERY_CHARS: usize = 2000;

#[derive(Deserialize)]
pub struct CreateResearch {
    pub query: String,
}

pub fn validate_query(raw: &str) -> Result<String, ApiError> {
    let q = raw.trim();
    let len = q.chars().count();
    if len < MIN_QUERY_CHARS {
        return Err(ApiError::BadRequest(format!("query must be at least {MIN_QUERY_CHARS} characters")));
    }
    if len > MAX_QUERY_CHARS {
        return Err(ApiError::BadRequest(format!("query must be at most {MAX_QUERY_CHARS} characters")));
    }
    Ok(q.to_string())
}

pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

pub async fn create_research(
    State(state): State<AppState>,
    Json(body): Json<CreateResearch>,
) -> Result<impl IntoResponse, ApiError> {
    let query = validate_query(&body.query)?;
    let run = state.orchestrator.start(&query).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"research_id": run.id, "status": run.status}))))
}

pub async fn list_research(State(state): State<AppState>) -> Result<Json<Vec<ResearchRun>>, ApiError> {
    Ok(Json(state.repo.list_runs(50).await?))
}

#[derive(Serialize)]
pub struct ResearchOverview {
    pub run: ResearchRun,
    pub questions: Vec<ResearchQuestion>,
    pub tasks: Vec<ResearchTask>,
    pub stats: RunStats,
}

async fn require_run(state: &AppState, id: Uuid) -> Result<ResearchRun, ApiError> {
    state.repo.get_run(id).await?.ok_or(ApiError::NotFound)
}

pub async fn get_research(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<ResearchOverview>, ApiError> {
    let run = require_run(&state, id).await?;
    Ok(Json(ResearchOverview {
        questions: state.repo.list_questions(id).await?,
        tasks: state.repo.list_tasks(id).await?,
        stats: state.repo.run_stats(id).await?,
        run,
    }))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    pub after: Option<i64>,
    pub limit: Option<i64>,
}

pub async fn get_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Vec<AgentEvent>>, ApiError> {
    require_run(&state, id).await?;
    let limit = q.limit.unwrap_or(500).clamp(1, 2000);
    Ok(Json(state.repo.events_after(id, q.after.unwrap_or(0), limit).await?))
}

pub async fn get_sources(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Vec<SourceReference>>, ApiError> {
    require_run(&state, id).await?;
    Ok(Json(state.repo.list_source_refs(id).await?))
}

/// Claims with their evidence and source: the claim -> evidence -> source -> URL trace.
pub async fn get_claims(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Vec<ClaimRecord>>, ApiError> {
    require_run(&state, id).await?;
    Ok(Json(state.repo.claim_records(id, None, None).await?))
}

pub async fn get_contradictions(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Vec<Contradiction>>, ApiError> {
    require_run(&state, id).await?;
    Ok(Json(state.repo.list_contradictions(id).await?))
}

pub async fn get_critiques(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Vec<Critique>>, ApiError> {
    require_run(&state, id).await?;
    Ok(Json(state.repo.list_critiques(id).await?))
}

pub async fn get_report(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Report>, ApiError> {
    require_run(&state, id).await?;
    state.repo.latest_report(id).await?.map(Json).ok_or(ApiError::NotFound)
}

/// Server-Sent Events: replays persisted events after `Last-Event-ID` (or
/// `?after=`), then follows live events. Subscribing before replaying plus the
/// id filter guarantees no gap and no duplicates.
pub async fn stream_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    require_run(&state, id).await?;
    let after = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .or(q.after)
        .unwrap_or(0);

    let live = state.orchestrator.events().subscribe();
    let replay = state.repo.events_after(id, after, 5000).await?;
    let last_replayed = replay.last().map(|e| e.id).unwrap_or(after);

    let replay_stream = stream::iter(replay);
    let live_stream = BroadcastStream::new(live).filter_map(move |item| async move {
        match item {
            Ok(ev) if ev.research_id == id && ev.id > last_replayed => Some(ev),
            // Lagged receivers drop events; clients recover by reconnecting
            // with Last-Event-ID, which replays from the database.
            _ => None,
        }
    });
    let events = replay_stream.chain(live_stream).map(|ev| {
        Ok(Event::default()
            .id(ev.id.to_string())
            .json_data(&ev)
            .unwrap_or_else(|_| Event::default().comment("serialization error")))
    });
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_length_is_validated_after_trimming() {
        assert!(validate_query("   short ").is_err());
        assert_eq!(validate_query("  What limits autonomous agents?  ").unwrap(), "What limits autonomous agents?");
        assert!(validate_query(&"x".repeat(MAX_QUERY_CHARS + 1)).is_err());
        assert!(validate_query(&"x".repeat(MAX_QUERY_CHARS)).is_ok());
    }
}
