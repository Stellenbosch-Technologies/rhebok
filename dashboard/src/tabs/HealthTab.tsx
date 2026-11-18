import { useEffect, useRef, useState } from "react";
import { AGENT_NAME, getProgress, startIndex, PALETTE, MEMORY_SHADE, type BenchTier, type Progress, type Stats } from "../api";
import { ago, ErrorBanner, Panel } from "../components";
import SkippedPanel from "../SkippedPanel";
import { secondaryButton } from "../integrations/agents";
import VersionCard from "../VersionCard";
import InstallCard from "../InstallCard";

const TIER_NAMES = ["instant", "fast", "balanced", "thorough", "exhaustive"];

function CopyCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <span className="inline-flex items-center gap-2">
      <code className="font-mono text-xs bg-sunken rounded px-1.5 py-0.5">{command}</code>
      <button
        onClick={() => navigator.clipboard.writeText(command).then(() => setCopied(true)).catch(() => setCopied(false))}
        className="text-xs font-semibold text-accent"
      >
        {copied ? "copied" : "copy"}
      </button>
    </span>
  );
}

type Slice = { key: string; label: string; count: number; color: string };

function slicesOf(stats: Stats): Slice[] {
  const agents = Object.entries(stats.sessions_by_agent ?? {}).filter(([, n]) => n > 0);
  const slices: Slice[] = [];
  for (const [source, count] of Object.entries(stats.by_source)) {
    if (source === "transcript" && agents.length > 1) {
      agents.forEach(([agent, n], i) =>
        slices.push({ key: `transcript:${agent}`, label: `${AGENT_NAME[agent] ?? agent} sessions`, count: n, color: i === 0 ? PALETTE.session : "#C4D3DE" }),
      );
    } else if (source === "transcript") {
      const agent = agents[0]?.[0];
      slices.push({ key: source, label: `${agent ? `${AGENT_NAME[agent] ?? agent} ` : ""}sessions`, count, color: PALETTE.session });
    } else if (source === "markdown") {
      slices.push({ key: source, label: "notes", count, color: PALETTE.note });
    } else {
      slices.push({ key: source, label: source, count, color: PALETTE.project });
    }
  }
  return slices.sort((a, b) => b.count - a.count);
}

function IndexPanel({ stats, prog, onIndexStarted }: { stats: Stats; prog: Progress; onIndexStarted: () => void }) {
  const slices = slicesOf(stats);
  const total = slices.reduce((a, s) => a + s.count, 0);
  const [starting, setStarting] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const indexing = !prog.idle;
  const index = () => {
    setStarting(true);
    setNote(null);
    startIndex()
      .then((r) => {
        setNote(r.kind === "busy" ? r.error : null);
        if (r.kind === "started") onIndexStarted();
      })
      .catch((e) => setNote(e instanceof Error ? e.message : String(e)))
      .finally(() => setStarting(false));
  };
  const perDoc = stats.documents ? Math.round(stats.chunks / stats.documents) : 0;
  return (
    <Panel
      title="Index"
      testid="index-panel"
      aside={
        <>
          <span className="grow" />
          <button onClick={index} disabled={starting || indexing} className={secondaryButton}>
            {indexing ? "Indexing…" : starting ? "Starting…" : "Index now"}
          </button>
        </>
      }
    >
      <div className="flex gap-x-10 gap-y-4 flex-wrap">
        <div className="flex flex-col" data-testid="documents">
          <span className="text-3xl font-semibold tabular-nums leading-none">{stats.documents.toLocaleString()}</span>
          <span className="text-sm text-muted pt-1">documents</span>
        </div>
        <div className="flex flex-col">
          <span className="text-3xl font-semibold tabular-nums leading-none">{stats.chunks.toLocaleString()}</span>
          <span className="text-sm text-muted pt-1">chunks{perDoc ? `, about ${perDoc} per document` : ""}</span>
        </div>
        <div className="flex flex-col min-w-0">
          <span className="text-lg font-semibold leading-tight truncate pt-1.5" title={stats.model ?? undefined}>{stats.model?.split("@")[0] ?? "no model yet"}</span>
          <span className="text-sm text-muted pt-1">embedding model, 512-token chunks</span>
        </div>
      </div>
      {total > 0 && (
        <div className="flex flex-col gap-2">
          <div className="flex h-3 rounded overflow-hidden bg-sunken gap-px" role="img" aria-label={slices.map((s) => `${s.count} ${s.label}`).join(", ")}>
            {slices.map((s) => (
              <div key={s.key} title={`${s.count.toLocaleString()} ${s.label}`} style={{ width: `${(s.count / total) * 100}%`, minWidth: 4, background: s.color }} />
            ))}
          </div>
          <div className="flex gap-x-5 gap-y-1 flex-wrap text-sm">
            {slices.map((s) => (
              <span key={s.key} className="inline-flex items-center gap-1.5">
                <span className="w-2.5 h-2.5 rounded-sm" style={{ background: s.color }} />
                <span className="tabular-nums font-medium">{s.count.toLocaleString()}</span>
                <span className="text-muted">{s.label}</span>
              </span>
            ))}
          </div>
        </div>
      )}
      {indexing && (
        <div className="flex items-center gap-3.5 pt-1" data-testid="index-progress">
          <div className="grow h-2 bg-sunken rounded overflow-hidden">
            <div className="h-full bg-accent rounded transition-[width] duration-500" style={{ width: `${prog.pct ?? 0}%` }} />
          </div>
          <span className="text-sm tabular-nums shrink-0">
            {(prog.pct ?? 0).toFixed(1)}%, {prog.docs_done}/{prog.docs_total} documents, about {Math.max(1, Math.round((prog.eta_s ?? 0) / 60))} min left
          </span>
        </div>
      )}
      {note && <span className="text-sm text-warn">{note}</span>}
    </Panel>
  );
}

