import { useEffect, useMemo, useState } from "react";
import { getConfig, previewIgnore, saveConfig, type IgnorePreview, type Skip, type SkipKind } from "./api";
import { Panel } from "./components";
import { primaryButton, secondaryButton } from "./integrations/agents";

const KINDS: Record<SkipKind, { title: string; explain: string }> = {
  empty: { title: "Empty sessions", explain: "These session files hold no messages, so there is nothing to index. Ignoring them loses nothing." },
  settling: { title: "Still being written", explain: "A session is still adding to these. The first index after ten quiet minutes reads them." },
  unreadable: { title: "Could not be read", explain: "The operating system refused to open these files. The error code says why." },
  scanned: { title: "Scanned PDFs", explain: "These PDFs have no text layer. Turn on OCR in Settings to read them." },
  failed: { title: "Could not be indexed", explain: "rhebok opened these files but could not turn them into documents. The detail names the problem." },
};

const ORDER: SkipKind[] = ["unreadable", "failed", "scanned", "settling", "empty"];

function baseName(path: string) {
  return path.slice(path.lastIndexOf("/") + 1);
}

function parentOf(path: string) {
  return path.slice(0, Math.max(1, path.lastIndexOf("/")));
}

function homeOf(path: string) {
  return path.match(/^(\/Users\/[^/]+|\/home\/[^/]+)/)?.[1] ?? null;
}

function tilde(path: string) {
  const home = homeOf(path);
  return home ? `~${path.slice(home.length)}` : path;
}

function commonFolder(paths: string[]) {
  const split = paths.map((p) => parentOf(p).split("/"));
  const first = split[0] ?? [];
  let n = 0;
  while (n < first.length && split.every((s) => s[n] === first[n])) n++;
  return first.slice(0, n).join("/");
}

