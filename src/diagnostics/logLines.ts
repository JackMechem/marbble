// The Diagnostics window's copy of the log: the lines it has been sent, kept in order and bounded,
// and the ones its level filter and search let through.
import type { LogLevel, LogLine } from "./snapshot.ts";

/** The most lines the window keeps (the app keeps as many). */
export const KEPT_LINES = 2000;

/** From the most to the least severe. */
export const LEVELS: LogLevel[] = ["error", "warn", "info", "debug", "trace"];

/** `lines` with `fresh` after them: only lines newer than the last one held, at most `KEPT_LINES`. */
export function addLines(lines: LogLine[], fresh: LogLine[], keep = KEPT_LINES): LogLine[] {
  const last = lines.length ? lines[lines.length - 1].seq : 0;
  const newer = fresh.filter((l) => l.seq > last);
  if (!newer.length) return lines;
  const all = lines.concat(newer);
  return all.length > keep ? all.slice(all.length - keep) : all;
}

/** The number of the last line held, to ask for those after it. */
export const lastSeq = (lines: LogLine[]): number => (lines.length ? lines[lines.length - 1].seq : 0);

/** The lines at `level` or more severe whose message or module holds every word of `query`,
 *  ignoring case. */
export function filterLines(lines: LogLine[], level: LogLevel, query: string): LogLine[] {
  const most = LEVELS.indexOf(level);
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return lines.filter((l) => {
    if (LEVELS.indexOf(l.level) > most) return false;
    if (!words.length) return true;
    const text = `${l.message} ${l.target}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

/** How many lines of each level there are. */
export function levelCounts(lines: LogLine[]): Record<LogLevel, number> {
  const out: Record<LogLevel, number> = { error: 0, warn: 0, info: 0, debug: 0, trace: 0 };
  for (const l of lines) out[l.level]++;
  return out;
}

/** A module path without the crate's prefix, for a narrow column: "session::engine::device". */
export const shortTarget = (target: string): string => target.replace(/^(daw_engine|daw|vst3_host)::/, "");
