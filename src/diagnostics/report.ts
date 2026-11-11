// The Diagnostics window's Copy Report: everything it shows as plain text, to paste into an issue.
// The Overview's sections (facts.ts), the Tracks table and the log, with marked values flagged so
// they stand out without colour.
import { overview, threadShares, trackRows, type Counts, type Fact } from "./facts.ts";
import { pct, sinceLaunch } from "./format.ts";
import type { HeldPeaks } from "./peaks.ts";
import type { Diagnostics, LogLine } from "./snapshot.ts";

/** The log lines a report carries at most, the newest. */
export const REPORT_LOG_LINES = 300;

/** What the report is about, besides the session's figures. */
export interface ReportContext {
  version: string;
  os: string;
  arch: string;
  /** When it was taken, as the reader's clock says it. */
  taken: string;
  /** When the window's counts were last reset, if they were. */
  resetAt: string | null;
}

const mark = (f: Fact) => (f.level === "alert" ? "  <-- !!" : f.level === "warn" ? "  <--" : "");

/** Rows of cells as columns, each as wide as its widest cell, the last left ragged. */
export function columns(rows: string[][]): string[] {
  const widths = rows[0].map((_, i) => Math.max(...rows.map((r) => (r[i] ?? "").length)));
  return rows.map((r) =>
    r
      .map((c, i) => (i === r.length - 1 ? c : c.padEnd(widths[i])))
      .join("  ")
      .trimEnd(),
  );
}

/** The report: `counts` as the window shows them, the tracks with the peaks `held` holds. */
export function buildReport(ctx: ReportContext, d: Diagnostics | null, counts: Counts | null, log: LogLine[], held?: HeldPeaks): string {
  const out: string[] = [`Marbble ${ctx.version} diagnostics, ${ctx.os} ${ctx.arch}, taken ${ctx.taken}`];
  if (!d || !counts) {
    out.push("", "(The session hadn't answered yet.)");
  } else {
    if (ctx.resetAt) out.push(`Counts and load figures since ${ctx.resetAt}.`);
    for (const s of overview(d, counts)) {
      out.push("", `== ${s.title} ==`);
      const width = Math.max(...s.facts.map((f) => f.label.length));
      for (const f of s.facts) out.push(`${f.label.padEnd(width)}  ${f.value}${mark(f)}`);
    }
    out.push("", "== Tracks ==");
    const rows = trackRows(d, held);
    if (!rows.length) out.push("(none)");
    else {
      const head = ["Track", "Kind", "Thread", "Share", "Peak", "Latency", "Delay", "State", "Plug-ins"];
      const body = rows.map((r) => [r.name + (r.level === "alert" ? " !!" : r.level === "warn" ? " !" : ""), r.kind, r.thread, r.share, r.peak, r.latency, r.delay, r.state || "-", r.plugins]);
      out.push(...columns([head, ...body]));
      const shares = threadShares(d);
      if (shares.length) out.push("", `Share of the latest buffer by thread: ${shares.map(([t, s]) => `${t} ${pct(s, 1)}`).join(", ")}`);
    }
  }
  const tail = log.slice(-REPORT_LOG_LINES);
  out.push("", `== Log (${tail.length < log.length ? `the last ${tail.length} of ${log.length} lines` : `${log.length} lines`}) ==`);
  for (const l of tail) out.push(`${sinceLaunch(l.ms)} ${l.level.toUpperCase().padEnd(5)} ${l.target}: ${l.message}`);
  return out.join("\n") + "\n";
}
