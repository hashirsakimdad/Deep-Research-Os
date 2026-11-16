<!-- Deep Research OS: autonomous multi-agent AI research engine (Rust, Grok/xAI, PostgreSQL, Next.js) that writes fully cited reports. -->

<p align="center">
  <img src="./assets/readme/hero.png" width="100%" alt="Deep Research OS: autonomous multi-agent AI deep research engine where every claim traces to evidence, source and URL">
</p>

<h1 align="center">Deep Research OS</h1>

<p align="center">
  <b>Open-source autonomous AI deep research agent, built in Rust.</b><br>
  You ask a hard question. It plans, searches the web in parallel, checks the evidence, finds contradictions and writes a cited report. Then it critiques that report and goes back to research if the critique finds gaps.
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-Axum%20%2B%20Tokio-0a0c10?style=for-the-badge&logo=rust&logoColor=4fd1c5">
  <img alt="Grok xAI" src="https://img.shields.io/badge/LLM-Grok%20(xAI)-0a0c10?style=for-the-badge&logo=x&logoColor=4fd1c5">
  <img alt="PostgreSQL" src="https://img.shields.io/badge/PostgreSQL-16-0a0c10?style=for-the-badge&logo=postgresql&logoColor=4fd1c5">
  <img alt="Next.js" src="https://img.shields.io/badge/Next.js-TypeScript-0a0c10?style=for-the-badge&logo=nextdotjs&logoColor=4fd1c5">
  <img alt="Docker" src="https://img.shields.io/badge/Docker-ready-0a0c10?style=for-the-badge&logo=docker&logoColor=4fd1c5">
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#why-you-can-trust-the-report">Provenance</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#documentation">Docs</a>
</p>

<p align="center">
  <a href="https://github.com/hashirsakimdad/Deep-Research-Os/stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/hashirsakimdad/Deep-Research-Os?style=social"></a>
</p>

---

## Proof first: one real run

<p align="center">
  <img src="./assets/readme/proof.png" width="100%" alt="Real end-to-end run: 29 sources, 31 verified claims, 2 contradictions found, 8 of 8 citations audited">
</p>

A full run against live Tavily search and an OpenAI-compatible LLM (Groq, `openai/gpt-oss-120b`, free tier) finished in two iterations: 6 questions, 10 tasks, 29 sources, 31 verified claims (24 supported, 7 partially supported) and 2 disagreements. The report had 6 key findings, 6 sections, 7 limitations and 8 citations.

A database audit confirmed that every citation URL is a stored source, every cited claim exists, and no key finding is uncited. The critic noticed the report under-discussed a contradiction, and the system wrote that weakness into the limitations section instead of hiding it.

<details>
<summary><b>Bugs this real run caught that mocks could not</b></summary>

- JSON mode rejected without the word "json" in the prompt
- Strict JSON validation failing on long replies
- Rate limits (now honors `Retry-After`)
- Prompts exceeding per-request token limits (`REPORT_MAX_CLAIMS`)
- Failures cascading through task dependency chains

</details>

## What it is

Deep Research OS is a **multi-agent research system**, not a chatbot and not a one-prompt RAG app.

| Layer | Job |
|---|---|
| **Rust** (Axum, Tokio, SQLx) | Owns the workflow: state, scheduling, concurrency, retries, timeouts, persistence, limits, events |
| **Grok (xAI)** | Reasoning inside six agents, behind a provider-independent `LlmProvider` trait |
| **PostgreSQL** | Relational research memory: questions, tasks, sources, claims, evidence, reports |
| **Next.js + TypeScript** | Live dashboard driven only by real backend events and counts |

The LLM thinks. Rust decides. A model never gets to invent a citation.

## How it works

<p align="center">
  <img src="./assets/readme/how-it-works.gif" width="100%" alt="Animated flowchart: a question moves from Planner to three parallel Researchers, into Postgres evidence, through Verifier, Contradiction Detector, Synthesizer and Critic, loops back once for more research, then becomes a cited report">
</p>

<details>
<summary><b>Static flowchart (Mermaid)</b></summary>

```mermaid
flowchart LR
    Q[Question] --> P[Planner] --> T{{Research tasks}}
    T --> R1[Researcher] & R2[Researcher] & R3[Researcher]
    R1 & R2 & R3 --> E[(Sources + claims + evidence)]
    E --> V[Verifier] --> C[Contradiction Detector] --> S[Synthesizer] --> K[Critic]
    K -- "gaps / needs evidence<br/>(max 2 iterations)" --> T
    K -- accepted --> F[Final report with citations]
```

</details>

<details>
<summary><b>The six agents</b></summary>

| Agent | What it does |
|---|---|
| Planner | Splits the question into sub-questions and dependent research tasks |
| Researcher | Searches, fetches pages, extracts claims with verbatim excerpts (runs in parallel) |
| Verifier | Grades each claim: supported, partially supported, unverified |
| Contradiction Detector | Compares claims across sources and flags disagreements |
| Synthesizer | Drafts the report from citable claims only |
| Critic | Reviews the draft; can send the run back for another iteration |

More in [docs/agent-system.md](docs/agent-system.md).
</details>

<details>
<summary><b>Run lifecycle (state machine)</b></summary>

`Planning → Researching → Verifying → Analyzing → Synthesizing → Critiquing → Completed`