function MemoryPanel({ stats, onOpenMemories }: { stats: Stats; onOpenMemories: () => void }) {
  const kinds = Object.entries(stats.memory ?? {}).filter(([, n]) => n > 0);
  const total = kinds.reduce((a, [, n]) => a + n, 0);
  return (
    <Panel
      title="Memory"
      testid="memory-panel"
      aside={
        <>
          <span className="grow" />
          <button onClick={onOpenMemories} className={secondaryButton}>Open Memories</button>
        </>
      }
    >
      <div className="flex flex-col">
        <span className="text-3xl font-semibold tabular-nums leading-none">{total}</span>
        <span className="text-sm text-muted pt-1">{total === 1 ? "memory saved" : "memories saved"}</span>
      </div>
      {kinds.length > 0 ? (
        <ul className="flex flex-col gap-1.5 text-sm">
          {kinds.map(([kind, n]) => (
            <li key={kind} className="flex items-center gap-2">
              <span className="w-2.5 h-2.5 rounded-full" style={{ background: MEMORY_SHADE[kind] ?? PALETTE.memory }} />
              <span className="grow">{n === 1 ? kind : `${kind}s`}</span>
              <span className="tabular-nums font-medium">{n}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-sm text-muted">Nothing saved yet. Lessons and facts you save, and summaries of past sessions, show up here.</p>
      )}
    </Panel>
  );
}

function pct(r: number) {
  return `${Math.round(r * 100)}%`;
}

function TierDetail({ tier, all, hook, mcp, onOpenSetting }: {
  tier: BenchTier;
  all: BenchTier[];
  hook: string | null;
  mcp: string | null;
  onOpenSetting: (key: string) => void;
}) {
  const base = all.find((t) => t.tier === hook);
  const compare = base && base.tier !== tier.tier ? base : null;
  const points = compare ? Math.round((tier.recall_at_5 - compare.recall_at_5) * 100) : 0;
  const ms = compare ? tier.p50_ms - compare.p50_ms : 0;
  return (
    <div className="bg-sunken/60 rounded-md px-3.5 py-3 flex flex-col gap-2.5 text-sm" data-testid="tier-detail">
      <p className="max-w-[78ch]">
        At <strong>{tier.tier}</strong>, {pct(tier.recall_at_5)} of the test questions that have an answer found it in the top five results.
        Half the searches finished within {tier.p50_ms} ms and 95% within {tier.p95_ms} ms.
        {compare && (
          <> Against {compare.tier}, the hook's tier, that is {points >= 0 ? "+" : ""}{points} points of recall for {ms >= 0 ? "+" : ""}{ms} ms.</>
        )}
      </p>
      {(tier.tier !== hook || tier.tier !== mcp) && (
        <div className="flex gap-2 flex-wrap">
          {tier.tier !== hook && <button onClick={() => onOpenSetting("hook.quality")} className={secondaryButton}>Change the hook's tier</button>}
          {tier.tier !== mcp && <button onClick={() => onOpenSetting("mcp.quality")} className={secondaryButton}>Change the search tool's tier</button>}
        </div>
      )}
    </div>
  );
}

function RetrievalPanel({ stats, onOpenSetting }: { stats: Stats; onOpenSetting: (key: string) => void }) {
  const bench = stats.bench ?? [];
  const hook = stats.tiers ? TIER_NAMES[stats.tiers.hook] ?? null : null;
  const mcp = stats.tiers ? TIER_NAMES[stats.tiers.mcp] ?? null : null;
  const [picked, setPicked] = useState<string | null>(null);
  const selected = bench.find((t) => t.tier === (picked ?? hook)) ?? bench[0];
  const slowest = Math.max(1, ...bench.map((t) => t.p95_ms));
  const prov = stats.bench_provenance;
  const thenDocs = prov?.documents;
  if (bench.length === 0) {
    return (
      <Panel title="Retrieval quality" testid="retrieval-panel">
        <p className="text-sm text-muted">
          Nothing measured yet. Run <CopyCommand command="rhebok bench" /> to measure, for each quality tier, how often the right document is found and how long a search takes.
        </p>
      </Panel>
    );
  }
  return (
    <Panel
      title="Retrieval quality"
      testid="retrieval-panel"
      aside={<span className="text-sm text-muted">measured {ago(stats.bench_at)}{prov ? ` with ${prov.cases} test questions` : ""}</span>}
    >
      {thenDocs != null && thenDocs !== stats.documents && (
        <div className="flex items-center gap-x-3 gap-y-1 flex-wrap bg-accent-soft rounded-md px-3.5 py-2 text-sm" data-testid="bench-stale">
          <span className="grow">
            The index has changed since this was measured ({thenDocs.toLocaleString()} documents then, {stats.documents.toLocaleString()} now), so these numbers may have moved.
          </span>
          <CopyCommand command="rhebok bench" />
        </div>
      )}
      <div className="grid grid-cols-1 md:grid-cols-[minmax(150px,auto)_minmax(0,1fr)_minmax(0,1fr)] gap-x-6 text-sm">
        <div className="hidden md:contents text-muted text-xs">
          <span className="pb-1.5 px-2">Tier</span>
          <span className="pb-1.5">Right document in the top five</span>
          <span className="pb-1.5 pr-2">Search time: median, then 95th percentile</span>
        </div>
        {bench.map((t) => {
          const on = t.tier === selected?.tier;
          return (
            <button
              key={t.tier}
              aria-pressed={on}
              onClick={() => setPicked(t.tier)}
              data-tier={t.tier}
              className={`grid grid-cols-1 gap-1.5 md:gap-y-0 md:grid-cols-subgrid md:col-span-3 items-center text-left rounded-md px-2 py-2 border-t border-sunken first-of-type:border-t-0 focus-visible:outline-2 focus-visible:outline-accent ${on ? "bg-sunken" : "hover:bg-sunken/50"}`}
            >
              <span className="flex items-center gap-1.5">
                <span className={on ? "font-semibold" : ""}>{t.tier}</span>
                {t.tier === hook && <span className="text-[11px] font-semibold text-accent border border-accent/40 rounded px-1">hook</span>}
                {t.tier === mcp && <span className="text-[11px] font-semibold text-muted border border-line rounded px-1">search tool</span>}
              </span>
              <span className="flex items-center gap-2.5">
                <span className="md:hidden w-12 text-xs text-muted">found</span>
                <span className="grow h-2.5 bg-sunken rounded overflow-hidden">
                  <span className="block h-full bg-accent rounded" style={{ width: pct(t.recall_at_5) }} />
                </span>
                <span className="w-12 text-right tabular-nums font-medium">{pct(t.recall_at_5)}</span>
              </span>
              <span className="flex items-center gap-2.5">
                <span className="md:hidden w-12 text-xs text-muted">time</span>
                <span className="grow h-2.5 relative">
                  <span className="absolute inset-y-0 left-0 bg-accent-soft rounded" style={{ width: `${(t.p95_ms / slowest) * 100}%` }} />
                  <span className="absolute inset-y-0 left-0 bg-sun rounded" style={{ width: `${Math.max(1, (t.p50_ms / slowest) * 100)}%` }} />
                </span>
                <span className="w-[96px] text-right tabular-nums">
                  <span className="font-medium">{t.p50_ms}</span> <span className="text-muted">/ {t.p95_ms} ms</span>
                </span>
              </span>
            </button>
          );
        })}
      </div>
      {selected && <TierDetail tier={selected} all={bench} hook={hook} mcp={mcp} onOpenSetting={onOpenSetting} />}
      {hook && stats.hook_threshold != null && (
        <p className="text-sm text-muted">
          Before each prompt the hook searches at <strong className="text-ink font-semibold">{hook}</strong> and passes on only results scoring at least{" "}
          <button onClick={() => onOpenSetting("hook.threshold")} className="font-semibold text-accent underline decoration-dotted underline-offset-2">
            {stats.hook_threshold.toFixed(2)}
          </button>
          .
        </p>
      )}
    </Panel>
  );
}

/// `stats` is owned by `App`, which already fetches it for the header. Fetching
/// it here as well meant two `/api/stats` calls — two store opens — on every
/// page load for one payload, and two places deciding what a failed load looks
/// like. `err` is App's fetch failure, passed down.
export default function HealthTab({
  stats,
  err,
  active,
  onOpenSetting,
  onOpenMemories,
  onIndexed,
  onStatsChanged,
}: {
  stats: Stats | null;
  err: string | null;
  active: boolean;
  onOpenSetting: (key: string) => void;
  onOpenMemories: () => void;
  onIndexed: () => void;
  onStatsChanged: () => void;
}) {
  const [prog, setProg] = useState<Progress>({ idle: true });
  const wasIndexing = useRef(false);
  const poll = () => getProgress().then(setProg).catch(() => {});
  useEffect(() => {
    // This tab stays mounted once visited, so the poll has to stop itself when
    // the tab is not on screen — otherwise first visit means polling for the
    // life of the page.
    if (!active) return;
    // The 2s progress poll is secondary and self-healing (a failed tick just
    // tries again next cycle), so it stays silent on a single miss; the stats
    // load, by contrast, must be able to say it failed rather than leave this
    // tab reading "Loading…" forever with no way to tell failure from slow.
    poll();
    const t = setInterval(poll, 2000);
    return () => clearInterval(t);
  }, [active]);
  useEffect(() => {
    if (wasIndexing.current && prog.idle) onIndexed();
    wasIndexing.current = !prog.idle;
  }, [prog.idle]);
  if (err) return <div className="m-4"><ErrorBanner>{err}</ErrorBanner></div>;
  if (!stats) return <p className="text-muted p-4">Loading…</p>;
  return (
    <div className="flex flex-col gap-3.5 h-full overflow-auto pb-4">
      <VersionCard active={active} indexing={!prog.idle} />
      <InstallCard active={active} onOpenSetting={onOpenSetting} />
      <div className="grid gap-3.5 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
        <IndexPanel stats={stats} prog={prog} onIndexStarted={poll} />
        <MemoryPanel stats={stats} onOpenMemories={onOpenMemories} />
      </div>
      <RetrievalPanel stats={stats} onOpenSetting={onOpenSetting} />
      <SkippedPanel skipped={stats.skipped} hidden={stats.skipped_ignored ?? 0} onChanged={onStatsChanged} onOpenSetting={onOpenSetting} />
    </div>
  );
}
