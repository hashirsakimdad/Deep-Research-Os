import type { AgentEvent, Overview } from "@/lib/api";
import { deriveStages, type StageState } from "@/lib/stages";
import { Panel } from "./ui";

const COLOR: Record<StageState, string> = {
  done: "var(--ok)",
  active: "var(--accent)",
  pending: "var(--line)",
  failed: "var(--bad)",
};

const TASK_COLOR = {
  completed: "var(--ok)",
  running: "var(--accent)",
  pending: "var(--line)",
  failed: "var(--bad)",
} as const;

const W = 128; // node width
const H = 34; // node height
const GAP = 40; // column gap
const MAX_TASKS = 10;

function truncate(s: string, n: number) {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s;
}

/**
 * Workflow graph. Task nodes are the real persisted tasks; every other node's
 * state and count is derived from the same backend data as the progress list.
 */
export function ResearchGraph({
  overview,
  events,
}: {
  overview: Overview;
  events: AgentEvent[];
}) {
  const { run, stats, tasks } = overview;
  const stages = Object.fromEntries(
    deriveStages(run, stats, events).map((s) => [s.key, s.state]),
  ) as Record<string, StageState>;

  const shown = tasks.slice(0, MAX_TASKS);
  const hidden = tasks.length - shown.length;
  const rows = Math.max(shown.length + (hidden > 0 ? 1 : 0), 1);
  const height = rows * (H + 10) + 30;
  const mid = height / 2;
  const col = (i: number) => 10 + i * (W + GAP);
  const width = col(8) + W + 10;

  const single: { key: string; col: number; label: string; sub: string; state: StageState }[] = [
    { key: "query", col: 0, label: "User query", sub: "", state: "done" },
    { key: "planner", col: 1, label: "Planner", sub: `${stats.questions} questions`, state: stages.planner },
    { key: "sources", col: 3, label: "Sources", sub: `${stats.sources} found`, state: stages.research },
    { key: "verify", col: 4, label: "Verify", sub: `${stats.claims} claims`, state: stages.verify },
    {
      key: "contra",
      col: 5,
      label: "Contradictions",
      sub: `${stats.contradictions} found`,
      state: stages.contradictions,
    },
    { key: "synth", col: 6, label: "Synthesis", sub: "", state: stages.synthesis },
    { key: "critic", col: 7, label: "Critic", sub: "", state: stages.critic },
    { key: "report", col: 8, label: "Final report", sub: "", state: stages.report },
  ];

  const taskY = (i: number) => 15 + i * (H + 10);
  const edge = (x1: number, y1: number, x2: number, y2: number, key: string) => {
    const mx = (x1 + x2) / 2;
    return (
      <path
        key={key}
        d={`M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`}
        fill="none"
        stroke="var(--line)"
        strokeWidth={1.5}
      />
    );
  };

  const edges = [
    edge(col(0) + W, mid, col(1), mid, "q-p"),
    ...shown.flatMap((_, i) => [
      edge(col(1) + W, mid, col(2), taskY(i) + H / 2, `p-t${i}`),
      edge(col(2) + W, taskY(i) + H / 2, col(3), mid, `t${i}-s`),
    ]),
    ...(shown.length === 0 ? [edge(col(1) + W, mid, col(3), mid, "p-s")] : []),
    ...[3, 4, 5, 6, 7].map((c) => edge(col(c) + W, mid, col(c + 1), mid, `e${c}`)),
  ];

  return (
    <Panel title="Research Graph">
      <div className="overflow-x-auto">
        <svg
          viewBox={`0 0 ${width} ${height}`}
          width={width}
          height={height}
          className="min-w-[960px]"
          role="img"
          aria-label="Research workflow graph"
        >
          {edges}
          {run.iteration > 1 && (
            <path
              d={`M${col(7) + W / 2},${mid + H / 2} C${col(7) + W / 2},${height + 10} ${col(2) + W / 2},${height + 10} ${col(2) + W / 2},${taskY(Math.max(shown.length - 1, 0)) + H}`}
              fill="none"
              stroke="var(--info)"
              strokeDasharray="4 4"
              strokeWidth={1.5}
            />
          )}
          {single.map((n) => (
            <g key={n.key} transform={`translate(${col(n.col)},${mid - H / 2})`}>
              <rect
                width={W}
                height={H}
                rx={6}
                fill="var(--panel-2)"
                stroke={COLOR[n.state]}
                strokeWidth={1.5}
                className={n.state === "active" ? "live-pulse" : ""}
              />
              <text x={W / 2} y={n.sub ? 14 : 21} textAnchor="middle" fontSize={11} fill="var(--foreground)">
                {n.label}
              </text>
              {n.sub && (
                <text x={W / 2} y={27} textAnchor="middle" fontSize={9.5} fill="var(--muted)">
                  {n.sub}
                </text>
              )}
            </g>
          ))}
          {shown.map((t, i) => (
            <g key={t.id} transform={`translate(${col(2)},${taskY(i)})`}>
              <title>{`${t.question} (${t.status}${t.iteration > 1 ? `, iteration ${t.iteration}` : ""})`}</title>
              <rect
                width={W}
                height={H}
                rx={6}
                fill="var(--panel-2)"
                stroke={TASK_COLOR[t.status]}
                strokeWidth={1.5}
                className={t.status === "running" ? "live-pulse" : ""}
              />
              <text x={8} y={14} fontSize={9.5} fill="var(--muted)">
                {`task ${i + 1} · ${t.status}`}
              </text>
              <text x={8} y={27} fontSize={9.5} fill="var(--foreground)">
                {truncate(t.question, 21)}
              </text>
            </g>
          ))}
          {hidden > 0 && (
            <text x={col(2) + W / 2} y={taskY(shown.length) + 20} textAnchor="middle" fontSize={10} fill="var(--muted)">
              {`+${hidden} more tasks`}
            </text>
          )}
        </svg>
      </div>
    </Panel>
  );
}
