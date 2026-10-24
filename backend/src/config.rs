//! Runtime configuration, loaded from environment variables.

use std::{env, str::FromStr, time::Duration};

use crate::error::ConfigError;

/// Tunables that bound cost, concurrency and run length.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Maximum research iterations (initial pass + follow-up passes).
    pub max_iterations: u32,
    /// Maximum tasks created per iteration.
    pub max_tasks_per_iteration: usize,
    /// Research tasks executing at the same time.
    pub max_concurrent_tasks: usize,
    /// In-flight LLM requests across the whole process.
    pub max_concurrent_llm_calls: usize,
    /// Research runs executing at the same time.
    pub max_concurrent_runs: usize,
    /// Search queries generated per task.
    pub queries_per_task: usize,
    /// Sources fetched and mined per task.
    pub sources_per_task: usize,
    /// Characters of a source sent to the LLM for claim extraction.
    pub source_char_limit: usize,
    /// Claims extracted per source.
    pub claims_per_source: usize,
    /// Claims verified per LLM call.
    pub verify_batch_size: usize,
    /// Claims compared per contradiction call.
    pub contradiction_group_size: usize,
    /// Verified claims given to the synthesizer and critic; bounds prompt size and cost.
    pub report_claims: usize,
    /// Wall-clock budget for one research task.
    pub task_timeout: Duration,
    /// Per-attempt LLM request timeout.
    pub llm_timeout: Duration,
    /// Per-request search / fetch timeout.
    pub http_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_iterations: 2,
            max_tasks_per_iteration: 6,
            max_concurrent_tasks: 3,
            max_concurrent_llm_calls: 4,
            max_concurrent_runs: 2,
            queries_per_task: 2,
            sources_per_task: 4,
            source_char_limit: 8_000,
            claims_per_source: 4,
            verify_batch_size: 8,
            contradiction_group_size: 12,
            report_claims: 40,
            task_timeout: Duration::from_secs(240),
            llm_timeout: Duration::from_secs(120),
            http_timeout: Duration::from_secs(20),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub grok_api_key: String,
    pub grok_model: String,
    pub grok_base_url: String,
    pub search_api_key: String,
    pub cors_origin: String,
    /// Shared secret required on `/api/*` requests. `None` leaves the API open (local use only).
    pub access_key: Option<String>,
    pub limits: Limits,
}

/// `BIND_ADDR` wins; otherwise `PORT` (set by most hosting platforms) on all interfaces; otherwise `:8080`.
pub fn resolve_bind_addr(bind_addr: Option<&str>, port: Option<&str>) -> String {
    fn clean(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|v| !v.is_empty())
    }
    match (clean(bind_addr), clean(port)) {
        (Some(addr), _) => addr.to_string(),
        (None, Some(port)) => format!("0.0.0.0:{port}"),
        (None, None) => "0.0.0.0:8080".to_string(),
    }
}

/// Empty means "no access control". A set key must be long enough not to be guessable.
pub fn parse_access_key(raw: Option<&str>) -> Result<Option<String>, ConfigError> {
    match raw.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(k) if k.len() < crate::api::auth::MIN_KEY_LEN => {
            Err(ConfigError::TooShort("API_ACCESS_KEY", crate::api::auth::MIN_KEY_LEN))
        }
        Some(k) => Ok(Some(k.to_string())),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut limits = Limits::default();
        limits.max_iterations = parse_or("MAX_ITERATIONS", limits.max_iterations)?;
        limits.max_tasks_per_iteration = parse_or("MAX_TASKS_PER_ITERATION", limits.max_tasks_per_iteration)?;
        limits.max_concurrent_tasks = parse_or("MAX_CONCURRENT_TASKS", limits.max_concurrent_tasks)?;
        limits.max_concurrent_llm_calls = parse_or("MAX_CONCURRENT_LLM_CALLS", limits.max_concurrent_llm_calls)?;
        limits.max_concurrent_runs = parse_or("MAX_CONCURRENT_RUNS", limits.max_concurrent_runs)?;
        limits.sources_per_task = parse_or("SOURCES_PER_TASK", limits.sources_per_task)?;
        limits.report_claims = parse_or("REPORT_MAX_CLAIMS", limits.report_claims)?.max(1);
        limits.task_timeout = Duration::from_secs(parse_or("TASK_TIMEOUT_SECS", 240u64)?);
        limits.llm_timeout = Duration::from_secs(parse_or("LLM_TIMEOUT_SECS", 120u64)?);
        limits.http_timeout = Duration::from_secs(parse_or("HTTP_TIMEOUT_SECS", 20u64)?);

        Ok(Self {
            bind_addr: resolve_bind_addr(env::var("BIND_ADDR").ok().as_deref(), env::var("PORT").ok().as_deref()),
            database_url: required("DATABASE_URL")?,
            grok_api_key: required("GROK_API_KEY")?,
            grok_model: env::var("GROK_MODEL").unwrap_or_else(|_| "grok-4".into()),
            grok_base_url: env::var("GROK_BASE_URL").unwrap_or_else(|_| "https://api.x.ai/v1".into()),
            search_api_key: required("SEARCH_API_KEY")?,
            cors_origin: env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into()),
            access_key: parse_access_key(env::var("API_ACCESS_KEY").ok().as_deref())?,
            limits,
        })
    }
}

fn required(key: &'static str) -> Result<String, ConfigError> {
    match env::var(key) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(ConfigError::Missing(key)),
    }
}

fn parse_or<T: FromStr>(key: &'static str, default: T) -> Result<T, ConfigError> {
    match env::var(key) {
        Ok(v) if !v.trim().is_empty() => v.trim().parse().map_err(|_| ConfigError::Invalid(key, v)),
        _ => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_address_prefers_explicit_then_port_then_default() {
        assert_eq!(resolve_bind_addr(Some("127.0.0.1:9000"), Some("1234")), "127.0.0.1:9000");
        assert_eq!(resolve_bind_addr(None, Some("8081")), "0.0.0.0:8081");
        assert_eq!(resolve_bind_addr(Some("  "), Some(" 8081 ")), "0.0.0.0:8081");
        assert_eq!(resolve_bind_addr(None, None), "0.0.0.0:8080");
        assert_eq!(resolve_bind_addr(Some(""), Some("")), "0.0.0.0:8080");
    }

    #[test]
    fn access_key_is_optional_but_never_weak() {
        assert_eq!(parse_access_key(None).unwrap(), None);
        assert_eq!(parse_access_key(Some("   ")).unwrap(), None);
        assert!(matches!(parse_access_key(Some("short")), Err(ConfigError::TooShort("API_ACCESS_KEY", _))));
        assert_eq!(
            parse_access_key(Some(" a-long-enough-secret-key ")).unwrap().as_deref(),
            Some("a-long-enough-secret-key")
        );
    }

    #[test]
    fn a_rejected_key_is_never_echoed_in_the_error() {
        let err = parse_access_key(Some("tiny-secret")).unwrap_err().to_string();
        assert!(!err.contains("tiny-secret"), "{err}");
    }
}
