# Deep Research OS

An autonomous, multi-agent deep research system. Give it a hard question; it plans, researches in parallel,
verifies evidence, detects contradictions, writes a cited report, critiques it and researches again if the
critique demands it. Every statement in the report traces back through **claim -> evidence -> source -> URL**.

* **Rust (Axum + Tokio + SQLx)** controls the workflow: state, scheduling, concurrency, retries, timeouts,
  persistence, limits, events.
* **Grok (xAI)** provides the reasoning inside six distinct agents, behind a provider-independent trait.
* **PostgreSQL** holds relational research memory.
* **Next.js + TypeScript** shows the run live, driven only by real backend events and counts.

> It is not a chatbot and not a single-prompt RAG app. The application uses the Grok API for all LLM
> reasoning and has no Claude/Anthropic dependency.

```mermaid
flowchart LR
    Q[Question] --> P[Planner] --> T{{Research tasks}}
    T --> R1[Researcher] & R2[Researcher] & R3[Researcher]
    R1 & R2 & R3 --> E[(Sources + claims + evidence)]
    E --> V[Verifier] --> C[Contradiction Detector] --> S[Synthesizer] --> K[Critic]
    K -- "gaps / needs evidence<br/>(max 2 iterations)" --> T
    K -- accepted --> F[Final report with citations]
```

## Status

Implemented and covered by tests (unit tests with mocked LLM, search, fetch and HTTP):

* Provider abstractions (`LlmProvider`, `SearchProvider`, `Fetcher`), Grok provider with bounded retries,
  structured-output validation and repair loop, Tavily search, HTML fetcher
* Planner, Researcher, Verifier, Contradiction Detector, Synthesizer, Critic with distinct prompts and
  validated, typed outputs
* Deterministic state machine, bounded-concurrency scheduler (dependencies, timeouts, cancellation, partial
  failure), explicit review criteria, iteration loop capped at `MAX_ITERATIONS` (2)
* Provenance guards: claims need a verbatim excerpt from the source; citations are built by Rust from stored
  sources, never from model output
* Relational schema, repository, Axum REST API, SSE with `Last-Event-ID` replay
* Next.js dashboard: progress, evidence ledger, workflow graph, live event feed, source/claim explorers,
  contradiction panel, critic panel, report with click-through provenance trace

What has **not** been exercised yet: a run against the live Grok and Tavily APIs (no keys were available to the
development environment), and `backend/tests/pipeline.rs` (full lifecycle + SQL against PostgreSQL) requires a
database; without `DATABASE_URL` it skips itself. See [docs/development.md](docs/development.md).

## Quick start

```bash
cp .env.example .env            # set GROK_API_KEY and SEARCH_API_KEY (Tavily)
docker compose up -d postgres
cd backend && cargo run         # http://localhost:8080 (applies migrations)
cd frontend && npm install && npm run dev   # http://localhost:3000
```

Run the database-backed lifecycle tests (no paid API calls; LLM/search are scripted):

```bash
cd backend
DATABASE_URL=postgres://research:research@localhost:5432/research cargo test --test pipeline
```

## Repository layout

```
backend/        Rust service (api, agents, orchestrator, llm, tools, database, models) + migrations
frontend/       Next.js dashboard
docs/           architecture, agents, lifecycle, database, API, Grok integration, ADRs
docker-compose.yml, .env.example
```

## Configuration

See [.env.example](.env.example). Secrets live only in `.env` (git-ignored). Cost and concurrency are bounded by
`MAX_ITERATIONS`, `MAX_TASKS_PER_ITERATION`, `MAX_CONCURRENT_TASKS`, `MAX_CONCURRENT_LLM_CALLS`,
`MAX_CONCURRENT_RUNS`, `SOURCES_PER_TASK` and timeouts.

## Documentation

[Architecture](docs/architecture.md) · [Agent system](docs/agent-system.md) ·
[Research lifecycle](docs/research-lifecycle.md) · [Database](docs/database.md) · [API](docs/api.md) ·
[Grok integration](docs/grok-integration.md) · [Development](docs/development.md) ·
[Architecture decisions](docs/architecture-decisions.md)

## Known limitations

* Source quality is judged by heuristics (host patterns) and the extractor's classification, not an external
  reputation database; the report communicates uncertainty but cannot guarantee source reliability.
* Claim deduplication is exact-match per source; semantically equal claims from different sources are kept
  (and compared by the contradiction detector).
* PDFs and JavaScript-rendered pages are not fetched (the search provider's extracted text is used when present).
* Runs do not resume after a server restart; they are marked failed.
* No authentication or rate limiting on the API (single-user V1).
