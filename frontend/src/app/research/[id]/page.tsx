"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useState } from "react";
import {
  ClaimExplorer,
  ContradictionPanel,
  CritiquePanel,
  SourceExplorer,
} from "@/components/Explorers";
import { LiveFeed } from "@/components/LiveFeed";
import { ProgressTracker } from "@/components/ProgressTracker";
import { ReportView, TracePanel, type TraceTarget } from "@/components/ReportView";
import { ResearchGraph } from "@/components/ResearchGraph";
import { StatsPanel } from "@/components/StatsPanel";
import { Empty, Panel, Tag } from "@/components/ui";
import { useResearch } from "@/lib/useResearch";

type Tab = "report" | "claims" | "sources" | "contradictions" | "critic";

export default function ResearchPage() {
  const { id } = useParams<{ id: string }>();
  // keyed so navigating between runs never shows another run's state
  return <ResearchDashboard key={id} id={id} />;
}

function ResearchDashboard({ id }: { id: string }) {
  const view = useResearch(id);
  const [tab, setTab] = useState<Tab>("report");
  const [trace, setTrace] = useState<TraceTarget | null>(null);

  const { overview } = view;
  if (!overview) {
    return (
      <main className="mx-auto w-full max-w-6xl flex-1 px-6 py-10">
        <Link href="/" className="font-mono text-xs text-muted hover:text-foreground">
          ← new research
        </Link>
        <div className="mt-8">
          {view.error ? (
            <p className="text-sm text-bad">{view.error}</p>
          ) : (
            <Empty>Loading research run…</Empty>
          )}
        </div>
      </main>
    );
  }

  const { run, stats } = overview;
  const tabs: { key: Tab; label: string; count?: number }[] = [
    { key: "report", label: "Report" },
    { key: "claims", label: "Claims", count: view.claims.length },
    { key: "sources", label: "Sources", count: view.sources.length },
    { key: "contradictions", label: "Contradictions", count: view.contradictions.length },
    { key: "critic", label: "Critic", count: view.critiques.length },
  ];

  return (
    <main className="mx-auto w-full max-w-6xl flex-1 space-y-5 px-6 py-8">
      <header>
        <div className="flex items-center justify-between">
          <Link href="/" className="font-mono text-xs text-muted hover:text-foreground">
            ← new research
          </Link>
          <Tag>{run.status}</Tag>
        </div>
        <p className="mt-4 font-mono text-[11px] uppercase tracking-[0.14em] text-muted">
          Research run
        </p>
        <h1 className="mt-1 text-xl font-semibold leading-snug">{run.query}</h1>
      </header>

      <div className="grid gap-5 lg:grid-cols-[320px_1fr]">
        <div className="space-y-5">
          <ProgressTracker run={run} stats={stats} events={view.events} />
          <StatsPanel stats={stats} />
          <Panel title="Research Questions" right={<Tag>{overview.questions.length}</Tag>}>
            <ol className="space-y-2 text-sm">
              {overview.questions.map((q, i) => (
                <li key={q.id} className="flex gap-2">
                  <span className="font-mono text-muted">{i + 1}.</span>
                  <span>{q.question}</span>
                </li>
              ))}
              {overview.questions.length === 0 && (
                <li className="text-muted">The planner has not produced questions yet.</li>
              )}
            </ol>
          </Panel>
        </div>

        <div className="min-w-0 space-y-5">
          <ResearchGraph overview={overview} events={view.events} />
          <LiveFeed events={view.events} live={view.live} />
        </div>
      </div>

      <TracePanel target={trace} claims={view.claims} onClose={() => setTrace(null)} />

      <nav className="flex flex-wrap gap-1 border-b border-line">
        {tabs.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`-mb-px border-b-2 px-3 py-2 font-mono text-xs uppercase tracking-wide ${
              tab === t.key
                ? "border-accent text-accent"
                : "border-transparent text-muted hover:text-foreground"
            }`}
          >
            {t.label}
            {t.count !== undefined && <span className="ml-1.5 text-muted">{t.count}</span>}
          </button>
        ))}
      </nav>

      {tab === "report" && <ReportView report={view.report} claims={view.claims} onTrace={setTrace} />}
      {tab === "claims" && <ClaimExplorer claims={view.claims} />}
      {tab === "sources" && <SourceExplorer sources={view.sources} claims={view.claims} />}
      {tab === "contradictions" && (
        <ContradictionPanel contradictions={view.contradictions} claims={view.claims} />
      )}
      {tab === "critic" && <CritiquePanel critiques={view.critiques} />}
    </main>
  );
}
