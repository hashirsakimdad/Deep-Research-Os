"use client";

import { useMemo, useState } from "react";
import type {
  ClaimRecord,
  Contradiction,
  Critique,
  SourceRef,
  SupportLevel,
} from "@/lib/api";
import { Empty, Panel, SupportBadge, Tag, hostOf } from "./ui";

export function SourceExplorer({
  sources,
  claims,
}: {
  sources: SourceRef[];
  claims: ClaimRecord[];
}) {
  const perSource = useMemo(() => {
    const m = new Map<string, number>();
    claims.forEach((c) => m.set(c.source.id, (m.get(c.source.id) ?? 0) + 1));
    return m;
  }, [claims]);

  return (
    <Panel title="Source Explorer" right={<Tag>{sources.length}</Tag>}>
      {sources.length === 0 ? (
        <Empty>No sources collected yet.</Empty>
      ) : (
        <ul className="divide-y divide-line">
          {sources.map((s) => (
            <li key={s.id} className="py-2.5">
              <a
                href={s.url}
                target="_blank"
                rel="noopener noreferrer"
                className="text-sm text-info hover:underline"
              >
                {s.title}
              </a>
              <div className="mt-1 flex flex-wrap items-center gap-2 font-mono text-[11px] text-muted">
                <span>{hostOf(s.url)}</span>
                <Tag>{s.source_type.replace("_", " ")}</Tag>
                <Tag>{s.nature}</Tag>
                {s.published_at && <span>{s.published_at}</span>}
                <span>{perSource.get(s.id) ?? 0} claims</span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}

const FILTERS: (SupportLevel | "ALL")[] = [
  "ALL",
  "SUPPORTED",
  "PARTIALLY_SUPPORTED",
  "UNVERIFIED",
  "UNSUPPORTED",
  "CONTRADICTED",
];

export function ClaimExplorer({
  claims,
  highlight,
}: {
  claims: ClaimRecord[];
  highlight?: Set<string>;
}) {
  const [filter, setFilter] = useState<SupportLevel | "ALL">("ALL");
  const shown = claims.filter((c) => filter === "ALL" || c.claim.status === filter);

  return (
    <Panel title="Claim Explorer" right={<Tag>{claims.length}</Tag>}>
      <div className="mb-3 flex flex-wrap gap-1.5">
        {FILTERS.map((f) => (
          <button
            key={f}
            onClick={() => setFilter(f)}
            className={`rounded border px-2 py-0.5 font-mono text-[10px] uppercase tracking-wide ${
              filter === f
                ? "border-accent text-accent"
                : "border-line text-muted hover:text-foreground"
            }`}
          >
            {f.replace("_", " ")}
          </button>
        ))}
      </div>
      {shown.length === 0 ? (
        <Empty>No claims in this view.</Empty>
      ) : (
        <ul className="space-y-2">
          {shown.map((c) => (
            <ClaimRow key={c.claim.id} record={c} highlighted={highlight?.has(c.claim.id)} />
          ))}
        </ul>
      )}
    </Panel>
  );
}

export function ClaimRow({
  record: c,
  highlighted,
}: {
  record: ClaimRecord;
  highlighted?: boolean;
}) {
  const ev = c.evidence[0];
  return (
    <li
      id={`claim-${c.claim.id}`}
      className={`rounded border p-3 ${highlighted ? "border-accent/60 bg-accent/5" : "border-line bg-panel2"}`}
    >
      <div className="flex items-start justify-between gap-3">
        <p className="text-sm">{c.claim.text}</p>
        <SupportBadge level={c.claim.status} />
      </div>
      {ev && (
        <blockquote className="mt-2 border-l-2 border-line pl-3 text-xs italic text-muted">
          “{ev.excerpt}”
        </blockquote>
      )}
      {ev?.reasoning && (
        <p className="mt-2 text-xs text-foreground/80">
          <span className="font-mono text-[10px] uppercase text-muted">verifier · </span>
          {ev.reasoning}
        </p>
      )}
      <div className="mt-2 flex flex-wrap items-center gap-2 font-mono text-[11px] text-muted">
        <a
          href={c.source.url}
          target="_blank"
          rel="noopener noreferrer"
          className="text-info hover:underline"
        >
          {hostOf(c.source.url)}
        </a>
        <Tag>{c.source.nature}</Tag>
        <span>confidence {c.claim.confidence.toFixed(2)}</span>
        <span className="truncate">Q: {c.question}</span>
      </div>
    </li>
  );
}

const KIND_TONE: Record<string, string> = {
  DIRECT_CONTRADICTION: "text-bad border-bad/40",
};

export function ContradictionPanel({
  contradictions,
  claims,
}: {
  contradictions: Contradiction[];
  claims: ClaimRecord[];
}) {
  const byId = useMemo(() => new Map(claims.map((c) => [c.claim.id, c])), [claims]);
  return (
    <Panel title="Contradiction Panel" right={<Tag>{contradictions.length}</Tag>}>
      {contradictions.length === 0 ? (
        <Empty>No disagreements between sources recorded.</Empty>
      ) : (
        <ul className="space-y-3">
          {contradictions.map((c) => {
            const a = byId.get(c.claim_a_id);
            const b = byId.get(c.claim_b_id);
            return (
              <li key={c.id} className="rounded border border-line bg-panel2 p-3">
                <span
                  className={`rounded border px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide ${KIND_TONE[c.kind] ?? "border-warn/40 text-warn"}`}
                >
                  {c.kind.replaceAll("_", " ").toLowerCase()}
                </span>
                <p className="mt-2 text-sm">{c.explanation}</p>
                <div className="mt-2 grid gap-2 md:grid-cols-2">
                  {[a, b].map((r, i) => (
                    <div key={i} className="rounded border border-line p-2 text-xs">
                      <p>{r?.claim.text ?? "(claim not loaded)"}</p>
                      {r && (
                        <a
                          href={r.source.url}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="mt-1 inline-block font-mono text-[11px] text-info hover:underline"
                        >
                          {hostOf(r.source.url)}
                        </a>
                      )}
                    </div>
                  ))}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </Panel>
  );
}

export function CritiquePanel({ critiques }: { critiques: Critique[] }) {
  return (
    <Panel title="Critic" right={<Tag>{critiques.length}</Tag>}>
      {critiques.length === 0 ? (
        <Empty>No critique yet.</Empty>
      ) : (
        <ul className="space-y-4">
          {critiques.map((c) => (
            <li key={c.id}>
              <div className="flex items-center gap-3 font-mono text-xs text-muted">
                <span>iteration {c.iteration}</span>
                <span>quality {c.quality.toFixed(2)}</span>
                {c.needs_more_research && <Tag>more research requested</Tag>}
              </div>
              <ul className="mt-2 space-y-1.5">
                {c.issues.map((i, k) => (
                  <li key={k} className="text-sm">
                    <span
                      className={`mr-2 font-mono text-[10px] uppercase ${
                        i.severity === "high"
                          ? "text-bad"
                          : i.severity === "medium"
                            ? "text-warn"
                            : "text-muted"
                      }`}
                    >
                      {i.type.replaceAll("_", " ").toLowerCase()}
                    </span>
                    {i.description}
                  </li>
                ))}
                {c.issues.length === 0 && (
                  <li className="text-sm text-muted">No issues raised.</li>
                )}
              </ul>
              {c.required_research.length > 0 && (
                <p className="mt-2 text-xs text-muted">
                  Requested research: {c.required_research.join("; ")}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}
