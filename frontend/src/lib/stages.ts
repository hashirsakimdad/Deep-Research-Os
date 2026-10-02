import type { AgentEvent, ResearchRun, RunStats } from "./api";

export type StageState = "done" | "active" | "pending" | "failed";

export interface Stage {
  key: string;
  label: string;
  state: StageState;
}

interface StageDef {
  key: string;
  label: string;
  /** Run status during which this stage is the one executing. */
  activeOn: string | null;
  /** Evidence (from persisted counts/events) that the stage has finished. */
  satisfied: (stats: RunStats, types: Set<string>) => boolean;
}

const DEFS: StageDef[] = [
  {
    key: "planner",
    label: "Planner",
    activeOn: "planning",
    satisfied: (s) => s.questions > 0,
  },
  {
    key: "tasks",
    label: "Research Tasks",
    activeOn: null,
    satisfied: (s) => s.tasks_total > 0,
  },
  {
    key: "research",
    label: "Parallel Research",
    activeOn: "researching",
    satisfied: (_s, t) => t.has("verification_started"),
  },
  {
    key: "verify",
    label: "Evidence Verification",
    activeOn: "verifying",
    satisfied: (_s, t) => t.has("verification_completed"),
  },
  {
    key: "contradictions",
    label: "Contradiction Analysis",
    activeOn: "analyzing",
    satisfied: (_s, t) => t.has("contradiction_analysis_completed"),
  },
  {
    key: "synthesis",
    label: "Synthesis",
    activeOn: "synthesizing",
    satisfied: (_s, t) => t.has("synthesis_completed"),
  },
  {
    key: "critic",
    label: "Critic",
    activeOn: "critiquing",
    satisfied: (_s, t) => t.has("critique_completed"),
  },
  {
    key: "report",
    label: "Final Report",
    activeOn: null,
    satisfied: (_s, t) => t.has("research_completed"),
  },
];

/** Events belonging to the iteration that is currently (or was last) running. */
export function currentIterationEvents(events: AgentEvent[]): AgentEvent[] {
  let start = 0;
  events.forEach((e, i) => {
    if (e.event_type === "iteration_started") start = i;
  });
  return events.slice(start);
}

/**
 * Derives stage states purely from backend data: the run status, persisted
 * counts and persisted events. Nothing is animated or assumed.
 */
export function deriveStages(
  run: ResearchRun,
  stats: RunStats,
  events: AgentEvent[],
): Stage[] {
  if (run.status === "completed") {
    return DEFS.map((d) => ({ key: d.key, label: d.label, state: "done" }));
  }

  if (run.status === "failed") {
    const types = new Set(currentIterationEvents(events).map((e) => e.event_type));
    let failedAt = DEFS.findIndex((d) => !d.satisfied(stats, types));
    if (failedAt < 0) failedAt = DEFS.length - 1;
    return DEFS.map((d, i) => ({
      key: d.key,
      label: d.label,
      state: i < failedAt ? "done" : i === failedAt ? "failed" : "pending",
    }));
  }

  const activeIdx = DEFS.findIndex((d) => d.activeOn === run.status);
  return DEFS.map((d, i) => {
    let state: StageState;
    if (i === activeIdx) state = "active";
    else if (d.key === "tasks") state = stats.tasks_total > 0 ? "done" : "pending";
    else state = i < activeIdx ? "done" : "pending";
    return { key: d.key, label: d.label, state };
  });
}
