use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::error::AgentError;

/// A component with an identity, a strongly typed input and a typed output.
#[async_trait]
pub trait Agent: Send + Sync {
    type Input: Send;
    type Output: Send;

    fn name(&self) -> &'static str;

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, AgentError>;
}

/// Where agents report things they actually did. The orchestrator's event bus
/// persists and broadcasts these; the UI renders them.
#[async_trait]
pub trait EventSink: Send + Sync {
    async fn emit(&self, research_id: Uuid, agent: &str, event_type: &str, message: String, metadata: Value);
}

/// Sink that discards events (tests, one-off tools).
pub struct NoopSink;

#[async_trait]
impl EventSink for NoopSink {
    async fn emit(&self, _: Uuid, _: &str, _: &str, _: String, _: Value) {}
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod testing {
    use std::sync::Mutex;

    use super::*;

    /// Records every emitted event as (agent, event_type, message).
    #[derive(Default)]
    pub struct RecordingSink {
        pub events: Mutex<Vec<(String, String, String)>>,
    }

    impl RecordingSink {
        pub fn count(&self, event_type: &str) -> usize {
            self.events.lock().unwrap().iter().filter(|e| e.1 == event_type).count()
        }
    }

    #[async_trait]
    impl EventSink for RecordingSink {
        async fn emit(&self, _: Uuid, agent: &str, event_type: &str, message: String, _: Value) {
            self.events.lock().unwrap().push((agent.into(), event_type.into(), message));
        }
    }
}

