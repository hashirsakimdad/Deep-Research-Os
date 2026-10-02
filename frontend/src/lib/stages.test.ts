import assert from "node:assert/strict";
import { test } from "node:test";
import type { AgentEvent, ResearchRun, RunStats } from "./api.ts";
import { currentIterationEvents, deriveStages } from "./stages.ts";

const stats = (over: Partial<RunStats> = {}): RunStats => ({
  questions: 0,
  tasks_total: 0,
  tasks_completed: 0,
  tasks_failed: 0,
  tasks_running: 0,
  sources: 0,
  claims: 0,
  supported: 0,
  partially_supported: 0,
  unverified: 0,
  unsupported: 0,
  contradicted: 0,
  contradictions: 0,
  genuine_contradictions: 0,
  ...over,
});

const run = (status: ResearchRun["status"], iteration = 1): ResearchRun => ({
  id: "r",
  query: "q",
  status,
  iteration,
  error: null,
  created_at: "",
  updated_at: "",
  completed_at: null,
});

const ev = (id: number, event_type: string): AgentEvent => ({
  id,
  research_id: "r",
  agent: "x",
  event_type,
  message: "",
  metadata: {},
  timestamp: "",
});

const states = (r: ResearchRun, s: RunStats, e: AgentEvent[] = []) =>
  deriveStages(r, s, e).map((x) => `${x.key}:${x.state}`);

test("planning shows only the planner active", () => {
  assert.deepEqual(states(run("planning"), stats()), [
    "planner:active",
    "tasks:pending",
    "research:pending",
    "verify:pending",
    "contradictions:pending",
    "synthesis:pending",
    "critic:pending",
    "report:pending",
  ]);
});

test("researching marks planner and tasks done only when the data says so", () => {
  const s = states(run("researching"), stats({ questions: 5, tasks_total: 5 }));
  assert.equal(s[0], "planner:done");
  assert.equal(s[1], "tasks:done");
  assert.equal(s[2], "research:active");
  assert.equal(s[3], "verify:pending");
});

test("completed run is fully done", () => {
  assert.ok(states(run("completed"), stats()).every((s) => s.endsWith(":done")));
});

test("a failed run flags the first stage whose completion was never recorded", () => {
  const events = [ev(1, "planning_completed"), ev(2, "verification_started")];
  const s = states(run("failed"), stats({ questions: 3, tasks_total: 3 }), events);
  assert.equal(s[2], "research:done");
  assert.equal(s[3], "verify:failed");
  assert.equal(s[4], "contradictions:pending");
});

test("only events after the latest iteration_started count for failure analysis", () => {
  const events = [
    ev(1, "verification_completed"),
    ev(2, "iteration_started"),
    ev(3, "verification_started"),
  ];
  assert.deepEqual(
    currentIterationEvents(events).map((e) => e.id),
    [2, 3],
  );
  const s = states(run("failed", 2), stats({ questions: 3, tasks_total: 3 }), events);
  assert.equal(s[3], "verify:failed");
});
