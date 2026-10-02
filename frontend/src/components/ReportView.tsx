"use client";

import { useMemo } from "react";
import type { CitedStatement, ClaimRecord, Report } from "@/lib/api";
import { ClaimRow } from "./Explorers";
import { Empty, Panel, Tag, hostOf } from "./ui";

export interface TraceTarget {
  label: string;
  claimIds: string[];
}

export function ReportView({
  report,
  claims,
  onTrace,
}: {
  report: Report | null;
  claims: ClaimRecord[];
  onTrace: (t: TraceTarget) => void;
}) {
  const claimById = useMemo(() => new Map(claims.map((c) => [c.claim.id, c])), [claims]);
  const numberBySource = useMemo(
    () => new Map((report?.citations ?? []).map((c) => [c.source_id, c.number])),
    [report],
  );

  if (!report) {
    return (
      <Panel title="Report">
        <Empty>The report appears here once the synthesizer has written a draft.</Empty>
      </Panel>
    );
  }
  const c = report.content;

  const Statement = ({ s }: { s: CitedStatement }) => {
    // citation chips: one per distinct source, numbered by the backend
    const chips = new Map<number, string[]>();
    s.claim_ids.forEach((id) => {
      const rec = claimById.get(id);
      const n = rec && numberBySource.get(rec.source.id);
      if (n) chips.set(n, [...(chips.get(n) ?? []), id]);
    });
    return (
      <span>
        {s.text}{" "}
        {[...chips.entries()]
          .sort((a, b) => a[0] - b[0])
          .map(([n, ids]) => (
            <button
              key={n}
              onClick={() => onTrace({ label: `Citation [${n}]`, claimIds: ids })}
              className="mx-0.5 rounded border border-info/40 px-1 font-mono text-[11px] text-info hover:bg-info/10"
              title="Trace this citation to its claims, evidence and source"
            >
              {n}
            </button>
          ))}
        {s.claim_ids.length > 0 && (
          <button
            onClick={() => onTrace({ label: s.text, claimIds: s.claim_ids })}
            className="ml-1 font-mono text-[10px] uppercase tracking-wide text-muted hover:text-accent"
          >
            trace
          </button>
        )}
      </span>
    );
  };

  return (
    <Panel
      title="Report"
      right={
        <span className="flex items-center gap-2">
          <Tag>{report.is_final ? "final" : "draft"}</Tag>
          <Tag>iteration {report.iteration}</Tag>
        </span>
      }
    >
      <article className="space-y-6 text-[15px] leading-relaxed">
        <Section title="Executive summary">
          <p>{c.executive_summary}</p>
        </Section>
        <Section title="Methodology">
          <p>{c.methodology}</p>
        </Section>
        <Section title="Key findings">
          <ul className="list-disc space-y-2 pl-5">
            {c.key_findings.map((f, i) => (
              <li key={i}>
                <Statement s={f} />
              </li>
            ))}
          </ul>
        </Section>
        <Section title="Detailed findings">
          {c.detailed_findings.map((sec, i) => (
            <div key={i} className="mb-4">
              <h4 className="mb-1 font-medium text-foreground">{sec.heading}</h4>
              {sec.body.map((s, k) => (
                <p key={k} className="mb-2">
                  <Statement s={s} />
                </p>
              ))}
            </div>
          ))}
        </Section>
        <Section title="Contradictory evidence">
          {c.contradictory_evidence.length === 0 ? (
            <p className="text-muted">No genuine contradictions between sources were identified.</p>
          ) : (
            <ul className="list-disc space-y-2 pl-5">
              {c.contradictory_evidence.map((s, i) => (
                <li key={i}>
                  <Statement s={s} />
                </li>
              ))}
            </ul>
          )}
        </Section>
        <Section title="Limitations">
          <ul className="list-disc space-y-1 pl-5">
            {c.limitations.map((l, i) => (
              <li key={i}>{l}</li>
            ))}
          </ul>
        </Section>
        {c.open_questions.length > 0 && (
          <Section title="Open questions">
            <ul className="list-disc space-y-1 pl-5">
              {c.open_questions.map((l, i) => (
                <li key={i}>{l}</li>
              ))}
            </ul>
          </Section>
        )}
        <Section title="Conclusion">
          <p>{c.conclusion}</p>
        </Section>
        <Section title="Sources">
          <ol className="space-y-1.5 font-mono text-xs">
            {report.citations.map((s) => (
              <li key={s.source_id} className="flex gap-2">
                <span className="text-info">[{s.number}]</span>
                <span className="min-w-0">
                  {s.title}{" "}
                  <a
                    href={s.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="break-all text-info hover:underline"
                  >
                    {hostOf(s.url)}
                  </a>
                </span>
              </li>
            ))}
          </ol>
        </Section>
      </article>
    </Panel>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h3 className="mb-2 font-mono text-[11px] uppercase tracking-[0.14em] text-accent">
        {title}
      </h3>
      {children}
    </section>
  );
}

/** claim -> evidence -> source -> original URL for the selected statement. */
export function TracePanel({
  target,
  claims,
  onClose,
}: {
  target: TraceTarget | null;
  claims: ClaimRecord[];
  onClose: () => void;
}) {
  if (!target) return null;
  const records = target.claimIds
    .map((id) => claims.find((c) => c.claim.id === id))
    .filter((c): c is ClaimRecord => !!c);
  return (
    <Panel
      title="Provenance Trace"
      right={
        <button onClick={onClose} className="font-mono text-[11px] text-muted hover:text-foreground">
          close ✕
        </button>
      }
      className="border-accent/40"
    >
      <p className="mb-3 text-sm text-muted">
        <span className="font-mono text-[10px] uppercase">conclusion · </span>
        {target.label}
      </p>
      <ul className="space-y-2">
        {records.map((r) => (
          <ClaimRow key={r.claim.id} record={r} />
        ))}
      </ul>
      {records.length === 0 && <Empty>The claims for this statement are not loaded.</Empty>}
      <ul className="mt-3 space-y-1 font-mono text-[11px] text-muted">
        {[...new Map(records.map((r) => [r.source.id, r.source])).values()].map((s) => (
          <li key={s.id}>
            ↳ original source:{" "}
            <a
              href={s.url}
              target="_blank"
              rel="noopener noreferrer"
              className="break-all text-info hover:underline"
            >
              {s.url}
            </a>
          </li>
        ))}
      </ul>
    </Panel>
  );
}
