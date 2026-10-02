//! xAI Grok implementation of [`LlmProvider`] (OpenAI-compatible chat API).

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::provider::{LlmProvider, LlmRequest, LlmResponse, Usage};
use crate::{
    error::LlmError,
    util::retry::{RetryPolicy, retry},
};

pub struct GrokProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
    retry: RetryPolicy,
}

#[derive(Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct WireResponse {
    model: Option<String>,
    choices: Vec<WireChoice>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireChoiceMessage,
}

#[derive(Deserialize)]
struct WireChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

impl GrokProvider {
    pub fn new(api_key: String, model: String, base_url: String, timeout: Duration) -> Result<Self, LlmError> {
        let client = Client::builder().timeout(timeout).build().map_err(|e| LlmError::Network(e.to_string()))?;
        Ok(Self {
            client,
            api_key,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            retry: RetryPolicy::default(),
        })
    }

    fn build_body(&self, req: &LlmRequest) -> Value {
        let mut messages = vec![json!({"role": "system", "content": req.system})];
        for m in &req.messages {
            let role = match m.role {
                super::provider::MessageRole::User => "user",
                super::provider::MessageRole::Assistant => "assistant",
            };
            messages.push(serde_json::to_value(WireMessage { role, content: &m.content }).unwrap_or(Value::Null));
        }
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "temperature": req.temperature,
            "stream": false,
        });
        if let Some(max) = req.max_tokens {
            body["max_tokens"] = json!(max);
        }
        if req.json_mode {
            body["response_format"] = json!({"type": "json_object"});
        }
        body
    }

    async fn attempt(&self, body: &Value) -> Result<LlmResponse, LlmError> {
        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { LlmError::Timeout } else { LlmError::Network(e.to_string()) })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let body = text.chars().take(500).collect::<String>();
            return Err(match status {
                StatusCode::TOO_MANY_REQUESTS => LlmError::RateLimited,
                s if s.is_server_error() => LlmError::Server { status: s.as_u16(), body },
                s => LlmError::Rejected { status: s.as_u16(), body },
            });
        }

        let parsed: WireResponse = resp.json().await.map_err(|e| LlmError::InvalidResponse(e.to_string()))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .filter(|c| !c.trim().is_empty())
            .ok_or_else(|| LlmError::InvalidResponse("empty completion".into()))?;
        Ok(LlmResponse {
            content,
            model: parsed.model.unwrap_or_else(|| self.model.clone()),
            usage: parsed
                .usage
                .map(|u| Usage { prompt_tokens: u.prompt_tokens, completion_tokens: u.completion_tokens }),
        })
    }
}

#[async_trait]
impl LlmProvider for GrokProvider {
    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let body = self.build_body(&request);
        let label = request.label;
        retry(self.retry, LlmError::is_retryable, |attempt| {
            let body = &body;
            async move {
                let result = self.attempt(body).await;
                if let Err(e) = &result {
                    tracing::warn!(agent = label, attempt, error = %e, "grok request failed");
                }
                result
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::LlmMessage;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    fn provider(base: &str) -> GrokProvider {
        let mut p = GrokProvider::new("k".into(), "grok-test".into(), base.into(), Duration::from_secs(5)).unwrap();
        p.retry = RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(2));
        p
    }

    fn request() -> LlmRequest {
        LlmRequest {
            system: "sys".into(),
            messages: vec![LlmMessage::user("hi")],
            temperature: 0.0,
            max_tokens: None,
            json_mode: true,
            label: "test",
        }
    }

    fn ok_body(text: &str) -> serde_json::Value {
        json!({"model": "grok-test", "choices": [{"message": {"content": text}}],
               "usage": {"prompt_tokens": 3, "completion_tokens": 4}})
    }

    #[tokio::test]
    async fn sends_auth_and_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("{\"a\":1}")))
            .mount(&server)
            .await;
        let out = provider(&server.uri()).generate(request()).await.unwrap();
        assert_eq!(out.content, "{\"a\":1}");
        assert_eq!(out.usage.unwrap().completion_tokens, 4);
    }

    #[tokio::test]
    async fn retries_rate_limits_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(429)).up_to_n_times(2).mount(&server).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("ok")))
            .mount(&server)
            .await;
        let out = provider(&server.uri()).generate(request()).await.unwrap();
        assert_eq!(out.content, "ok");
    }

    #[tokio::test]
    async fn does_not_retry_client_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
            .expect(1)
            .mount(&server)
            .await;
        let err = provider(&server.uri()).generate(request()).await.unwrap_err();
        assert!(matches!(err, LlmError::Rejected { status: 401, .. }));
    }

    #[tokio::test]
    async fn gives_up_after_bounded_retries() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(503)).expect(3).mount(&server).await;
        let err = provider(&server.uri()).generate(request()).await.unwrap_err();
        assert!(matches!(err, LlmError::Server { status: 503, .. }));
    }
}
