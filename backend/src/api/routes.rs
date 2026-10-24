use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use super::{
    AppState,
    auth::{AccessKey, require_access_key},
    handlers,
};

/// `access_key = None` leaves the API open (local use). `/health` is always open.
pub fn router(state: AppState, cors: CorsLayer, access_key: Option<AccessKey>) -> Router {
    let api = Router::new()
        .route("/api/research", post(handlers::create_research).get(handlers::list_research))
        .route("/api/research/{id}", get(handlers::get_research))
        .route("/api/research/{id}/events", get(handlers::get_events))
        .route("/api/research/{id}/sources", get(handlers::get_sources))
        .route("/api/research/{id}/claims", get(handlers::get_claims))
        .route("/api/research/{id}/contradictions", get(handlers::get_contradictions))
        .route("/api/research/{id}/critiques", get(handlers::get_critiques))
        .route("/api/research/{id}/report", get(handlers::get_report))
        .route("/api/research/{id}/stream", get(handlers::stream_events))
        .route_layer(from_fn_with_state(access_key, require_access_key));

    Router::new()
        .route("/health", get(handlers::health))
        .merge(api)
        // CORS is outermost so browser preflight (OPTIONS, which carries no key) is answered before auth.
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
