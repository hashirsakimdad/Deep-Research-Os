import type { AgentEvent, ResearchRun, RunStats } from "@/lib/api";
import { deriveStages, type StageState } from "@/lib/stages";
import { Panel } from "./ui";

const GLYPH: Record<StageState, string> = {
  done: "✓",
  active: "●",
  pending: "○",
  failed: "✕",
};

const STYLE: Record<StageState, string> = {
  done: "text-ok",
  active: "text-accent live-pulse",
  pending: "text-muted/60",
  failed: "text-bad",
};

export function ProgressTracker({
  run,
  stats,
  events,
}: {
  run: ResearchRun;
  stats: RunStats;
  events: AgentEvent[];
}) {
  const stages = deriveStages(run, stats, events);
  return (
    <Panel
      title="Research Progress"
      right={
        <span className="font-mono text-[11px] text-muted">
          iteration {run.iteration}
        </span>
      }
    >
      <ol className="space-y-2.5">
        {stages.map((s) => (
          <li key={s.key} className="flex items-center gap-3 font-mono text-sm">
            <span className={`w-4 text-center ${STYLE[s.state]}`}>
              {GLYPH[s.state]}
            </span>
            <span className={s.state === "pending" ? "text-muted/70" : ""}>
              {s.label}
            </span>
          </li>
        ))}
      </ol>
      {run.status === "failed" && run.error && (
        <p className="mt-4 rounded border border-bad/30 bg-bad/5 p-2.5 text-xs text-bad">
          {run.error}
        </p>
      )}
    </Panel>
  );
}
