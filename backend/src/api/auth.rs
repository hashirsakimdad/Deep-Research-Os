//! Shared-secret access control for the API.
//!
//! When `API_ACCESS_KEY` is set, every `/api/*` request must present it as
//! `x-api-key: <key>` or `Authorization: Bearer <key>`. `/health` stays open for platform health checks.
//! When it is not set the API is open (fine on localhost, not on the internet).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use sha2::{Digest, Sha256};

/// Minimum accepted key length; short keys are guessable.
pub const MIN_KEY_LEN: usize = 16;

/// SHA-256 of the configured key. Requests are hashed too, so the comparison is over fixed-size digests
/// and takes the same time whatever the input length.
#[derive(Clone)]
pub struct AccessKey(Arc<[u8; 32]>);

impl AccessKey {
    pub fn new(key: &str) -> Self {
        Self(Arc::new(digest(key)))
    }

    fn matches(&self, presented: &str) -> bool {
        let theirs = digest(presented);
        // constant-time over the 32 bytes
        self.0.iter().zip(theirs.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

fn digest(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

fn presented_key(headers: &HeaderMap) -> Option<&str> {
    if let Some(v) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(v.trim());
    }
    headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(str::trim)
}

/// Middleware: reject requests without the right key. `None` means access control is disabled.
pub async fn require_access_key(State(key): State<Option<AccessKey>>, req: Request, next: Next) -> Response {
    let Some(key) = key else { return next.run(req).await };
    match presented_key(req.headers()) {
        Some(p) if key.matches(p) => next.run(req).await,
        _ => (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request as HttpRequest, StatusCode},
        middleware::from_fn_with_state,
        routing::get,
    };
    use tower::ServiceExt;

    use super::*;

    const KEY: &str = "correct-horse-battery-staple";

    fn app(key: Option<&str>) -> Router {
        Router::new()
            .route("/api/x", get(|| async { "secret" }))
            .route_layer(from_fn_with_state(key.map(AccessKey::new), require_access_key))
    }

    async fn status(app: Router, header: Option<(&str, &str)>) -> StatusCode {
        let mut req = HttpRequest::builder().uri("/api/x");
        if let Some((k, v)) = header {
            req = req.header(k, v);
        }
        app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap().status()
    }

    #[tokio::test]
    async fn rejects_missing_and_wrong_keys() {
        assert_eq!(status(app(Some(KEY)), None).await, StatusCode::UNAUTHORIZED);
        assert_eq!(status(app(Some(KEY)), Some(("x-api-key", "nope"))).await, StatusCode::UNAUTHORIZED);
        assert_eq!(status(app(Some(KEY)), Some(("x-api-key", ""))).await, StatusCode::UNAUTHORIZED);
        assert_eq!(status(app(Some(KEY)), Some(("authorization", "Bearer nope"))).await, StatusCode::UNAUTHORIZED);
        assert_eq!(status(app(Some(KEY)), Some(("authorization", KEY))).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn accepts_the_key_in_either_header() {
        assert_eq!(status(app(Some(KEY)), Some(("x-api-key", KEY))).await, StatusCode::OK);
        let bearer = format!("Bearer {KEY}");
        assert_eq!(status(app(Some(KEY)), Some(("authorization", bearer.as_str()))).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn is_open_when_no_key_is_configured() {
        assert_eq!(status(app(None), None).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_401_body_does_not_leak_anything() {
        let resp =
            app(Some(KEY)).oneshot(HttpRequest::builder().uri("/api/x").body(Body::empty()).unwrap()).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], br#"{"error":"unauthorized"}"#);
    }
}
