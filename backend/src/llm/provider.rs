use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::error::LlmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: MessageRole,
    pub content: String,
}

impl LlmMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: MessageRole::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: MessageRole::Assistant, content: content.into() }
    }
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    /// Agent-specific system instructions.
    pub system: String,
    pub messages: Vec<LlmMessage>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    /// Ask the provider to emit a single JSON object.
    pub json_mode: bool,
    /// Which agent is calling; used for logging only.
    pub label: &'static str,
}

impl LlmRequest {
    pub fn json(label: &'static str, system: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            system: system.into(),
            messages: vec![LlmMessage::user(user)],
            temperature: 0.2,
            max_tokens: None,
            json_mode: true,
            label,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<Usage>,
}

/// Provider-independent text generation.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
}

/// Decorator that bounds the number of simultaneous LLM requests.
pub struct LimitedProvider {
    inner: Arc<dyn LlmProvider>,
    permits: Semaphore,
}

impl LimitedProvider {
    pub fn new(inner: Arc<dyn LlmProvider>, max_concurrent: usize) -> Self {
        Self { inner, permits: Semaphore::new(max_concurrent.max(1)) }
    }
}

#[async_trait]
impl LlmProvider for LimitedProvider {
    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let _permit = self.permits.acquire().await.map_err(|_| LlmError::Network("llm limiter closed".into()))?;
        self.inner.generate(request).await
    }
}
