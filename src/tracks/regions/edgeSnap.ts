// Moving regions onto the edges of others, as with Logic's alignment guides: while a selection is
// dragged, a start or end of any moved region that comes within a few pixels of another region's
// start or end (on any track), the stopped playhead or a cycle locator lands exactly on it, and
// the Tracks area draws a guide line there. It wins over the grid. Pure, tested in
// tests/tracks/regions/edgeSnap.test.ts; tracks/canvas/pointer.ts drives it.
import type { Id, Track } from "../../engine/project";
import { regionEnd } from "./loops.ts";

/** How near (px) an edge comes to a target before it snaps. */
export const EDGE_SNAP_PX = 8;

/** A move that snapped: where the grabbed region goes, and the position it snapped to. */
export interface EdgeSnap {
  to: number;
  at: number;
}

/** The starts and ends (loop ends for looping regions) of the regions `which` picks, on every
 *  track. */
export function regionEdges(tracks: Pick<Track, "regions">[], which: (id: Id) => boolean): number[] {
  return tracks.flatMap((t) => t.regions.filter((r) => which(r.id)).flatMap((r) => [r.start, regionEnd(r)]));
}

/** Positions to snap to, sorted and without repeats. */
export function edgeTargets(positions: number[]): number[] {
  return [...new Set(positions)].sort((a, b) => a - b);
}

/** The moved regions' edges (`edges`, from regionEdges) as distances from the grabbed region's
 *  start `from`. */
export function movingEdges(edges: number[], from: number): number[] {
  return [...new Set(edges)].map((x) => x - from);
}

/** The target in `sorted` nearest `x`. */
function nearest(sorted: number[], x: number): number | undefined {
  let lo = 0;
  let hi = sorted.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (sorted[mid] < x) lo = mid + 1;
    else hi = mid;
  }
  const a = sorted[lo - 1];
  const b = sorted[lo];
  if (a === undefined) return b;
  if (b === undefined) return a;
  return x - a <= b - x ? a : b;
}

/** With the grabbed region dragged to `raw` (unsnapped), the moved edge nearest a target
 *  within `reach` (samples) put on it; null when none is that near, or when the grabbed region
 *  would go before the project's start. */
export function edgeSnap(raw: number, edges: number[], targets: number[], reach: number): EdgeSnap | null {
  let best: EdgeSnap | null = null;
  let bestD = reach;
  for (const off of edges) {
    const at = nearest(targets, raw + off);
    if (at === undefined || at - off < 0) continue;
    const d = Math.abs(raw + off - at);
    if (d < bestD || (d === bestD && !best)) {
      best = { to: at - off, at };
      bestD = d;
    }
  }
  return best;
}
