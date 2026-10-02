"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  AgentEvent,
  ApiError,
  ClaimRecord,
  Contradiction,
  Critique,
  Overview,
  Report,
  SourceRef,
  api,
} from "./api";

export interface ResearchView {
  overview: Overview | null;
  events: AgentEvent[];
  sources: SourceRef[];
  claims: ClaimRecord[];
  contradictions: Contradiction[];
  critiques: Critique[];
  report: Report | null;
  live: boolean;
  error: string | null;
}

const EMPTY: ResearchView = {
  overview: null,
  events: [],
  sources: [],
  claims: [],
  contradictions: [],
  critiques: [],
  report: null,
  live: false,
  error: null,
};

const TERMINAL = new Set(["research_completed", "research_failed"]);

/**
 * Loads a research run and keeps it current. Everything shown comes from the
 * backend: the SSE stream is a trigger to re-read persisted state, and the
 * event log is the stream itself.
 */
export function useResearch(id: string): ResearchView {
  const [view, setView] = useState<ResearchView>(EMPTY);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const alive = useRef(true);

  const refresh = useCallback(
    async (includeReport: boolean) => {
      const [overview, sources, claims, contradictions, critiques] =
        await Promise.allSettled([
          api.overview(id),
          api.sources(id),
          api.claims(id),
          api.contradictions(id),
          api.critiques(id),
        ]);
      let report: Report | null | undefined;
      if (includeReport) {
        try {
          report = await api.report(id);
        } catch (e) {
          // 404 just means no report yet
          report = e instanceof ApiError && e.status === 404 ? null : undefined;
        }
      }
      if (!alive.current) return;
      setView((v) => ({
        ...v,
        overview: overview.status === "fulfilled" ? overview.value : v.overview,
        sources: sources.status === "fulfilled" ? sources.value : v.sources,
        claims: claims.status === "fulfilled" ? claims.value : v.claims,
        contradictions:
          contradictions.status === "fulfilled" ? contradictions.value : v.contradictions,
        critiques: critiques.status === "fulfilled" ? critiques.value : v.critiques,
        report: report === undefined ? v.report : report,
        error:
          overview.status === "rejected"
            ? String((overview.reason as Error).message)
            : null,
      }));
    },
    [id],
  );

  useEffect(() => {
    alive.current = true;
    let source: EventSource | null = null;

    const schedule = (includeReport: boolean) => {
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => void refresh(includeReport), 500);
    };

    void (async () => {
      await refresh(true);
      if (!alive.current) return;
      source = new EventSource(api.streamUrl(id));
      source.onopen = () => setView((v) => ({ ...v, live: true }));
      source.onerror = () => setView((v) => ({ ...v, live: false }));
      source.onmessage = (msg) => {
        const ev = JSON.parse(msg.data) as AgentEvent;
        setView((v) =>
          v.events.some((e) => e.id === ev.id) ? v : { ...v, events: [...v.events, ev] },
        );
        const wantsReport =
          ev.event_type === "synthesis_completed" || TERMINAL.has(ev.event_type);
        schedule(wantsReport);
        if (TERMINAL.has(ev.event_type)) {
          source?.close();
          setView((v) => ({ ...v, live: false }));
        }
      };
    })();

    return () => {
      alive.current = false;
      if (timer.current) clearTimeout(timer.current);
      source?.close();
    };
  }, [id, refresh]);

  return view;
}