Critiquing can loop back to Researching while `iteration < MAX_ITERATIONS` (default 2). Any phase can move to `Failed`. Transitions are enforced by `orchestrator::state::ResearchState`. Details in [docs/research-lifecycle.md](docs/research-lifecycle.md).
</details>

## Why you can trust the report

- Claims must carry a **verbatim excerpt** from the source, or they are dropped.
- Citations are **built by Rust from stored sources**, never copied from model output.
- `UNVERIFIED` claims are excluded from the report by design.
- In the dashboard, click any sentence to trace **claim → evidence → source → URL**.

## Quick start

```bash
cp .env.example .env            # set GROK_API_KEY and SEARCH_API_KEY (Tavily)
docker compose up -d postgres
cd backend && cargo run         # http://localhost:8080 (applies migrations)
cd frontend && npm install && npm run dev   # http://localhost:3000
```

<details>
<summary><b>Windows: one command</b></summary>

```powershell
./scripts/start.ps1        # database + backend + frontend, then opens http://localhost:3001
```

Uses frontend `:3001` and backend `:8081` so it doesn't clash with other apps (`-FrontendPort` / `-BackendPort` to change). Needs `.env` with `GROK_API_KEY` and `SEARCH_API_KEY`.
</details>

<details>
<summary><b>Run the lifecycle tests (no paid API calls)</b></summary>

```bash
cd backend
DATABASE_URL=postgres://research:research@localhost:5432/research cargo test --test pipeline
```

LLM, search and fetch are scripted. Covers the happy path, the iteration loop, partial and total task failure, and deduplication. Without `DATABASE_URL` the test skips itself.
</details>

## The dashboard

Progress tracker, evidence ledger, workflow graph, live event feed (SSE with `Last-Event-ID` replay), source and claim explorers, contradiction panel, critic panel, and a report view with click-through provenance.

## Configuration

See [.env.example](.env.example). Secrets live only in `.env` (git-ignored).

<details>
<summary><b>Cost and concurrency limits</b></summary>

| Variable | Default |
|---|---|
| `MAX_ITERATIONS` | 2 |
| `MAX_TASKS_PER_ITERATION` | 6 |
| `MAX_CONCURRENT_TASKS` | 3 |
| `MAX_CONCURRENT_LLM_CALLS` | 4 |
| `MAX_CONCURRENT_RUNS` | 2 |
| `SOURCES_PER_TASK` | 4 |
| `TASK_TIMEOUT_SECS` / `LLM_TIMEOUT_SECS` / `HTTP_TIMEOUT_SECS` | 240 / 120 / 20 |
| `REPORT_MAX_CLAIMS` | 40 |

To use another OpenAI-compatible provider, set `GROK_BASE_URL` and `GROK_MODEL`.
</details>

## Deploying

Vercel for the frontend, any container host for the backend (`backend/Dockerfile`), hosted Postgres, protected by a shared `API_ACCESS_KEY`. Guide: [docs/deployment.md](docs/deployment.md).

## Status

<details>
<summary><b>Implemented and tested</b></summary>

- Provider abstractions (`LlmProvider`, `SearchProvider`, `Fetcher`), Grok provider with bounded retries, structured-output validation and a repair loop, Tavily search, HTML fetcher
- Six agents with distinct prompts and validated, typed outputs
- Deterministic state machine, bounded-concurrency scheduler (dependencies, timeouts, cancellation, partial failure), explicit review criteria
- Relational schema, repository, Axum REST API, SSE with replay
- Verified against real PostgreSQL 16: migrations, all SQL, and `backend/tests/pipeline.rs`
- Verified in the browser: a run rejected by the xAI API (bad key) fails cleanly and shows as failed

</details>

<details>
<summary><b>Not yet verified</b></summary>

- Runs on xAI's own Grok models. The code defaults to xAI; the real run used Groq because that key was available.
- Report quality is checked structurally (provenance, citations), not by an independent domain expert.
- On a rate-limited free tier some claims stay `UNVERIFIED` and are left out of the report.

</details>

<details>
<summary><b>Known limitations</b></summary>

- Source quality is judged by heuristics and the extractor's classification, not an external reputation database.
- Claim dedup is exact-match per source; equal claims from different sources are kept and compared.
- PDFs and JavaScript-rendered pages are not fetched (the search provider's text is used when present).
- Runs don't resume after a server restart; they are marked failed.
- Access control is one shared key. No per-user accounts, quotas or billing.

</details>

## Repository layout

```
backend/     Rust service: api, agents, orchestrator, llm, tools, database, models + migrations
frontend/    Next.js dashboard
docs/        architecture, agents, lifecycle, database, API, Grok integration, ADRs
assets/      README images
```

## Documentation

[Architecture](docs/architecture.md) · [Agent system](docs/agent-system.md) · [Research lifecycle](docs/research-lifecycle.md) · [Database](docs/database.md) · [API](docs/api.md) · [Grok integration](docs/grok-integration.md) · [Development](docs/development.md) · [Architecture decisions](docs/architecture-decisions.md) · [Deployment](docs/deployment.md)

---

<p align="center">
  If this is useful to you, a star helps other people find it.<br>
  Built by <a href="https://github.com/hashirsakimdad">Hashir Sakimdad</a>
</p>