function shortPath(path: string): { project: string | null; shown: string } {
  const home = homeOf(path);
  const rel = tilde(path);
  const session = rel.match(/^~\/\.claude\/projects\/([^/]+)\/(.*)$/);
  if (session && home) {
    const encodedHome = home.replace(/\//g, "-");
    const dir = session[1].startsWith(encodedHome) ? session[1].slice(encodedHome.length + 1) : session[1];
    return { project: dir || "home", shown: session[2] };
  }
  return { project: null, shown: rel };
}

type Suggestion = { label: string; pattern: string };

function suggestionsFor(items: Skip[]): Suggestion[] {
  const out: Suggestion[] = [];
  if (items.length === 1) {
    out.push({ label: "this file", pattern: tilde(items[0].path) });
    out.push({ label: "its folder", pattern: tilde(parentOf(items[0].path)) });
  }
  const names = new Map<string, number>();
  for (const s of items) names.set(baseName(s.path), (names.get(baseName(s.path)) ?? 0) + 1);
  const [name, count] = [...names.entries()].sort((a, b) => b[1] - a[1])[0] ?? ["", 0];
  if (name && (count > 1 || items.length === 1)) {
    const workflow = items.some((s) => s.path.includes("/subagents/workflows/") && baseName(s.path) === name);
    if (workflow) out.push({ label: `every workflow ${name}`, pattern: `subagents/workflows/*/${name}` });
    out.push({ label: `every file named ${name}`, pattern: name });
  }
  if (items.length > 1) {
    const folder = commonFolder(items.map((s) => s.path));
    if (folder && folder !== homeOf(folder) && folder.split("/").length > 3) out.push({ label: "their shared folder", pattern: tilde(folder) });
  }
  return out;
}

function IgnoreEditor({ items, onCancel, onAdded }: { items: Skip[]; onCancel: () => void; onAdded: (pattern: string) => void }) {
  const suggestions = useMemo(() => suggestionsFor(items), [items]);
  const [pattern, setPattern] = useState(suggestions[0]?.pattern ?? "");
  const [preview, setPreview] = useState<IgnorePreview | null>(null);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    if (!pattern.trim()) {
      setPreview(null);
      return;
    }
    let live = true;
    const t = setTimeout(() => {
      previewIgnore(pattern.trim()).then((p) => live && setPreview(p)).catch(() => live && setPreview(null));
    }, 250);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [pattern]);
  const add = async () => {
    const entry = pattern.trim();
    setSaving(true);
    setErr(null);
    try {
      for (let attempt = 0; attempt < 2; attempt++) {
        const state = await getConfig();
        const current = state.effective.ignore ?? [];
        if (current.includes(entry)) {
          onAdded(entry);
          return;
        }
        const r = await saveConfig(state.etag, { set: { ignore: [...current, entry] } });
        if (r.kind === "saved") {
          onAdded(entry);
          return;
        }
        if (r.kind === "invalid") {
          setErr(r.errors.map((e) => e.message).join(" ") || r.error);
          return;
        }
      }
      setErr("config.toml changed while saving. Try again.");
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };
  const blocked = !pattern.trim() || !!preview?.error || saving;
  return (
    <div className="bg-sunken/60 rounded-md p-3 flex flex-col gap-2.5 text-sm" data-testid="ignore-editor">
      {suggestions.length > 0 && (
        <div className="flex gap-1.5 flex-wrap items-center">
          <span className="text-muted text-xs">Ignore</span>
          {suggestions.map((s) => (
            <button
              key={s.pattern}
              onClick={() => setPattern(s.pattern)}
              aria-pressed={pattern === s.pattern}
              className={`text-xs rounded px-2 py-0.5 border ${pattern === s.pattern ? "border-accent text-accent bg-surface font-semibold" : "border-line text-ink bg-surface"}`}
            >
              {s.label}
            </button>
          ))}
        </div>
      )}
      <div className="flex gap-2 items-center flex-wrap">
        <input
          value={pattern}
          onChange={(e) => setPattern(e.target.value)}
          aria-label="Ignore entry"
          className="font-mono text-xs bg-surface border border-line rounded-md px-2.5 py-1.5 grow min-w-0 basis-80"
        />
        <button onClick={add} disabled={blocked} className={primaryButton}>{saving ? "Adding…" : "Add to ignore list"}</button>
        <button onClick={onCancel} className={secondaryButton}>Cancel</button>
      </div>
      <span className={`text-xs ${preview?.error ? "text-warn" : "text-muted"}`} data-testid="ignore-preview">
        {preview?.error
          ? preview.error
          : preview
            ? `Hides ${preview.matches} of the ${preview.of} skipped files listed here. Notes and sessions it matches are no longer indexed either.`
            : "A name skips any file or folder with that name. A path skips it and everything under it. A glob such as *.tmp or drafts/**/*.md matches paths."}
      </span>
      {err && <span className="text-xs text-warn">{err}</span>}
    </div>
  );
}

function SkipRow({ item, onIgnore }: { item: Skip; onIgnore: () => void }) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const { project, shown } = shortPath(item.path);
  const code = item.io ? `${item.io.kind}${item.io.os_error != null ? ` · os error ${item.io.os_error}` : ""}` : null;
  return (
    <div className="border-t border-sunken min-w-0">
      <div className="flex items-center gap-3 py-1.5 text-sm min-w-0">
        <button onClick={() => setOpen(!open)} aria-expanded={open} className="flex items-center gap-3 grow min-w-0 text-left" title={item.path}>
          {project && <span className="shrink-0 max-w-[30%] truncate text-xs bg-sunken rounded px-1.5 py-0.5">{project}</span>}
          <span className="font-mono text-xs truncate grow min-w-0 [direction:rtl] text-left"><bdi>{shown}</bdi></span>
          {code && <span className="font-mono text-[11px] text-warn bg-warn-soft rounded px-1.5 py-0.5 shrink-0">{code}</span>}
        </button>
        <button onClick={onIgnore} className="text-xs font-semibold text-accent shrink-0">Ignore…</button>
        <button
          onClick={() => navigator.clipboard.writeText(item.path).then(() => setCopied(true)).catch(() => setCopied(false))}
          className="text-xs font-semibold text-accent shrink-0"
        >
          {copied ? "copied" : "copy path"}
        </button>
      </div>
      {open && (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs pb-2 pl-1">
          <dt className="text-muted">Path</dt>
          <dd className="font-mono break-all">{item.path}</dd>
          <dt className="text-muted">Reason</dt>
          <dd className="font-mono break-words whitespace-pre-wrap">{item.detail || "no detail recorded"}</dd>
          {item.io && (
            <>
              <dt className="text-muted">Error</dt>
              <dd className="font-mono">{item.io.kind}{item.io.os_error != null ? `, os error ${item.io.os_error}` : ""}</dd>
            </>
          )}
        </dl>
      )}
    </div>
  );
}

const SHOWN_PER_GROUP = 50;

type Editing = { key: string; items: Skip[] };

