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
    pub limits: Limits,
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
        limits.task_timeout = Duration::from_secs(parse_or("TASK_TIMEOUT_SECS", 240u64)?);
        limits.llm_timeout = Duration::from_secs(parse_or("LLM_TIMEOUT_SECS", 120u64)?);
        limits.http_timeout = Duration::from_secs(parse_or("HTTP_TIMEOUT_SECS", 20u64)?);

        Ok(Self {
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url: required("DATABASE_URL")?,
            grok_api_key: required("GROK_API_KEY")?,
            grok_model: env::var("GROK_MODEL").unwrap_or_else(|_| "grok-4".into()),
            grok_base_url: env::var("GROK_BASE_URL").unwrap_or_else(|_| "https://api.x.ai/v1".into()),
            search_api_key: required("SEARCH_API_KEY")?,
            cors_origin: env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into()),
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
