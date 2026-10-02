//! Event bus: every event is persisted first, then broadcast to live
//! subscribers, so the UI only ever shows things that really happened.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{agents::EventSink, database::Repository, models::AgentEvent};

#[derive(Clone)]
pub struct EventBus {
    repo: Repository,
    tx: broadcast::Sender<AgentEvent>,
}

impl EventBus {
    pub fn new(repo: Repository) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { repo, tx }
    }

    /// Live events for all runs; callers filter by `research_id`. A lagging
    /// subscriber can recover by replaying from the database.
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.tx.subscribe()
    }
}

#[async_trait]
impl EventSink for EventBus {
    async fn emit(&self, research_id: Uuid, agent: &str, event_type: &str, message: String, metadata: Value) {
        tracing::info!(%research_id, agent, event_type, %message, "research event");
        match self.repo.insert_event(research_id, agent, event_type, &message, &metadata).await {
            Ok(event) => {
                // No subscribers is fine.
                let _ = self.tx.send(event);
            }
            Err(e) => tracing::error!(error = %e, event_type, "failed to persist event"),
        }
    }
}
