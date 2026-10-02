import type { ReactNode } from "react";
import type { SupportLevel } from "@/lib/api";

export function Panel({
  title,
  right,
  children,
  className = "",
}: {
  title: string;
  right?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`rounded-lg border border-line bg-panel ${className}`}>
      <header className="flex items-center justify-between border-b border-line px-4 py-2.5">
        <h2 className="font-mono text-[11px] uppercase tracking-[0.14em] text-muted">
          {title}
        </h2>
        {right}
      </header>
      <div className="p-4">{children}</div>
    </section>
  );
}

const SUPPORT_STYLE: Record<SupportLevel, string> = {
  SUPPORTED: "border-ok/40 text-ok",
  PARTIALLY_SUPPORTED: "border-warn/40 text-warn",
  CONTRADICTED: "border-bad/40 text-bad",
  UNSUPPORTED: "border-bad/40 text-bad",
  UNVERIFIED: "border-muted/40 text-muted",
};

export function SupportBadge({ level }: { level: SupportLevel }) {
  return (
    <span
      className={`rounded border px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide ${SUPPORT_STYLE[level]}`}
    >
      {level.replace("_", " ").toLowerCase()}
    </span>
  );
}

export function Tag({ children }: { children: ReactNode }) {
  return (
    <span className="rounded border border-line px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">
      {children}
    </span>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="py-6 text-center text-sm text-muted">{children}</p>;
}

export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], { hour12: false });
}

export function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}
