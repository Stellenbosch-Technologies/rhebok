import { useEffect, useState, type ReactElement } from "react";
import { describeError, getVersion, startRestart, startUpdate, VersionInfo } from "./api";
import { ago } from "./components";

const RESTART_WAIT_MS = 30_000;

export default function VersionCard({ active, indexing }: { active: boolean; indexing: boolean }) {
  const [v, setV] = useState<VersionInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [restarting, setRestarting] = useState(false);
  useEffect(() => {
    if (!active) return;
    const tick = () => getVersion().then(setV).catch(() => {});
    tick();
    const t = setInterval(tick, 2000);
    return () => clearInterval(t);
  }, [active]);

  const check = () => {
    setBusy(true);
    setErr(null);
    getVersion(true).then(setV).catch((e) => setErr(describeError(e))).finally(() => setBusy(false));
  };
  const update = () => {
    setBusy(true);
    setErr(null);
    startUpdate().then(() => getVersion().then(setV)).catch((e) => setErr(describeError(e))).finally(() => setBusy(false));
  };

  const restart = () => {
    setConfirming(false);
    setBusy(true);
    setErr(null);
    startRestart()
      .then(({ dashboard_pid }) => {
        setRestarting(true);
        const deadline = Date.now() + RESTART_WAIT_MS;
        const retry = () => {
          if (Date.now() < deadline) return void setTimeout(wait, 500);
          setRestarting(false);
          setBusy(false);
          setErr("The dashboard did not come back. See db.log beside the index, or run rhebok restart.");
        };
        const wait = () =>
          getVersion()
            .then((now) => (now.pid !== dashboard_pid ? window.location.reload() : retry()))
            .catch(retry);
        wait();
      })
      .catch((e) => {
        setErr(e instanceof Error ? e.message : describeError(e));
        setBusy(false);
      });
  };
  const askToRestart = () => (indexing ? setConfirming(true) : restart());

  if (!v) return null;
  const u = v.update;
  const running = u && !u.done;
  const btn = "px-3 py-1 rounded-md text-sm font-medium border border-line disabled:opacity-40";
  let body: ReactElement;
  if (restarting) {
    body = <span className="text-sm">Restarting the dashboard{indexing && " and the index"}… this page reloads when the new one answers.</span>;
  } else if (confirming) {
    body = <span className="text-sm text-warn">An index is running. Restart stops it and starts it again: the work it has not yet published is lost, and the live index is unchanged.</span>;
  } else if (running) {
    body = <span className="text-sm">Updating to {u.to ?? "…"}: <span className="font-mono">{u.phase}</span> {u.message && <span className="text-muted">— {u.message}</span>}</span>;
  } else if (u && u.done && u.ok && u.to) {
    body = <span className="text-sm">Updated to <span className="font-mono">{u.to}</span>. Restart this dashboard so it serves the new version, and restart Claude Code.</span>;
  } else if (u && u.done && u.ok === false) {
    body = <span className="text-sm text-warn">Update failed: {u.message}. See db.log beside the index.</span>;
  } else if (v.update_available) {
    body = (
      <span className="text-sm">
        <span className="font-mono">{v.update_available}</span> available, you have <span className="font-mono">{v.installed}</span>
        {v.url && <> · <a className="text-accent underline" href={v.url} target="_blank" rel="noreferrer">release notes</a></>}
        {v.error && <span className="text-warn"> · last check failed {ago(v.checked_at)}: {v.error}</span>}
      </span>
    );
  } else if (v.error) {
    body = <span className="text-sm text-warn">update check failed {ago(v.checked_at)}: {v.error}</span>;
  } else if (v.no_release) {
    body = <span className="text-sm text-muted">no release published yet · checked {ago(v.checked_at)}</span>;
  } else {
    body = <span className="text-sm text-muted">up to date · checked {ago(v.checked_at)}</span>;
  }
  const updateSucceeded = !!(u && u.done && u.ok && u.to);
  const showUpdate = !updateSucceeded && !running && (v.update_available || (u && u.done && u.ok === false));
  const showCheck = !updateSucceeded && !running && !restarting && !confirming;
  const showRestart = !running && !restarting && !confirming;
  return (
    <div data-testid="version-card" className="bg-surface border border-line rounded-lg px-4 py-3 flex items-center gap-3.5">
      <span className="text-[11px] font-semibold uppercase tracking-wider text-muted">rhebok</span>
      <span className="font-mono text-sm">{v.installed}</span>
      <div className="grow">{body}</div>
      {err && <span className="text-xs text-warn">{err}</span>}
      {indexing && !running && !confirming && !restarting && <span className="text-xs text-muted">an index is running</span>}
      {confirming && <button className={`${btn} text-warn border-warn`} disabled={busy} onClick={restart}>Stop the index and restart</button>}
      {confirming && <button className={btn} disabled={busy} onClick={() => setConfirming(false)}>Cancel</button>}
      {showRestart && <button className={updateSucceeded ? `${btn} bg-accent text-white border-accent` : btn} disabled={busy} onClick={askToRestart}>Restart</button>}
      {showUpdate && !restarting && !confirming && <button className={`${btn} bg-accent text-white border-accent`} disabled={busy || indexing} onClick={update}>Update</button>}
      {showCheck && <button className={btn} disabled={busy} onClick={check}>Check now</button>}
    </div>
  );
}
