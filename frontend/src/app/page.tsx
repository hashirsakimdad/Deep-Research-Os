"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { ApiError, ResearchRun, api } from "@/lib/api";
import { Tag, formatTime } from "@/components/ui";

const EXAMPLES = [
  "What are the major technical limitations of autonomous AI agents?",
  "What are the technical, economic and security implications of autonomous AI coding agents over the next five years?",
];

export default function Home() {
  const router = useRouter();
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [runs, setRuns] = useState<ResearchRun[] | null>(null);

  useEffect(() => {
    api.list().then(setRuns).catch(() => setRuns([]));
  }, []);

  async function start(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const { research_id } = await api.start(query.trim());
      router.push(`/research/${research_id}`);
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : "Cannot reach the research backend. Is it running?",
      );
      setBusy(false);
    }
  }

  return (
    <main className="mx-auto w-full max-w-3xl flex-1 px-6 py-16">
      <p className="font-mono text-xs uppercase tracking-[0.3em] text-accent">
        Deep Research OS
      </p>
      <h1 className="mt-4 text-3xl font-semibold tracking-tight">
        What do you want to research?
      </h1>
      <p className="mt-3 max-w-xl text-sm leading-relaxed text-muted">
        A planner, concurrent researchers, an evidence verifier, a contradiction
        detector, a synthesizer and a critic work on your question. Every claim in
        the report traces back to a source.
      </p>

      <form onSubmit={start} className="mt-8">
        <textarea
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          rows={4}
          placeholder="Ask a difficult, open-ended research question…"
          className="w-full resize-none rounded-lg border border-line bg-panel p-4 text-[15px] outline-none placeholder:text-muted/60 focus:border-accent"
        />
        <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap gap-2">
            {EXAMPLES.slice(0, 1).map((ex) => (
              <button
                type="button"
                key={ex}
                onClick={() => setQuery(ex)}
                className="rounded border border-line px-2 py-1 text-left text-xs text-muted hover:text-foreground"
              >
                Try: {ex}
              </button>
            ))}
          </div>
          <button
            type="submit"
            disabled={busy || query.trim().length < 8}
            className="rounded-lg bg-accent px-5 py-2.5 font-mono text-xs font-semibold uppercase tracking-[0.18em] text-black transition-opacity disabled:cursor-not-allowed disabled:opacity-40"
          >
            {busy ? "Starting…" : "Start research"}
          </button>
        </div>
        {error && <p className="mt-3 text-sm text-bad">{error}</p>}
      </form>

      <section className="mt-16">
        <h2 className="mb-3 font-mono text-[11px] uppercase tracking-[0.14em] text-muted">
          Previous runs
        </h2>
        {runs === null ? (
          <p className="text-sm text-muted">Loading…</p>
        ) : runs.length === 0 ? (
          <p className="text-sm text-muted">No research runs yet.</p>
        ) : (
          <ul className="divide-y divide-line rounded-lg border border-line bg-panel">
            {runs.map((r) => (
              <li key={r.id}>
                <Link
                  href={`/research/${r.id}`}
                  className="flex items-center justify-between gap-4 px-4 py-3 hover:bg-panel2"
                >
                  <span className="min-w-0 truncate text-sm">{r.query}</span>
                  <span className="flex shrink-0 items-center gap-3 font-mono text-[11px] text-muted">
                    <Tag>{r.status}</Tag>
                    {formatTime(r.created_at)}
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}
