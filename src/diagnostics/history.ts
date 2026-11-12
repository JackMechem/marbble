// The last minute of the Diagnostics window's snapshots, for its graphs: the engine's load, each
// thread's share of the buffer, and whether buffers ran late since the snapshot before.
import { threadShares } from "./facts.ts";
import type { Diagnostics } from "./snapshot.ts";

/** How far back the graphs reach. */
export const HISTORY_MS = 60_000;

export interface Sample {
  /** `performance.now()` when the snapshot came. */
  at: number;
  /** The CPU meter's load, and its recent peak. */
  load: number;
  peak: number;
  /** Buffers that ran late since the sample before, and since launch. */
  late: number;
  lateTotal: number;
  /** Each thread's share of the latest buffer, by name ("Audio", "DSP 1"). */
  threads: Record<string, number>;
}

/** `history` with a sample of `d` taken at `at`, dropping those older than `HISTORY_MS`. */
export function addSample(history: Sample[], d: Diagnostics, at: number): Sample[] {
  const prev = history[history.length - 1];
  const lateTotal = d.load.late_blocks;
  const late = prev ? Math.max(0, lateTotal - prev.lateTotal) : 0;
  const sample: Sample = { at, load: d.load.cpu, peak: d.load.cpu_peak, late, lateTotal, threads: Object.fromEntries(threadShares(d)) };
  return [...history.filter((s) => at - s.at <= HISTORY_MS), sample];
}

/** The threads that played in the last minute, in order: Audio first, then DSP 1, DSP 2… */
export function threadNames(history: Sample[]): string[] {
  const names = new Set<string>();
  for (const s of history) for (const n of Object.keys(s.threads)) names.add(n);
  const order = (n: string) => (n === "Audio" ? 0 : Number(n.replace(/\D/g, "")) || 0);
  return [...names].sort((a, b) => order(a) - order(b) || a.localeCompare(b));
}
