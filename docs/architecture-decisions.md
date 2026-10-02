# Architecture decisions

## ADR-1: Rust orchestrates, the model reasons
The workflow, concurrency, retries, timeouts, limits and persistence are ordinary deterministic Rust. Models
cannot extend a run, skip a phase or spawn work. Consequence: cost and failure modes are bounded and testable.

## ADR-2: Provider abstractions for LLM and search
`LlmProvider`, `SearchProvider` and `Fetcher` are traits. Agents depend only on them, which makes every agent
testable with scripted providers and lets Grok or Tavily be swapped without touching agents.

## ADR-3: The model names claims; Rust writes citations
The synthesizer returns `claim_ids`. Rust rejects ids it did not supply, then derives citation numbers and URLs
from stored sources. A fabricated URL cannot reach a report.

## ADR-4: Verbatim-excerpt provenance gate
A claim is accepted only if its quoted excerpt is found in the fetched page text. This is cheap, deterministic
protection against hallucinated evidence. Limitation: it proves the quote exists, not that the claim follows
from it; that is the verifier's job.

## ADR-5: Conservative verification with deterministic caps
Missing or unparsable verdicts become `UNVERIFIED`. Confidence is clamped by support level and by source
nature (marketing/opinion <= 0.5). A model cannot talk its way to high confidence on weak evidence.

## ADR-6: Critic score is advisory
Iteration is decided by explicit rules (`orchestrator::review`): coverage gaps, high-severity issues, quality
threshold and remaining budget. Gaps are computed from data, not opinion.

## ADR-7: Runtime-checked SQL
SQLx runtime queries instead of compile-time macros, so the project builds and unit-tests without a database.
Trade-off: SQL mistakes surface at run time, which the Postgres integration tests exist to catch.

## ADR-8: Persist events, then broadcast
Events are written to PostgreSQL before they reach a broadcast channel. SSE replays from the database using
event ids, so the UI is consistent after reconnects and shows only real operations.

## ADR-9: Runs do not resume after a restart
A restart marks non-terminal runs as failed. Resuming mid-run would need idempotent phase checkpoints; for V1 a
clear failure is preferable to an ambiguous state.

## ADR-10: Tavily as the V1 search provider
Chosen for a simple JSON API that can return page text. It sits behind `SearchProvider`; a different provider is
one new implementation.
