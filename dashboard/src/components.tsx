import React, { type ReactNode } from "react";

/// The one error banner. All three tabs can fail, and all three failed in their
/// own markup — four copies of the same class string, which is three chances
/// for the tabs to disagree about what a failure looks like after a palette
/// change.
export function ErrorBanner({ children }: { children: React.ReactNode }) {
  return <div className="bg-warn-soft text-warn rounded-md px-4 py-2.5 text-sm">{children}</div>;
}

/// A labelled dashed rule marking where a cut falls inside a column — the gate
/// in the fused column, the token budget in the injected one. Both are the same
/// statement ("everything below here did not make it"), so they are one shape.
export function Divider({ label }: { label: string }) {
  return (
    <div className="flex items-center gap-2 py-0.5">
      <div className="grow border-t-2 border-dashed border-warn" />
      <span className="font-mono text-[11px] font-medium text-warn">{label}</span>
      <div className="grow border-t-2 border-dashed border-warn" />
    </div>
  );
}

export function ago(unix: number | null | undefined): string {
  if (!unix) return "never";
  const s = Math.max(0, Math.floor(Date.now() / 1000) - unix);
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

export function Panel({ title, aside, children, testid }: { title: string; aside?: ReactNode; children: ReactNode; testid?: string }) {
  return (
    <section className="bg-surface border border-line rounded-lg p-4 flex flex-col gap-3 min-w-0" data-testid={testid}>
      <header className="flex items-center gap-3 flex-wrap min-h-7">
        <h2 className="text-[15px] font-semibold">{title}</h2>
        {aside}
      </header>
      {children}
    </section>
  );
}
