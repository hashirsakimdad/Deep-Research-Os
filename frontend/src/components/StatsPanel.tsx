import type { RunStats } from "@/lib/api";
import { Panel } from "./ui";

function Row({
  label,
  value,
  tone = "",
}: {
  label: string;
  value: number | string;
  tone?: string;
}) {
  return (
    <div className="flex items-baseline justify-between py-1 font-mono text-sm">
      <span className="text-muted">{label}</span>
      <span className={tone}>{value}</span>
    </div>
  );
}

/** Counts come straight from `GET /api/research/{id}` (computed in SQL). */
export function StatsPanel({ stats }: { stats: RunStats }) {
  return (
    <Panel title="Evidence Ledger">
      <Row label="research questions" value={stats.questions} />
      <Row
        label="tasks done / failed"
        value={`${stats.tasks_completed} / ${stats.tasks_failed}`}
        tone={stats.tasks_failed > 0 ? "text-warn" : ""}
      />
      <Row label="sources" value={stats.sources} />
      <Row label="claims extracted" value={stats.claims} />
      <div className="my-2 border-t border-line" />
      <Row label="supported" value={stats.supported} tone="text-ok" />
      <Row label="partially supported" value={stats.partially_supported} tone="text-warn" />
      <Row label="unverified" value={stats.unverified} tone="text-muted" />
      <Row label="unsupported" value={stats.unsupported} tone="text-bad" />
      <Row label="contradicted" value={stats.contradicted} tone="text-bad" />
      <div className="my-2 border-t border-line" />
      <Row label="disagreements found" value={stats.contradictions} />
      <Row
        label="genuine contradictions"
        value={stats.genuine_contradictions}
        tone={stats.genuine_contradictions > 0 ? "text-bad" : ""}
      />
    </Panel>
  );
}
