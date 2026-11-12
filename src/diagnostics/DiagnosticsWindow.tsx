// The Diagnostics window (Window › Diagnostics; main.tsx renders this in the `diagnostics` window),
// laid out as a system task manager: a sidebar of pages (Performance, Tracks, Log) with the
// window's actions, the page, and the headline figures in a status bar. It shows how the device,
// the engine, its threads and the tracks are doing, live, with what you did and Marbble's log. It
// asks the app for a snapshot four times a second while open (src-tauri/src/diagnostics/), keeps
// the log lines it was sent and the last minute for the graphs (history.ts), and counts what went
// wrong from its last reset. The figures are worded in facts.ts, the copied report in report.ts.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { countersOf, noteRises, type Counters, type Rises } from "./counters";
import { countsAt, headline } from "./facts";
import { addLines, lastSeq } from "./logLines";
import { buildReport } from "./report";
import type { Diagnostics, DiagnosticsSnapshot, LogLine } from "./snapshot";
import { PerformanceTab } from "./PerformanceTab";
import { TracksTab } from "./TracksTab";
import { addSample, type Sample } from "./history";
import { pct } from "./format";
import { LogTab } from "./LogTab";
import { FactChip } from "./FactViews";
import { copyText } from "./copyText";
import type { ActivityStatus } from "./activity";
import { holdPeaks, type HeldPeaks } from "./peaks";
import { Icon } from "../ui/icons";
import { MenuHost } from "../ui/Menu";
import { applyTheme, savedThemeId } from "../theme/themes";
import { applyUiScale, savedUiScale } from "../ui/uiScale";
import { getState } from "../state/store";
import { textFieldMenu } from "../keyboard/textKeys";

/** How often the window asks for a snapshot. */
const POLL_MS = 250;
const TABS = [
  ["performance", "Performance", Icon.graph],
  ["tracks", "Tracks", Icon.rows],
  ["log", "Log", Icon.lines],
] as const;
type Tab = (typeof TABS)[number][0];
const TAB_KEY = "marbble.diagnostics.tab";

function savedTab(): Tab {
  try {
    const t = localStorage.getItem(TAB_KEY);
    return TABS.some(([id]) => id === t) ? (t as Tab) : "performance";
  } catch {
    return "performance";
  }
}

/** What a page's entry in the sidebar says under its name. */
function tabNote(tab: Tab, engine: Diagnostics | null, log: LogLine[]): { text: string; level?: "warn" | "alert" } | null {
  if (tab === "performance") return engine ? { text: `Load ${pct(engine.load.cpu)}`, level: engine.load.cpu > 1 ? "alert" : engine.load.cpu > 0.7 ? "warn" : undefined } : null;
  if (tab === "tracks") {
    if (!engine) return null;
    const asleep = engine.tracks.reduce((n, t) => n + (t.off ? 0 : t.asleep), 0);
    return { text: `${engine.tracks.length} tracks${asleep ? ` · ${asleep} asleep` : ""}` };
  }
  const errors = log.filter((l) => l.level === "error").length;
  const warnings = log.filter((l) => l.level === "warn").length;
  return errors ? { text: `${errors} errors`, level: "alert" } : warnings ? { text: `${warnings} warnings`, level: "warn" } : { text: `${log.length} lines` };
}

/** Menu bar items come here while this window is in front: a focused search field takes its
 *  edits (Select All, Copy…), and the rest go on to the main window, as from a panel window. */
function useMenuItems() {
  useEffect(() => {
    const off = getCurrentWebviewWindow().listen<string>("menu", (e) => {
      if (!textFieldMenu(e.payload)) emitTo("main", "menu", e.payload).catch(() => {});
    });
    return () => {
      off.then((f) => f()).catch(() => {});
    };
  }, []);
}

/** The theme and interface size follow the main window's, which saves them where this window can
 *  hear it change. */
