# Deployment (Vercel + a container host + hosted Postgres)

```
Browser ──> Vercel (Next.js frontend) ──> Backend container (Fly.io / Railway / Render) ──> Postgres (Neon / Supabase)
                                                   │
                                                   └──> LLM provider API, Tavily search
```

**Why the backend is not on Vercel:** it is an always-on server. Research runs last 10-15 minutes in the
background, stream live events over SSE and keep state in Postgres. Vercel's serverless functions are short-lived
and would cut runs off. Only the frontend belongs on Vercel.

> **Status:** the access control is tested (unit tests + a live check against the running server). The
> `Dockerfile` and the hosting steps below have **not** been built or run on a real platform yet, because the
> development machine could not run Docker. Expect to fix small things on the first deploy and treat the
> first deployment as a test.

## 1. Access control (do this before anything is public)

The API spends your LLM and search quota, so it must not be open to the internet.

* Set `API_ACCESS_KEY` on the backend (minimum 16 characters; the server refuses a shorter one). Generate one:

  ```bash
  openssl rand -hex 24          # or PowerShell:  -join ((1..48) | ForEach-Object { '{0:x}' -f (Get-Random -Max 16) })
  ```

* Every `/api/*` request must then send `x-api-key: <key>` (or `Authorization: Bearer <key>`). `/health` stays
  open so the platform can probe it. Without the key the API answers `401`.
* The frontend shows an **Access key** screen when the backend answers 401. The key is stored only in that
  browser's `localStorage` and sent as a header. It is **not** a `NEXT_PUBLIC_` variable on purpose: anything
  prefixed that way is baked into the public JavaScript and would be readable by everyone.
* Use HTTPS only (all three hosts provide it). The key travels in a header on every request.
* This is a single shared secret, not user accounts: fine for one person or a small team; rotate by changing
  the env var and redeploying. It does not give per-user history or revoke individuals.

Also keep spend bounded (defaults shown are conservative for a public instance):

```
MAX_CONCURRENT_RUNS=1        # research runs at once
MAX_ITERATIONS=2
MAX_TASKS_PER_ITERATION=6
MAX_CONCURRENT_LLM_CALLS=2
```

## 2. Database (Neon or Supabase)

Create a Postgres project and copy its connection string as `DATABASE_URL` (it must include `sslmode=require`).
The backend creates its tables on startup (migrations are embedded), so there is nothing to run by hand.
Use the *direct* (non-pooled) connection string if the provider offers both.

## 3. Backend container

Build context is `backend/`:

```bash
docker build -t deep-research-os-backend backend
```

Environment variables to set in the host's dashboard (never commit them):

| Variable | Value |
|----------|-------|
| `DATABASE_URL` | from step 2 |
| `GROK_API_KEY` | your LLM key |
| `GROK_BASE_URL`, `GROK_MODEL` | e.g. `https://api.x.ai/v1` + a Grok model, or an OpenAI-compatible provider (see [grok-integration](grok-integration.md)) |
| `SEARCH_API_KEY` | Tavily key |
| `API_ACCESS_KEY` | from step 1 |
| `CORS_ORIGIN` | your exact Vercel URL, e.g. `https://my-app.vercel.app` (no trailing slash) |
| `PORT` | usually set by the platform; the server listens on it |

Host notes:

* **Fly.io**: `fly launch --dockerfile backend/Dockerfile --no-deploy` from the repo root, then `fly secrets set ...`
  and `fly deploy`. Set the internal port to 8080 and the HTTP health check to `/health`. Keep one machine
  running (do not let it scale to zero mid-run): a run lives in that process's memory and is marked failed if
  the process restarts.
* **Railway / Render**: create a service from the repo with root `backend/` and the Dockerfile; set the env vars
  above; health check path `/health`.
* Run **one instance only**. Runs are driven by the process that started them; two instances would not share
  in-flight work.

## 4. Frontend on Vercel

1. Import the GitHub repo in Vercel; set **Root Directory** to `frontend`.
2. Add one environment variable: `NEXT_PUBLIC_API_URL` = the backend's public HTTPS URL
   (no trailing slash). This is just an address, not a secret.
3. Deploy, open the site, enter the access key when asked.

## 5. After deploying: verify

```bash
curl https://<backend>/health                                  # {"status":"ok"}
curl -i https://<backend>/api/research                         # 401 without the key
curl -i -H "x-api-key: <key>" https://<backend>/api/research   # 200 with it
```

Then start a short research from the site and watch the event feed. If the feed stays empty but the page
loads, check that `CORS_ORIGIN` exactly matches the Vercel URL and that your host does not buffer SSE responses.

## Limits to know about

* Runs do not resume after a backend restart (they are marked failed); redeploys interrupt active runs.
* The free tiers of LLM providers rate-limit heavily; see [grok-integration](grok-integration.md).
* No per-user accounts, quotas or billing: one shared key.