export default function SkippedPanel({
  skipped,
  hidden,
  onChanged,
  onOpenSetting,
}: {
  skipped: Skip[];
  hidden: number;
  onChanged: () => void;
  onOpenSetting: (key: string) => void;
}) {
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState<Set<SkipKind>>(new Set());
  const [whole, setWhole] = useState<Set<SkipKind>>(new Set());
  const [editing, setEditing] = useState<Editing | null>(null);
  const [added, setAdded] = useState<string | null>(null);
  const q = filter.trim().toLowerCase();
  const groups = useMemo(() => {
    const by = new Map<SkipKind, Skip[]>();
    for (const s of skipped) {
      if (q && !`${s.path} ${s.detail} ${s.io?.kind ?? ""}`.toLowerCase().includes(q)) continue;
      by.set(s.kind, [...(by.get(s.kind) ?? []), s]);
    }
    return ORDER.filter((k) => by.has(k)).map((k) => ({ kind: k, items: by.get(k)! }));
  }, [skipped, q]);
  const toggle = (k: SkipKind) =>
    setOpen((s) => {
      const next = new Set(s);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });
  const ignored = (pattern: string) => {
    setEditing(null);
    setAdded(pattern);
    onChanged();
  };
  return (
    <Panel
      title="Skipped files"
      testid="skipped-panel"
      aside={
        <>
          <span className="text-sm text-muted tabular-nums">{skipped.length === 0 ? "none" : `${skipped.length} in the last index run`}</span>
          {hidden > 0 && (
            <button onClick={() => onOpenSetting("ignore")} className="text-sm text-accent underline decoration-dotted underline-offset-2">
              {hidden} more hidden by your ignore list
            </button>
          )}
          <span className="grow" />
          {skipped.length > 0 && (
            <input
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter by path, reason or error"
              aria-label="Filter skipped files"
              className="text-sm bg-ground border border-line rounded-md px-2.5 py-1 w-64 max-w-full focus:outline-2 focus:outline-accent"
            />
          )}
        </>
      }
    >
      {added && (
        <div className="flex items-center gap-3 flex-wrap bg-accent-soft rounded-md px-3.5 py-2 text-sm" data-testid="ignore-added">
          <span className="grow">
            Added <code className="font-mono text-xs">{added}</code> to the ignore list. Matching files are hidden here now, and the next index does not read them.
          </span>
          <button onClick={() => onOpenSetting("ignore")} className={secondaryButton}>Edit the ignore list</button>
          <button onClick={() => setAdded(null)} className="text-xs font-semibold text-accent">dismiss</button>
        </div>
      )}
      {skipped.length === 0 && <p className="text-sm text-muted">The last index run read every file it found{hidden > 0 ? " that is not on your ignore list" : ""}.</p>}
      {skipped.length > 0 && groups.length === 0 && <p className="text-sm text-muted">No skipped file matches “{filter}”.</p>}
      {groups.length > 0 && (
        <div className="flex flex-col gap-2">
          {groups.map((g) => {
            const meta = KINDS[g.kind];
            const expanded = open.has(g.kind) || q !== "";
            const items = whole.has(g.kind) ? g.items : g.items.slice(0, SHOWN_PER_GROUP);
            const editingGroup = editing?.key === `group:${g.kind}`;
            return (
              <div key={g.kind} className="border border-line rounded-md min-w-0" data-skip-group={g.kind}>
                <div className="flex items-center gap-3 px-3 py-2">
                  <button onClick={() => toggle(g.kind)} aria-expanded={expanded} className="flex items-center gap-3 text-left grow min-w-0 rounded focus-visible:outline-2 focus-visible:outline-accent">
                    <span className={`text-muted text-[10px] transition-transform ${expanded ? "rotate-90" : ""}`}>▶</span>
                    <span className="font-semibold text-sm shrink-0">{meta.title}</span>
                    <span className="text-sm tabular-nums bg-sunken rounded px-1.5 shrink-0">{g.items.length}</span>
                    <span className="text-sm text-muted truncate min-w-0">{meta.explain}</span>
                  </button>
                  {g.kind === "scanned" && <button onClick={() => onOpenSetting("pdf.ocr")} className={secondaryButton}>OCR settings</button>}
                  <button onClick={() => setEditing({ key: `group:${g.kind}`, items: g.items })} className={secondaryButton}>
                    {g.items.length === 1 ? "Ignore…" : `Ignore all ${g.items.length}…`}
                  </button>
                </div>
                {editingGroup && (
                  <div className="px-3 pb-3">
                    <IgnoreEditor items={g.items} onCancel={() => setEditing(null)} onAdded={ignored} />
                  </div>
                )}
                {expanded && (
                  <ul className="px-3 pb-2 max-h-96 overflow-auto">
                    {items.map((item) => (
                      <li key={item.path + item.detail}>
                        <SkipRow item={item} onIgnore={() => setEditing({ key: item.path, items: [item] })} />
                        {editing?.key === item.path && (
                          <div className="pb-2">
                            <IgnoreEditor items={[item]} onCancel={() => setEditing(null)} onAdded={ignored} />
                          </div>
                        )}
                      </li>
                    ))}
                    {items.length < g.items.length && (
                      <li className="border-t border-sunken pt-2">
                        <button onClick={() => setWhole((s) => new Set(s).add(g.kind))} className="text-xs font-semibold text-accent">
                          Show all {g.items.length}
                        </button>
                      </li>
                    )}
                  </ul>
                )}
              </div>
            );
          })}
        </div>
      )}
    </Panel>
  );
}
