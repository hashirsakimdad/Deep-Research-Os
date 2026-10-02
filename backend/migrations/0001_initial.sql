-- Deep Research OS: relational research memory.
-- Everything a final conclusion rests on is traceable:
--   report -> claim -> evidence -> source -> original url

CREATE TABLE research_runs (
    id            UUID PRIMARY KEY,
    query         TEXT        NOT NULL,
    status        TEXT        NOT NULL,
    iteration     INTEGER     NOT NULL DEFAULT 1,
    error         TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at  TIMESTAMPTZ
);

CREATE TABLE research_questions (
    id               UUID PRIMARY KEY,
    research_run_id  UUID    NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
    question         TEXT    NOT NULL,
    objective        TEXT    NOT NULL,
    priority         TEXT    NOT NULL,
    position         INTEGER NOT NULL
);
CREATE INDEX research_questions_run_idx ON research_questions(research_run_id);

CREATE TABLE research_tasks (
    id                UUID PRIMARY KEY,
    research_run_id   UUID        NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
    question_id       UUID        REFERENCES research_questions(id) ON DELETE SET NULL,
    question          TEXT        NOT NULL,
    objective         TEXT        NOT NULL,
    priority          TEXT        NOT NULL,
    status            TEXT        NOT NULL,
    assigned_agent    TEXT        NOT NULL,
    required_evidence JSONB       NOT NULL DEFAULT '[]',
    dependencies      JSONB       NOT NULL DEFAULT '[]',
    iteration         INTEGER     NOT NULL DEFAULT 1,
    error             TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at      TIMESTAMPTZ
);
CREATE INDEX research_tasks_run_idx ON research_tasks(research_run_id);

CREATE TABLE sources (
    id               UUID PRIMARY KEY,
    research_run_id  UUID        NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
    url              TEXT        NOT NULL,
    normalized_url   TEXT        NOT NULL,
    title            TEXT        NOT NULL,
    publisher        TEXT,
    published_at     TEXT,
    retrieved_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    content          TEXT        NOT NULL,
    content_hash     TEXT        NOT NULL,
    source_type      TEXT        NOT NULL,
    nature           TEXT        NOT NULL DEFAULT 'unknown',
    -- one row per page per run: deduplicates concurrent tasks finding the same page
    UNIQUE (research_run_id, normalized_url)
);
CREATE INDEX sources_hash_idx ON sources(research_run_id, content_hash);

CREATE TABLE claims (
    id                UUID PRIMARY KEY,
    research_run_id   UUID        NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
    research_task_id  UUID        NOT NULL REFERENCES research_tasks(id) ON DELETE CASCADE,
    source_id         UUID        NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    text              TEXT        NOT NULL,
    confidence        REAL        NOT NULL,
    status            TEXT        NOT NULL DEFAULT 'UNVERIFIED',
    iteration         INTEGER     NOT NULL DEFAULT 1,
    dedupe_key        TEXT        NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- the same statement from the same page is stored once
    UNIQUE (research_run_id, source_id, dedupe_key)
);
CREATE INDEX claims_run_idx ON claims(research_run_id);
CREATE INDEX claims_task_idx ON claims(research_task_id);

CREATE TABLE evidence (
    id            UUID PRIMARY KEY,
    claim_id      UUID        NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
    source_id     UUID        NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    excerpt       TEXT        NOT NULL,
    relevance     REAL        NOT NULL,
    support_level TEXT        NOT NULL DEFAULT 'UNVERIFIED',
    reasoning     TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX evidence_claim_idx ON evidence(claim_id);

CREATE TABLE contradictions (
    id               UUID PRIMARY KEY,
    research_run_id  UUID        NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
    claim_a_id       UUID        NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
    claim_b_id       UUID        NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
    kind             TEXT        NOT NULL,
    explanation      TEXT        NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (claim_a_id, claim_b_id)
);
CREATE INDEX contradictions_run_idx ON contradictions(research_run_id);

CREATE TABLE reports (
    id               UUID PRIMARY KEY,
    research_run_id  UUID        NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
    iteration        INTEGER     NOT NULL,
    is_final         BOOLEAN     NOT NULL DEFAULT FALSE,
    content          JSONB       NOT NULL,
    markdown         TEXT        NOT NULL,
    citations        JSONB       NOT NULL DEFAULT '[]',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX reports_run_idx ON reports(research_run_id);

CREATE TABLE critiques (
    id                    UUID PRIMARY KEY,
    research_run_id       UUID        NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
    report_id             UUID        NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    iteration             INTEGER     NOT NULL,
    quality               REAL        NOT NULL,
    needs_more_research   BOOLEAN     NOT NULL,
    issues                JSONB       NOT NULL DEFAULT '[]',
    required_research     JSONB       NOT NULL DEFAULT '[]',
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX critiques_run_idx ON critiques(research_run_id);

-- bigserial gives events a total order that SSE clients can resume from.
CREATE TABLE agent_events (
    id           BIGSERIAL PRIMARY KEY,
    research_id  UUID        NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
    agent        TEXT        NOT NULL,
    event_type   TEXT        NOT NULL,
    message      TEXT        NOT NULL,
    metadata     JSONB       NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX agent_events_run_idx ON agent_events(research_id, id);
