"use client";

import { useEffect, useRef } from "react";
import type { AgentEvent } from "@/lib/api";
import { Empty, Panel, formatTime } from "./ui";

const AGENT_TONE: Record<string, string> = {
  planner: "text-info",
  researcher: "text-accent",
  verifier: "text-ok",
  contradiction_detector: "text-warn",
  synthesizer: "text-info",
  critic: "text-bad",
  orchestrator: "text-muted",
};

export function LiveFeed({ events, live }: { events: AgentEvent[]; live: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    const el = box.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [events.length]);

  return (
    <Panel
      title="Agent Events"
      right={
        <span className="flex items-center gap-1.5 font-mono text-[11px] text-muted">
          <span
            className={`inline-block h-1.5 w-1.5 rounded-full ${live ? "bg-ok live-pulse" : "bg-muted/50"}`}
          />
          {live ? "streaming" : "idle"} · {events.length}
        </span>
      }
    >
      <div
        ref={box}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
        className="h-80 overflow-y-auto font-mono text-xs leading-relaxed"
      >
        {events.length === 0 ? (
          <Empty>Waiting for the first event…</Empty>
        ) : (
          events.map((e) => (
            <div key={e.id} className="flex gap-3 py-0.5">
              <span className="shrink-0 text-muted/70">{formatTime(e.timestamp)}</span>
              <span className={`w-28 shrink-0 truncate ${AGENT_TONE[e.agent] ?? ""}`}>
                {e.agent}
              </span>
              <span className="min-w-0 break-words text-foreground/90">{e.message}</span>
            </div>
          ))
        )}
      </div>
    </Panel>
  );
}
