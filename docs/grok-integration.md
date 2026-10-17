# Grok integration

Grok (xAI) is the only LLM the application uses. It is isolated behind `llm::LlmProvider`:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
}
```

`GrokProvider` (`backend/src/llm/grok.rs`) owns authentication, request construction, response parsing,
timeouts and retries. It calls xAI's OpenAI-compatible `POST {GROK_BASE_URL}/chat/completions` with a bearer
token and `response_format: {"type": "json_object"}` for structured replies.

## Configuration

| Variable | Default | Meaning |
|----------|---------|---------|
| `GROK_API_KEY` | required | xAI API key. Never committed; `.env` is git-ignored |
| `GROK_MODEL` | `grok-4` | model id; change it without code changes |
| `GROK_BASE_URL` | `https://api.x.ai/v1` | override for proxies or tests |
| `LLM_TIMEOUT_SECS` | 120 | per-attempt request timeout |
| `MAX_CONCURRENT_LLM_CALLS` | 4 | process-wide in-flight request cap (`LimitedProvider`) |

## Retries

Up to 3 attempts with exponential backoff (500 ms, 1 s, ... capped at 8 s). Retried: timeouts, network
errors, HTTP 429 and 5xx. Not retried: other 4xx (e.g. bad key). Verified by wiremock tests.

## Structured output

`llm::generate_structured` asks for JSON, extracts the outermost object (tolerating code fences or prose),
deserializes into the agent's Rust type with Serde, and runs a validation callback. On failure the exact
problem is sent back to the model and the call is repeated, at most twice, before the agent fails with
`AgentError::InvalidOutput`. Agents therefore never parse strings by hand.

## Per-agent prompts

Each agent has its own system prompt in its own file (`planner.rs`, `researcher.rs`, `verifier.rs`,
`contradiction.rs`, `synthesizer.rs`, `critic.rs`); there is no shared mega-prompt. Prompts instruct the
model to return only JSON and spell out the rules the Rust validators enforce.

## Search

Search is abstracted by `tools::SearchProvider`. V1 ships a Tavily implementation (`SEARCH_API_KEY`); the
researcher also fetches pages itself (`tools::Fetcher`) when the search result carries no content.

## Using another OpenAI-compatible provider (e.g. Groq)

The provider speaks the OpenAI chat-completions protocol, so any compatible endpoint works by changing
`.env` (no code change). Verified with Groq:

```
GROK_BASE_URL=https://api.groq.com/openai/v1
GROK_MODEL=openai/gpt-oss-120b
GROK_API_KEY=gsk_...
```

Notes learned from real runs on a rate-limited free tier:

* Lower the load: `MAX_CONCURRENT_LLM_CALLS=2`, `MAX_CONCURRENT_TASKS=2`, `REPORT_MAX_CLAIMS=14`
  (some tiers cap a single request at 8,000 tokens; an oversized prompt is rejected with 413 and cannot be retried).
* `429` responses are retried after the server's `Retry-After`; other transient errors use exponential backoff.
* If the server rejects JSON mode on a long reply (`json_validate_failed`), the request is retried once in plain
  mode and the structured-output layer extracts and repairs the JSON.
