use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::{
    error::SearchError,
    models::SearchResult,
    util::{
        retry::{RetryPolicy, retry},
        text::host_of,
    },
};

/// Web search abstraction. The research agent depends only on this trait.
#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, SearchError>;
}

/// [Tavily](https://tavily.com) search API.
pub struct TavilySearch {
    client: Client,
    api_key: String,
    base_url: String,
    max_results: usize,
    retry: RetryPolicy,
}

#[derive(Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    results: Vec<TavilyHit>,
}

#[derive(Deserialize)]
struct TavilyHit {
    title: Option<String>,
    url: String,
    content: Option<String>,
    raw_content: Option<String>,
    published_date: Option<String>,
}

impl TavilySearch {
    pub fn new(api_key: String, timeout: Duration, max_results: usize) -> Result<Self, SearchError> {
        Self::with_base_url(api_key, "https://api.tavily.com".into(), timeout, max_results)
    }

    pub fn with_base_url(
        api_key: String,
        base_url: String,
        timeout: Duration,
        max_results: usize,
    ) -> Result<Self, SearchError> {
        let client = Client::builder().timeout(timeout).build().map_err(|e| SearchError::Network(e.to_string()))?;
        Ok(Self {
            client,
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            max_results,
            retry: RetryPolicy::default(),
        })
    }

    async fn attempt(&self, query: &str) -> Result<Vec<SearchResult>, SearchError> {
        let resp = self
            .client
            .post(format!("{}/search", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "query": query,
                "max_results": self.max_results,
                "search_depth": "basic",
                "include_raw_content": "text",
            }))
            .send()
            .await
            .map_err(|e| if e.is_timeout() { SearchError::Timeout } else { SearchError::Network(e.to_string()) })?;
        match resp.status() {
            StatusCode::TOO_MANY_REQUESTS => return Err(SearchError::RateLimited),
            s if s.is_server_error() => {
                return Err(SearchError::Network(format!("search provider returned {s}")));
            }
            s if !s.is_success() => {
                return Err(SearchError::Provider(format!("search provider returned {s}")));
            }
            _ => {}
        }
        let parsed: TavilyResponse =
            resp.json().await.map_err(|e| SearchError::Provider(format!("unreadable response: {e}")))?;
        Ok(parsed
            .results
            .into_iter()
            .map(|h| SearchResult {
                source: host_of(&h.url).unwrap_or_default(),
                title: h.title.unwrap_or_else(|| h.url.clone()),
                snippet: h.content.unwrap_or_default(),
                published_at: h.published_date,
                content: h.raw_content.filter(|c| c.trim().len() > 200),
                url: h.url,
            })
            .collect())
    }
}

#[async_trait]
impl SearchProvider for TavilySearch {
    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, SearchError> {
        retry(self.retry, SearchError::is_retryable, |_| self.attempt(query)).await
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::{collections::HashMap, sync::Mutex};

    use super::*;

    /// Returns canned results per query (or a default list) and counts calls.
    #[derive(Default)]
    pub struct MockSearch {
        pub by_query: Mutex<HashMap<String, Result<Vec<SearchResult>, String>>>,
        pub default: Mutex<Vec<SearchResult>>,
        pub calls: Mutex<Vec<String>>,
    }

    impl MockSearch {
        pub fn with_default(results: Vec<SearchResult>) -> Self {
            Self { default: Mutex::new(results), ..Self::default() }
        }
    }

    #[async_trait]
    impl SearchProvider for MockSearch {
        async fn search(&self, query: &str) -> Result<Vec<SearchResult>, SearchError> {
            self.calls.lock().unwrap().push(query.to_string());
            match self.by_query.lock().unwrap().get(query) {
                Some(Ok(r)) => Ok(r.clone()),
                Some(Err(e)) => Err(SearchError::Provider(e.clone())),
                None => Ok(self.default.lock().unwrap().clone()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    fn client(base: &str) -> TavilySearch {
        let mut s = TavilySearch::with_base_url("key".into(), base.into(), Duration::from_secs(5), 5).unwrap();
        s.retry = RetryPolicy::new(2, Duration::from_millis(1), Duration::from_millis(2));
        s
    }

    #[tokio::test]
    async fn maps_results() {
        let server = MockServer::start().await;
        let long = "x".repeat(300);
        Mock::given(method("POST"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {"title": "A", "url": "https://www.example.com/a", "content": "snip",
                     "raw_content": long, "published_date": "2025-01-02"},
                    {"url": "https://b.org/b", "raw_content": "short"}
                ]
            })))
            .mount(&server)
            .await;
        let hits = client(&server.uri()).search("q").await.unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].source, "example.com");
        assert_eq!(hits[0].published_at.as_deref(), Some("2025-01-02"));
        assert!(hits[0].content.is_some());
        assert_eq!(hits[1].title, "https://b.org/b");
        assert!(hits[1].content.is_none());
    }

    #[tokio::test]
    async fn retries_transient_failures_only() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(429)).up_to_n_times(1).mount(&server).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
            .mount(&server)
            .await;
        assert!(client(&server.uri()).search("q").await.unwrap().is_empty());

        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(401)).expect(1).mount(&server).await;
        assert!(matches!(client(&server.uri()).search("q").await, Err(SearchError::Provider(_))));
    }
}
