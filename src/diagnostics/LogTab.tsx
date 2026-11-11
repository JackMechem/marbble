// The Diagnostics window's Log: Marbble's log as it was written to the terminal, newest at the
// bottom, with a level filter, a search box and the choice to keep the newest line in view.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { filterLines, levelCounts, LEVELS, shortTarget } from "./logLines";
import { sinceLaunch } from "./format";
import type { LogLevel, LogLine } from "./snapshot";
import { Select } from "../ui/Form";
import { Icon } from "../ui/icons";

const LEVEL_NAMES: Record<LogLevel, string> = { error: "Errors", warn: "Warnings and up", info: "Info and up", debug: "Debug and up", trace: "Everything" };

export function LogTab({ lines }: { lines: LogLine[] }) {
  const [level, setLevel] = useState<LogLevel>("trace");
  const [query, setQuery] = useState("");
  const [follow, setFollow] = useState(true);
  const list = useRef<HTMLDivElement>(null);
  const shown = filterLines(lines, level, query);
  const counts = levelCounts(lines);

  // Keep the newest line in view while following.
  useLayoutEffect(() => {
    const el = list.current;
    if (el && follow) el.scrollTop = el.scrollHeight;
  }, [shown.length, follow, lines]);

  // Scrolling up stops following; back at the bottom starts it again.
  useEffect(() => {
    const el = list.current;
    if (!el) return;
    const onScroll = () => setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 4);
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <div className="diag-log">
      <div className="diag-toolbar">
        <Select value={level} options={LEVELS.map((l) => [l, LEVEL_NAMES[l]])} onChange={setLevel} width={150} title="Show lines at this level and more severe" />
        <input className="search diag-search" placeholder="Search the log" aria-label="Search the log" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
        <button type="button" className={"diag-btn" + (follow ? " on" : "")} aria-pressed={follow} title="Keep the newest line in view" aria-label="Follow the newest line" onClick={() => setFollow(!follow)}>
          <Icon.follow />
        </button>
        <span className="diag-note">
          {shown.length === lines.length ? `${lines.length} lines` : `${shown.length} of ${lines.length} lines`}
          {counts.error ? <span className="diag-count alert">{counts.error} errors</span> : null}
          {counts.warn ? <span className="diag-count warn">{counts.warn} warnings</span> : null}
        </span>
      </div>
      <div className="diag-log-lines" ref={list} role="log" aria-live="off">
        {shown.length === 0 ? <div className="diag-empty">{lines.length ? "No lines match." : "Nothing logged yet."}</div> : null}
        {shown.map((l) => (
          <div key={l.seq} className={`diag-line ${l.level}`}>
            <span className="diag-line-time">{sinceLaunch(l.ms)}</span>
            <span className="diag-line-level">{l.level}</span>
            <span className="diag-line-target" title={l.target}>
              {shortTarget(l.target)}
            </span>
            <span className="diag-line-message">{l.message}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
