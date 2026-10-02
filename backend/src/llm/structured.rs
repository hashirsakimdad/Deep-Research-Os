//! Validated structured output: JSON in, typed Rust value out, with a bounded
//! repair loop when the model returns something unusable.

use serde::de::DeserializeOwned;

use super::provider::{LlmMessage, LlmProvider, LlmRequest};
use crate::{error::AgentError, util::text::extract_json};

/// Maximum extra round-trips spent repairing malformed output.
pub const MAX_REPAIRS: u32 = 2;

/// Calls the provider and deserializes the reply into `T`, then runs
/// `validate`. On a parse or validation failure the error is fed back to the
/// model (up to [`MAX_REPAIRS`] times) before giving up.
pub async fn generate_structured<T, V>(
    llm: &dyn LlmProvider,
    agent: &'static str,
    mut request: LlmRequest,
    validate: V,
) -> Result<T, AgentError>
where
    T: DeserializeOwned,
    V: Fn(&T) -> Result<(), String>,
{
    let mut last_error = String::new();
    for attempt in 0..=MAX_REPAIRS {
        let response = llm.generate(request.clone()).await.map_err(|source| AgentError::Llm { agent, source })?;

        let problem = match parse::<T>(&response.content) {
            Ok(value) => match validate(&value) {
                Ok(()) => return Ok(value),
                Err(reason) => format!("the JSON was well-formed but invalid: {reason}"),
            },
            Err(reason) => format!("the reply could not be parsed: {reason}"),
        };

        tracing::warn!(agent, attempt, %problem, "structured output rejected");
        last_error = problem.clone();
        request.messages.push(LlmMessage::assistant(response.content));
        request.messages.push(LlmMessage::user(format!(
            "Your previous reply was rejected: {problem}. Reply again with ONLY the corrected JSON object."
        )));
    }
    Err(AgentError::InvalidOutput { agent, reason: last_error })
}

fn parse<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    let json = extract_json(raw).ok_or("no JSON object found")?;
    serde_json::from_str(json).map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use crate::{
        error::LlmError,
        llm::provider::{LlmProvider, LlmRequest, LlmResponse},
    };

    /// Replays scripted replies in order and records the requests it saw.
    pub struct ScriptedLlm {
        replies: Mutex<Vec<Result<String, LlmError>>>,
        pub requests: Mutex<Vec<LlmRequest>>,
    }

    impl ScriptedLlm {
        pub fn new(replies: Vec<Result<String, LlmError>>) -> Self {
            let mut replies = replies;
            replies.reverse();
            Self { replies: Mutex::new(replies), requests: Mutex::new(vec![]) }
        }
        pub fn ok(replies: &[&str]) -> Self {
            Self::new(replies.iter().map(|r| Ok(r.to_string())).collect())
        }
        pub fn call_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl LlmProvider for ScriptedLlm {
        async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
            self.requests.lock().unwrap().push(request);
            let next = self.replies.lock().unwrap().pop();
            match next {
                Some(Ok(content)) => Ok(LlmResponse { content, model: "scripted".into(), usage: None }),
                Some(Err(e)) => Err(e),
                None => Err(LlmError::InvalidResponse("script exhausted".into())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::ScriptedLlm, *};
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Out {
        n: u32,
    }

    fn req() -> LlmRequest {
        LlmRequest::json("test", "sys", "user")
    }

    #[tokio::test]
    async fn parses_fenced_json() {
        let llm = ScriptedLlm::ok(&["```json\n{\"n\": 5}\n```"]);
        let out: Out = generate_structured(&llm, "t", req(), |_| Ok(())).await.unwrap();
        assert_eq!(out, Out { n: 5 });
        assert_eq!(llm.call_count(), 1);
    }

    #[tokio::test]
    async fn repairs_malformed_output() {
        let llm = ScriptedLlm::ok(&["not json", "{\"n\": 7}"]);
        let out: Out = generate_structured(&llm, "t", req(), |_| Ok(())).await.unwrap();
        assert_eq!(out.n, 7);
        let seen = llm.requests.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[1].messages.last().unwrap().content.contains("rejected"));
    }

    #[tokio::test]
    async fn repairs_validation_failures() {
        let llm = ScriptedLlm::ok(&["{\"n\": 0}", "{\"n\": 3}"]);
        let out: Out =
            generate_structured(
                &llm,
                "t",
                req(),
                |o: &Out| {
                    if o.n == 0 { Err("n must be positive".into()) } else { Ok(()) }
                },
            )
            .await
            .unwrap();
        assert_eq!(out.n, 3);
    }

    #[tokio::test]
    async fn gives_up_after_bounded_repairs() {
        let llm = ScriptedLlm::ok(&["bad", "bad", "bad", "{\"n\": 1}"]);
        let err = generate_structured::<Out, _>(&llm, "t", req(), |_| Ok(())).await.unwrap_err();
        assert!(matches!(err, AgentError::InvalidOutput { .. }));
        assert_eq!(llm.call_count(), 3);
    }
}
