"use client";

import { useEffect, useState, type ReactNode } from "react";
import { checkAccess, getAccessKey, setAccessKey } from "@/lib/api";

type Phase = "checking" | "open" | "locked";

/**
 * Wraps the app. If the backend requires an access key (HTTP 401) and the one saved in this browser is
 * missing or wrong, shows a prompt instead of the app. If the backend is unreachable the app renders
 * normally so its own error messages explain what is wrong.
 */
export function AccessGate({ children }: { children: ReactNode }) {
  const [phase, setPhase] = useState<Phase>("checking");
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void checkAccess(getAccessKey()).then((result) => {
      if (!cancelled) setPhase(result === "unauthorized" ? "locked" : "open");
    });
    return () => {
      cancelled = true;
    };
  }, []);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const result = await checkAccess(key.trim());
    setBusy(false);
    if (result === "ok") {
      setAccessKey(key.trim());
      setPhase("open");
    } else {
      setError(
        result === "unauthorized"
          ? "That key was not accepted."
          : "Cannot reach the research backend. Is it running?",
      );
    }
  }

  if (phase === "checking") return null;
  if (phase === "open") return <>{children}</>;

  return (
    <main className="mx-auto flex w-full max-w-md flex-1 flex-col justify-center px-6 py-16">
      <p className="font-mono text-xs uppercase tracking-[0.3em] text-accent">Deep Research OS</p>
      <h1 className="mt-4 text-2xl font-semibold tracking-tight">Access key required</h1>
      <p className="mt-2 text-sm leading-relaxed text-muted">
        This deployment is protected. Enter the access key to continue; it is kept only in this browser.
      </p>
      <form onSubmit={submit} className="mt-6 space-y-3">
        <input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          autoComplete="current-password"
          aria-label="Access key"
          placeholder="Access key"
          className="w-full rounded-lg border border-line bg-panel p-3 font-mono text-sm outline-none placeholder:text-muted/60 focus:border-accent"
        />
        <button
          type="submit"
          disabled={busy || key.trim().length === 0}
          className="w-full rounded-lg bg-accent px-5 py-2.5 font-mono text-xs font-semibold uppercase tracking-[0.18em] text-black transition-opacity disabled:cursor-not-allowed disabled:opacity-40"
        >
          {busy ? "Checking…" : "Unlock"}
        </button>
        {error && (
          <p role="alert" className="text-sm text-bad">
            {error}
          </p>
        )}
      </form>
    </main>
  );
}
