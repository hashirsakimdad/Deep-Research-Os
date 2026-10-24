// Types mirror the JSON served by the Rust backend (docs/api.md).

import { readSse } from "./sse";

export const API_URL =
  process.env.NEXT_PUBLIC_API_URL?.replace(/\/$/, "") ?? "http://localhost:8080";

export type RunStatus =
  | "planning"
  | "researching"
  | "verifying"
  | "analyzing"
  | "synthesizing"
  | "critiquing"
  | "completed"
  | "failed";

export type TaskStatus = "pending" | "running" | "completed" | "failed";

export type SupportLevel =
  | "SUPPORTED"
  | "PARTIALLY_SUPPORTED"
  | "CONTRADICTED"
  | "UNSUPPORTED"
  | "UNVERIFIED";

export type ContradictionKind =
  | "DIRECT_CONTRADICTION"
  | "CONTEXT_DIFFERENCE"
  | "TIME_DIFFERENCE"
  | "POPULATION_DIFFERENCE"
  | "METRIC_DIFFERENCE"
  | "SCOPE_DIFFERENCE"
  | "NO_CONFLICT";

export interface ResearchRun {
  id: string;
  query: string;
  status: RunStatus;
  iteration: number;
  error: string | null;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
}

export interface ResearchQuestion {
  id: string;
  question: string;
  objective: string;
  priority: "high" | "medium" | "low";
  position: number;
}

export interface ResearchTask {
  id: string;
  question_id: string | null;
  question: string;
  objective: string;
  priority: "high" | "medium" | "low";
  status: TaskStatus;
  iteration: number;
  error: string | null;
  dependencies: string[];
}

export interface RunStats {
  questions: number;
  tasks_total: number;
  tasks_completed: number;
  tasks_failed: number;
  tasks_running: number;
  sources: number;
  claims: number;
  supported: number;
  partially_supported: number;
  unverified: number;
  unsupported: number;
  contradicted: number;
  contradictions: number;
  genuine_contradictions: number;
}

export interface Overview {
  run: ResearchRun;
  questions: ResearchQuestion[];
  tasks: ResearchTask[];
  stats: RunStats;
}

export interface AgentEvent {
  id: number;
  research_id: string;
  agent: string;
  event_type: string;
  message: string;
  metadata: Record<string, unknown>;
  timestamp: string;
}

export interface SourceRef {
  id: string;
  url: string;
  title: string;
  publisher: string | null;
  published_at: string | null;
  source_type: string;
  nature: string;
}

export interface Evidence {
  id: string;
  claim_id: string;
  source_id: string;
  excerpt: string;
  relevance: number;
  support_level: SupportLevel;
  reasoning: string | null;
}

export interface ClaimRecord {
  claim: {
    id: string;
    research_task_id: string;
    text: string;
    source_id: string;
    confidence: number;
    status: SupportLevel;
    iteration: number;
  };
  evidence: Evidence[];
  source: SourceRef;
  question_id: string | null;
  question: string;
}

export interface Contradiction {
  id: string;
  claim_a_id: string;
  claim_b_id: string;
  kind: ContradictionKind;
  explanation: string;
}

export interface CritiqueIssue {
  type: string;
  claim_id: string | null;
  description: string;
  severity: "high" | "medium" | "low";
}

export interface Critique {
  id: string;
  iteration: number;
  quality: number;
  needs_more_research: boolean;
  issues: CritiqueIssue[];
  required_research: string[];
}

export interface CitedStatement {
  text: string;
  claim_ids: string[];
}

export interface ReportContent {
  executive_summary: string;
  methodology: string;
  key_findings: CitedStatement[];
  detailed_findings: { heading: string; body: CitedStatement[] }[];
  contradictory_evidence: CitedStatement[];
  limitations: string[];
  open_questions: string[];
  conclusion: string;
}

export interface Citation {
  number: number;
  source_id: string;
  url: string;
  title: string;
}

export interface Report {
  id: string;
  iteration: number;
  is_final: boolean;
  content: ReportContent;
  markdown: string;
  citations: Citation[];
}

const KEY_STORAGE = "dro_access_key";

/** The access key lives only in this browser (localStorage); it is never baked into the site's code. */
export function getAccessKey(): string {
  try {
    return window.localStorage.getItem(KEY_STORAGE) ?? "";
  } catch {
    return "";
  }
}

export function setAccessKey(key: string): void {
  try {
    if (key) window.localStorage.setItem(KEY_STORAGE, key);
    else window.localStorage.removeItem(KEY_STORAGE);
  } catch {
    /* storage unavailable: the key just will not persist */
  }
}

export function authHeaders(key: string = getAccessKey()): Record<string, string> {
  return key ? { "x-api-key": key } : {};
}

/** Checks a key against the backend: "ok", "unauthorized", or "unreachable". */
export async function checkAccess(key: string): Promise<"ok" | "unauthorized" | "unreachable"> {
  try {
    const res = await fetch(`${API_URL}/api/research`, { headers: authHeaders(key), cache: "no-store" });
    if (res.status === 401) return "unauthorized";
    return res.ok ? "ok" : "unreachable";
  } catch {
    return "unreachable";
  }
}

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${API_URL}${path}`, { headers: authHeaders(), cache: "no-store" });
  if (!res.ok) {
    throw new ApiError(res.status, await safeMessage(res));
  }
  return res.json() as Promise<T>;
}

async function safeMessage(res: Response): Promise<string> {
  try {
    const body = (await res.json()) as { error?: string };
    return body.error ?? res.statusText;
  } catch {
    return res.statusText;
  }
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export const api = {
  async start(query: string): Promise<{ research_id: string; status: RunStatus }> {
    const res = await fetch(`${API_URL}/api/research`, {
      method: "POST",
      headers: { "Content-Type": "application/json", ...authHeaders() },
      body: JSON.stringify({ query }),
    });
    if (!res.ok) throw new ApiError(res.status, await safeMessage(res));
    return res.json();
  },
  list: () => get<ResearchRun[]>("/api/research"),
  overview: (id: string) => get<Overview>(`/api/research/${id}`),
  events: (id: string) => get<AgentEvent[]>(`/api/research/${id}/events`),
  sources: (id: string) => get<SourceRef[]>(`/api/research/${id}/sources`),
  claims: (id: string) => get<ClaimRecord[]>(`/api/research/${id}/claims`),
  contradictions: (id: string) =>
    get<Contradiction[]>(`/api/research/${id}/contradictions`),
  critiques: (id: string) => get<Critique[]>(`/api/research/${id}/critiques`),
  report: (id: string) => get<Report>(`/api/research/${id}/report`),
  streamUrl: (id: string) => `${API_URL}/api/research/${id}/stream`,
  /** Follows the live event stream; resumes after `lastEventId` so nothing is missed or repeated. */
  stream: (
    id: string,
    opts: { lastEventId?: string; signal?: AbortSignal },
    onOpen: () => void,
    onEvent: (e: AgentEvent, sseId?: string) => void,
  ) =>
    readSse(
      `${API_URL}/api/research/${id}/stream`,
      { headers: authHeaders(), lastEventId: opts.lastEventId, signal: opts.signal },
      onOpen,
      (m) => onEvent(JSON.parse(m.data) as AgentEvent, m.id),
    ),
};
