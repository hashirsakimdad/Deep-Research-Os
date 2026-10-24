//! HTTP API (Axum). Handlers read persisted state; research runs are driven
//! by the orchestrator in the background.

pub mod auth;
pub mod handlers;
pub mod routes;

use std::sync::Arc;

use crate::{database::Repository, orchestrator::Orchestrator};

#[derive(Clone)]
pub struct AppState {
    pub repo: Repository,
    pub orchestrator: Arc<Orchestrator>,
}

pub use routes::router;
