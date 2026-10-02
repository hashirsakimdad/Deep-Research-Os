use std::sync::Arc;

use anyhow::Context;
use axum::http::HeaderValue;
use deep_research_os_backend::{
    api::{self, AppState},
    config::Config,
    database::{self, Repository},
    llm::GrokProvider,
    orchestrator::{EventBus, Orchestrator},
    tools::{HttpFetcher, TavilySearch},
};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Accept a .env in the backend dir or the repo root.
    let _ = dotenvy::dotenv().or_else(|_| dotenvy::from_filename("../.env"));
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,deep_research_os_backend=debug")),
        )
        .init();

    let config = Config::from_env().context("invalid configuration")?;
    let pool = database::connect(&config.database_url).await.context("database connection / migration failed")?;
    let repo = Repository::new(pool);
    let interrupted = repo.fail_interrupted_runs().await?;
    if interrupted > 0 {
        tracing::warn!(interrupted, "marked runs left over from a previous process as failed");
    }

    let llm = Arc::new(GrokProvider::new(
        config.grok_api_key.clone(),
        config.grok_model.clone(),
        config.grok_base_url.clone(),
        config.limits.llm_timeout,
    )?);
    let search = Arc::new(TavilySearch::new(config.search_api_key.clone(), config.limits.http_timeout, 6)?);
    let fetcher = Arc::new(HttpFetcher::new(config.limits.http_timeout)?);

    let cancel = CancellationToken::new();
    let events = Arc::new(EventBus::new(repo.clone()));
    let orchestrator =
        Arc::new(Orchestrator::new(repo.clone(), events, llm, search, fetcher, config.limits.clone(), cancel.clone()));

    let cors = CorsLayer::new()
        .allow_origin(config.cors_origin.parse::<HeaderValue>().context("invalid CORS_ORIGIN")?)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = api::router(AppState { repo, orchestrator }, cors);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .with_context(|| format!("cannot bind {}", config.bind_addr))?;
    tracing::info!(addr = %config.bind_addr, model = %config.grok_model, "deep research os listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutdown requested; cancelling active research");
            cancel.cancel();
        })
        .await?;
    Ok(())
}
