//! Typed errors for every layer of the application.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value {1:?} for environment variable {0}")]
    Invalid(&'static str, String),
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("llm request timed out")]
    Timeout,
    #[error("llm rate limited")]
    RateLimited,
    #[error("llm server error ({status}): {body}")]
    Server { status: u16, body: String },
    #[error("llm rejected request ({status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("llm network error: {0}")]
    Network(String),
    #[error("invalid llm response: {0}")]
    InvalidResponse(String),
}

impl LlmError {
    /// Whether retrying the same request may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::RateLimited | Self::Server { .. } | Self::Network(_))
    }
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search timed out")]
    Timeout,
    #[error("search rate limited")]
    RateLimited,
    #[error("search provider error: {0}")]
    Provider(String),
    #[error("search network error: {0}")]
    Network(String),
}

impl SearchError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::RateLimited | Self::Network(_))
    }
}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("fetch timed out")]
    Timeout,
    #[error("fetch failed: {0}")]
    Network(String),
    #[error("unsupported content type {0}")]
    UnsupportedContent(String),
    #[error("page returned status {0}")]
    Status(u16),
    #[error("page had no usable text")]
    Empty,
}

impl FetchError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::Network(_))
    }
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("{agent}: invalid input: {reason}")]
    InvalidInput { agent: &'static str, reason: String },
    #[error("{agent}: llm failure: {source}")]
    Llm {
        agent: &'static str,
        #[source]
        source: LlmError,
    },
    #[error("{agent}: output failed validation: {reason}")]
    InvalidOutput { agent: &'static str, reason: String },
    #[error("{agent}: search failure: {source}")]
    Search {
        agent: &'static str,
        #[source]
        source: SearchError,
    },
    #[error("{agent}: no usable sources found")]
    NoSources { agent: &'static str },
    #[error("{agent}: timed out")]
    Timeout { agent: &'static str },
    #[error("{agent}: cancelled")]
    Cancelled { agent: &'static str },
    #[error("{agent}: storage failure: {reason}")]
    Storage { agent: &'static str, reason: String },
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("invalid status transition {from} -> {to}")]
    InvalidTransition { from: String, to: String },
    #[error("iteration limit of {0} reached")]
    IterationLimit(u32),
}

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("research run {0} was cancelled")]
    Cancelled(Uuid),
    #[error("all research tasks failed: {0}")]
    AllTasksFailed(String),
    #[error("{0}")]
    Other(String),
}

/// Error type returned by API handlers.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ApiError::Database(e) => {
                tracing::error!(error = %e, "database error in api handler");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
