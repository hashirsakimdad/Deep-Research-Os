use axum::{
    Router,
    routing::{get, post},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use super::{AppState, handlers};

pub fn router(state: AppState, cors: CorsLayer) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/api/research", post(handlers::create_research).get(handlers::list_research))
        .route("/api/research/{id}", get(handlers::get_research))
        .route("/api/research/{id}/events", get(handlers::get_events))
        .route("/api/research/{id}/sources", get(handlers::get_sources))
        .route("/api/research/{id}/claims", get(handlers::get_claims))
        .route("/api/research/{id}/contradictions", get(handlers::get_contradictions))
        .route("/api/research/{id}/critiques", get(handlers::get_critiques))
        .route("/api/research/{id}/report", get(handlers::get_report))
        .route("/api/research/{id}/stream", get(handlers::stream_events))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
