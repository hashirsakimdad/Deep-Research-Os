//! xAI Grok implementation of [`LlmProvider`] (OpenAI-compatible chat API).

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::provider::{LlmProvider, LlmRequest, LlmResponse, Usage};
use crate::{
    error::LlmError,
    util::retry::{RetryPolicy, retry_hinted},
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
            // Patient enough to ride out per-minute token limits on rate-limited tiers.
            retry: RetryPolicy::new(6, Duration::from_secs(3), Duration::from_secs(30)),
        })
    }

    fn build_body(&self, req: &LlmRequest) -> Value {
        // JSON mode on OpenAI-compatible APIs is rejected unless the messages mention "json".
        let mentions_json = req.system.to_lowercase().contains("json")
            || req.messages.iter().any(|m| m.content.to_lowercase().contains("json"));
        let system = if req.json_mode && !mentions_json {
            format!("{}\n\nRespond with a single valid JSON object and nothing else.", req.system)
        } else {
            req.system.clone()
        };
        let mut messages = vec![json!({"role": "system", "content": system})];
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
            let retry_after = parse_retry_after(resp.headers().get(reqwest::header::RETRY_AFTER));
            let text = resp.text().await.unwrap_or_default();
            let body = text.chars().take(500).collect::<String>();
            return Err(match status {
                StatusCode::TOO_MANY_REQUESTS => LlmError::RateLimited { retry_after },
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

/// `Retry-After` as whole seconds (the form rate-limited LLM APIs use), plus a small margin.
fn parse_retry_after(value: Option<&reqwest::header::HeaderValue>) -> Option<Duration> {
    let secs: f64 = value?.to_str().ok()?.trim().parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs) + Duration::from_millis(250))
}

#[async_trait]
impl LlmProvider for GrokProvider {
    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let mut body = self.build_body(&request);
        let label = request.label;
        let mut json_mode_dropped = false;
        loop {
            let result = retry_hinted(self.retry, LlmError::is_retryable, LlmError::retry_after, |attempt| {
                let body = &body;
                async move {
                    let result = self.attempt(body).await;
                    if let Err(e) = &result {
                        tracing::warn!(agent = label, attempt, error = %e, "grok request failed");
                    }
                    result
                }
            })
            .await;
            match &result {
                // Strict server-side JSON validation can reject long, valid-looking replies.
                // Retry once in plain mode; the structured-output layer extracts and repairs JSON.
                Err(LlmError::Rejected { status: 400, body: msg })
                    if !json_mode_dropped && msg.contains("json_validate_failed") =>
                {
                    tracing::warn!(agent = label, "json mode rejected the reply; retrying without response_format");
                    if let Some(obj) = body.as_object_mut() {
                        obj.remove("response_format");
                    }
                    json_mode_dropped = true;
                }
                _ => return result,
            }
        }
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

    #[test]
    fn json_mode_always_mentions_json() {
        let p = provider("http://unused");
        let mut req = request();
        req.system = "Reply with ONLY: {\"queries\": []}".into();
        req.messages = vec![LlmMessage::user("write queries")];
        let body = p.build_body(&req);
        assert!(body["messages"][0]["content"].as_str().unwrap().to_lowercase().contains("json"));

        // already mentioned: left untouched
        req.system = "Return JSON only".into();
        assert_eq!(p.build_body(&req)["messages"][0]["content"], "Return JSON only");

        // not in json mode: left untouched
        req.system = "plain".into();
        req.json_mode = false;
        assert_eq!(p.build_body(&req)["messages"][0]["content"], "plain");
    }

    #[tokio::test]
    async fn falls_back_to_plain_mode_when_json_mode_validation_fails() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string(r#"{"error":{"code":"json_validate_failed"}}"#))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("{\"a\":1}")))
            .expect(1)
            .mount(&server)
            .await;
        let out = provider(&server.uri()).generate(request()).await.unwrap();
        assert_eq!(out.content, "{\"a\":1}");
        let sent = server.received_requests().await.unwrap();
        assert!(sent[0].body_json::<serde_json::Value>().unwrap().get("response_format").is_some());
        assert!(sent[1].body_json::<serde_json::Value>().unwrap().get("response_format").is_none());
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

    #[test]
    fn parses_retry_after_seconds() {
        use reqwest::header::HeaderValue;
        let hv = |s: &str| HeaderValue::from_str(s).unwrap();
        assert_eq!(parse_retry_after(Some(&hv("2"))), Some(Duration::from_millis(2250)));
        assert_eq!(parse_retry_after(Some(&hv("0.5"))), Some(Duration::from_millis(750)));
        assert_eq!(parse_retry_after(Some(&hv("soon"))), None);
        assert_eq!(parse_retry_after(Some(&hv("-3"))), None);
        assert_eq!(parse_retry_after(None), None);
    }

    #[tokio::test]
    async fn rate_limit_waits_for_the_retry_after_the_server_asked_for() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("ok")))
            .mount(&server)
            .await;
        let start = std::time::Instant::now();
        assert_eq!(provider(&server.uri()).generate(request()).await.unwrap().content, "ok");
        assert!(start.elapsed() >= Duration::from_secs(1), "backoff here would only be ~1ms");
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
