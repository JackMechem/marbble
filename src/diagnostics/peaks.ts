// Each track's peak share of its buffer, held for a moment as the CPU meter's peak is: the engine
// reports the worst since the last snapshot, four times a second, which alone would flicker and
// reorder a table sorted by it.
import type { Id } from "../engine/project.ts";
import type { TrackDiagnostics } from "./snapshot.ts";

/** How long a peak stays before a lower one replaces it. */
export const PEAK_HOLD_MS = 2000;

export type HeldPeaks = Map<Id, { peak: number; at: number }>;

/** `held` after a snapshot taken at `at`: a track's peak rises at once and falls once it's been
 *  held `PEAK_HOLD_MS`. Tracks no longer there are dropped. */
export function holdPeaks(held: HeldPeaks, tracks: TrackDiagnostics[], at: number): HeldPeaks {
  const out: HeldPeaks = new Map();
  for (const t of tracks) {
    const h = held.get(t.id);
    out.set(t.id, !h || t.peak >= h.peak || at - h.at >= PEAK_HOLD_MS ? { peak: t.peak, at } : h);
  }
  return out;
}