function useFollowTheme() {
  useEffect(() => {
    const onStorage = () => {
      const theme = savedThemeId();
      if (theme !== getState().themeId) applyTheme(theme, false);
      const scale = savedUiScale();
      if (scale !== getState().uiScale) void applyUiScale(scale, false);
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
}

export function DiagnosticsWindow() {
  const [about, setAbout] = useState({ version: "", os: "", arch: "" });
  const [engine, setEngine] = useState<Diagnostics | null>(null);
  const [held, setHeld] = useState<HeldPeaks>(() => new Map());
  const [history, setHistory] = useState<Sample[]>([]);
  const [log, setLog] = useState<LogLine[]>([]);
  const [base, setBase] = useState<{ counters: Counters; at: string } | null>(null);
  const [rises, setRises] = useState<Rises>({});
  const [now, setNow] = useState(() => performance.now());
  const [tab, setTab] = useState<Tab>(savedTab);
  const [copied, setCopied] = useState(false);
  const [activity, setActivity] = useState<ActivityStatus | null>(null);
  const [activityError, setActivityError] = useState<string | null>(null);
  const logRef = useRef<LogLine[]>([]);
  const prevCounters = useRef<Counters | null>(null);
  useFollowTheme();
  useMenuItems();

  useEffect(() => {
    document.title = "Diagnostics";
    let busy = false;
    let live = true;
    const poll = async () => {
      if (busy) return;
      busy = true;
      try {
        const s = await invoke<DiagnosticsSnapshot>("diagnostics", { logAfter: lastSeq(logRef.current) });
        if (!live) return;
        const at = performance.now();
        setAbout((a) => (a.version === s.version ? a : { version: s.version, os: s.os, arch: s.arch }));
        if (s.log.length) {
          logRef.current = addLines(logRef.current, s.log);
          setLog(logRef.current);
        }
        if (s.engine) {
          const c = countersOf(s.engine);
          const prev = prevCounters.current;
          setRises((r) => noteRises(r, prev, c, at));
          prevCounters.current = c;
          const tracks = s.engine.tracks;
          setHeld((h) => holdPeaks(h, tracks, at));
          const e = s.engine;
          setHistory((h) => addSample(h, e, at));
          setEngine(e);
        }
        setNow(at);
      } catch {
        // The app is closing, or the session is busy with a render: try again next time.
      } finally {
        busy = false;
      }
    };
    void poll();
    const id = window.setInterval(poll, POLL_MS);
    return () => {
      live = false;
      window.clearInterval(id);
    };
  }, []);

  useEffect(() => {
    invoke<ActivityStatus>("activity_log").then(setActivity).catch(() => {});
  }, []);

  const toggleRecording = useCallback(async () => {
    try {
      setActivity(await invoke<ActivityStatus>("set_activity_log", { on: !activity?.on }));
      setActivityError(null);
    } catch (e) {
      // Couldn't start a file there.
      setActivityError(String(e));
      invoke<ActivityStatus>("activity_log").then(setActivity).catch(() => {});
    }
  }, [activity]);

  const recordTitle = activityError
    ? activityError
    : activity?.on
      ? `Recording everything to ${activity.file ?? activity.folder}. Click to stop`
      : "Record everything: what you do, every audio gap and the log, to a file, from now and from each launch";

  const counts = useMemo(() => (engine ? countsAt(countersOf(engine), base?.counters ?? null, rises, now) : null), [engine, base, rises, now]);

  const reset = useCallback(() => {
    if (!engine) return;
    setBase({ counters: countersOf(engine), at: new Date().toLocaleTimeString() });
    setRises({});
    invoke("reset_diagnostics").catch(() => {});
  }, [engine]);

  const copy = useCallback(async () => {
    const text = buildReport({ ...about, taken: new Date().toLocaleString(), resetAt: base?.at ?? null }, engine, counts, logRef.current, held);
    if (await copyText(text)) {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    }
  }, [about, base, engine, counts, held]);

  const choose = (t: Tab) => {
    setTab(t);
    try {
      localStorage.setItem(TAB_KEY, t);
    } catch {
      // Remembering the tab is a nicety.
    }
  };

  return (
    <div className="diag">
      <aside className="diag-side">
        <nav className="diag-pages" role="tablist" aria-label="Diagnostics" aria-orientation="vertical">
          {TABS.map(([id, label, Glyph]) => {
            const note = tabNote(id, engine, log);
            return (
              <button key={id} type="button" role="tab" aria-selected={tab === id} className={"diag-page" + (tab === id ? " on" : "")} onClick={() => choose(id)}>
                <span className="diag-page-icon">
                  <Glyph />
                </span>
                <span className="diag-page-text">
                  <span className="diag-page-name">{label}</span>
                  {note ? <span className={"diag-page-note" + (note.level ? ` ${note.level}` : "")}>{note.text}</span> : null}
                </span>
              </button>
            );
          })}
        </nav>
        <div className="diag-actions">
          <button type="button" className={"diag-btn" + (activity?.on ? " rec" : "")} title={recordTitle} aria-label="Record everything" aria-pressed={activity?.on ?? false} onClick={toggleRecording} disabled={!activity}>
            <Icon.record />
          </button>
          <button type="button" className="diag-btn" title={activity ? `Show the recorded files (${activity.folder})` : "Show the recorded files"} aria-label="Show recorded files" onClick={() => invoke("show_activity_logs").catch(() => {})}>
            <Icon.folder />
          </button>
          <button type="button" className="diag-btn" title={base ? `Counting since ${base.at}. Start the counts and load figures afresh` : "Start the counts and load figures afresh"} aria-label="Reset counts" onClick={reset} disabled={!engine}>
            <Icon.reset />
          </button>
          <button type="button" className={"diag-btn" + (copied ? " done" : "")} title={copied ? "Copied" : "Copy everything as a text report, to paste into an issue"} aria-label="Copy report" onClick={copy}>
            <Icon.copy />
          </button>
        </div>
      </aside>
      <main className="diag-body" role="tabpanel">
        {!engine && tab !== "log" ? <div className="diag-empty">Waiting for the engine…</div> : null}
        {engine && counts && tab === "performance" ? <PerformanceTab d={engine} counts={counts} history={history} now={now} /> : null}
        {engine && tab === "tracks" ? <TracksTab d={engine} held={held} /> : null}
        {tab === "log" ? <LogTab lines={log} /> : null}
      </main>
      <footer className="diag-status">{engine && counts ? headline(engine, counts).map((f) => <FactChip key={f.label} fact={f} />) : <span className="diag-waiting">Waiting for the engine…</span>}</footer>
      <MenuHost />
    </div>
  );
}
