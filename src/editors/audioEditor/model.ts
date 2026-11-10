// The Audio Track Editor's layout and what a press grabs, with no screen in them. Tested in
// tests/editors/audioEditor/model.test.ts; the range arithmetic itself is in range.ts.
import type { Id, Project } from "../../engine/project";
import type { Region } from "../../engine/regions";
import type { RegionCrossfades } from "../../tracks/regions/fades.ts";
import { fadeHandles } from "../../tracks/regions/fades.ts";
import { FADE_ZONE } from "../../tracks/regions/regionBar.ts";

/** Ruler height, with the cycle strip at its top. */
export const RULER = 24;
export const CYCLE_STRIP = 7;
/** Width of the dB scale column. */
export const SCALE_W = 38;
/** Region name bar, as drawRegion draws it. */
export const HEADER = 14;
export const LANE_PAD = 3;
/** Closest zoom: past this the 32-frame overview only interpolates. */
export const MAX_PPS = 0.5;
/** Keeps the scrolled width well inside what the browser can lay out. */
export const MAX_CONTENT_PX = 8_000_000;
export const DB_MARKS = [0, -1, -2, -3, -4.5, -6, -9, -12, -15, -18, -24, -30, -36, -42, -48, -60];

/** Vertical layout for a canvas `H` high: the lane, and the waveform body inside regions. */
export function geometry(H: number) {
  const top = RULER + LANE_PAD;
  const h = Math.max(HEADER + 10, H - RULER - 2 * LANE_PAD);
  return { top, h, bodyTop: top + HEADER + 2, bodyH: h - HEADER - 4 };
}

/** How long the scrollable timeline is, like the arrange area: the regions plus a minute. */
export function timelineFrames(p: Project): number {
  return Math.max(p.sample_rate * 60 * 6, ...p.tracks.flatMap((t) => t.regions.map((r) => r.start + r.length + p.sample_rate * 60)));
}

export const dbGain = (db: number) => (db <= -96 ? 0 : Math.pow(10, db / 20));

/** First index in sorted `list` whose value is at least `v`. */
export function lowerBound(list: number[], v: number): number {
  let lo = 0;
  let hi = list.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if (list[m] < v) lo = m + 1;
    else hi = m;
  }
  return lo;
}

/** The zoom range, in pixels per sample, for a view `W` wide: from the whole timeline fitting to the
 *  closest the overview and the browser's layout allow. */
export function zoomBounds(W: number, t: number) {
  return [W / t, Math.min(MAX_PPS, MAX_CONTENT_PX / t)] as const;
}

export type GrabKind = "move" | "left" | "right" | "fadeIn" | "fadeOut" | "select";

export interface Drag {
  kind: Exclude<GrabKind, "select">;
  id: Id;
  start?: number;
  length?: number;
  fade?: number;
}

/** A press on the lane: content x, y in the canvas, the layout, and the region under it. */
export interface AudioPress {
  region?: Region;
  x: number;
  y: number;
  g: ReturnType<typeof geometry>;
}

  /** What a press grabs: the name bar moves the region, the band at the top of the waveform
   *  holds the fade handles, the edges trim, and the rest selects a range. */
export function grabKind(h: AudioPress, pps: number, crossfades: (id: Id) => RegionCrossfades | undefined): GrabKind {
  const r = h.region;
  if (!r) return "select";
  const rx = r.start * pps;
  const rw = r.length * pps;
  if (h.y - h.g.top < HEADER + 1) return "move";
  const edge = Math.min(8, rw / 4);
  const inBody = h.y - h.g.bodyTop;
  if (r.content.type === "audio" && inBody >= 0 && inBody < FADE_ZONE) {
    // As in the arrange: a crossfade's handle is at its start, and replaces the fade-in.
    const hd = fadeHandles(r.start, r.length, r.content.fade_in, r.content.fade_out, crossfades(r.id));
    const fiX = rx + r.content.fade_in * pps;
    const foX = rx + hd.fadeOut * pps;
    const near = Math.min(7, rw / 4);
    if (hd.fadeIn !== null && (Math.abs(h.x - fiX) <= near || (r.content.fade_in === 0 && h.x < rx + edge))) return "fadeIn";
    if (Math.abs(h.x - foX) <= near || (r.content.fade_out === 0 && h.x > rx + rw - edge)) return "fadeOut";
  }
  if (h.x > rx + rw - edge) return "right";
  if (h.x < rx + edge) return "left";
  return "select";
}
