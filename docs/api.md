# HTTP API

Base URL `http://localhost:8080`. JSON everywhere. No authentication in V1.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | liveness |
| POST | `/api/research` | start a run; body `{"query": "..."}` (8-2000 chars) |
| GET | `/api/research` | the 50 most recent runs |
| GET | `/api/research/{id}` | run, questions, tasks and live counts (`stats`) |
| GET | `/api/research/{id}/events?after=<id>&limit=` | persisted events in order |
| GET | `/api/research/{id}/sources` | sources (metadata; content is not returned) |
| GET | `/api/research/{id}/claims` | claims with evidence, verifier reasoning, source and question |
| GET | `/api/research/{id}/contradictions` | classified claim pairs |
| GET | `/api/research/{id}/critiques` | critiques per iteration |
| GET | `/api/research/{id}/report` | final report if present, otherwise newest draft |
| GET | `/api/research/{id}/stream` | Server-Sent Events |

## Start a run

```http
POST /api/research
{"query": "What are the major technical limitations of autonomous AI agents?"}
```

```http
202 Accepted
{"research_id": "6f0c...", "status": "planning"}
```

The research continues in the background; follow it with `/stream` or poll the other endpoints.

## Stream

Each SSE message has `id:` set to the event id and a JSON `data:` payload:

```json
{"id": 42, "research_id": "...", "agent": "verifier", "event_type": "verification_completed",
 "message": "12 supported, 3 partially supported, ...", "metadata": {"supported": 12}, "timestamp": "..."}
```

Reconnect with the `Last-Event-ID` header (or `?after=`) to replay anything missed from the database. The
stream subscribes to live events before replaying and filters by id, so there is neither a gap nor duplicates.
A client that falls far behind may lose live messages and should reconnect.

## Report

```json
{"id": "...", "iteration": 2, "is_final": true,
 "content": {"executive_summary": "...", "key_findings": [{"text": "...", "claim_ids": ["..."]}], "...": "..."},
 "markdown": "# Research Report ...",
 "citations": [{"number": 1, "source_id": "...", "url": "https://...", "title": "..."}]}
```

## Errors

`{"error": "..."}` with `400` (bad query), `404` (unknown run / no report yet) or `500`.
