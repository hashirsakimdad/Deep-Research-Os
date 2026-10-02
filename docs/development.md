# Development

## Prerequisites

Rust (stable), Docker (for PostgreSQL), Node.js 20+ (frontend). API keys: an xAI Grok key and a Tavily key.

## Run

```bash
cp .env.example .env                 # fill GROK_API_KEY, SEARCH_API_KEY
docker compose up -d postgres
cd backend && cargo run              # migrations run on startup; listens on :8080
cd frontend && npm install && npm run dev   # http://localhost:3000
```

## Checks

```bash
cd backend
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                                   # unit tests, no network or database needed
DATABASE_URL=postgres://research:research@localhost:5432/research cargo test --test pipeline
```

Unit tests mock every external dependency (LLM, search, fetch, HTTP via wiremock). The
`tests/pipeline.rs` integration tests drive the **entire** lifecycle (planning, concurrent research,
verification, contradiction analysis, synthesis, critique, iteration, failure handling) against a real
PostgreSQL with scripted providers. Without `DATABASE_URL` they print a notice and return early, so a green
run without the variable does **not** exercise them. No test calls the live Grok or search APIs.

## Windows notes

Builds with many parallel rustc jobs can exhaust memory on small machines; `CARGO_BUILD_JOBS=2` helps.
`CARGO_TARGET_DIR` and `CARGO_HOME` can be pointed at another drive when the system drive is small.

## Workflow

`main` stays stable; features land from branches such as `feat/backend-foundation`,
`feat/agent-orchestrator`, `feat/frontend` with conventional commit messages.
