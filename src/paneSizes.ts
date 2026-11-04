// Pane sizes: their defaults and limits, reading the remembered ones, and how much of the
// window's height the bottom section may take. Pure, so it can be unit tested; the dividers
// themselves are components/Splitter.tsx.
import type { PaneSizes } from "./store";

export type PaneKey = keyof PaneSizes;

export const PANE_DEFAULTS: PaneSizes = { library: 230, inspector: 222, browser: 230, mixer: 560, editor: 360, headers: 296, fx: 420 };

/** Limits per pane: [min, max] in CSS pixels. The bottom panes are also kept within the window
 *  (`bottomMax`). */
export const PANE_LIMITS: Record<PaneKey, [number, number]> = {
  library: [170, 480],
  inspector: [200, 460],
  browser: [180, 520],
  mixer: [180, 900],
  editor: [140, 900],
  headers: [220, 520],
  fx: [160, 900],
};

/** A pane's size within its limits, and within `max` (the room there is) when given: the room
 *  wins over the pane's own minimum. */
export function clampPane(k: PaneKey, v: number, max = Infinity): number {
  const [lo, hi] = PANE_LIMITS[k];
  const within = Math.min(hi, Math.max(lo, Number.isFinite(v) ? v : PANE_DEFAULTS[k]));
  return Math.round(Math.max(0, Math.min(max, within)));
}

/** The remembered sizes (JSON from localStorage), each within its limits; anything missing or
 *  malformed is the default. */
export function parsePaneSizes(text: string | null): PaneSizes {
  const sizes = { ...PANE_DEFAULTS };
  let v: unknown = null;
  try {
    v = JSON.parse(text ?? "null");
  } catch {
    return sizes;
  }
  if (!v || typeof v !== "object") return sizes;
  for (const k of Object.keys(PANE_DEFAULTS) as PaneKey[]) {
    const n = (v as Record<string, unknown>)[k];
    if (typeof n === "number" && Number.isFinite(n)) sizes[k] = clampPane(k, n);
  }
  return sizes;
}

/** The Tracks area's local menu bar. */
export const LOCAL_BAR_H = 28;
/** The track rows the Tracks area keeps at the least, so a track or two stays in view. */
export const MIN_TRACK_ROWS_H = 48;

/**
 * The least height the Tracks area keeps beside the bottom section: its menu bar, the rows of
 * fixed height under it (`fixedRows`: the ruler, the global tracks and the overview) and a
 * little of the track list. The bottom section's divider sits right under it, so it stays in
 * reach however tall the section was made.
 */
export function tracksAreaMin(fixedRows: number): number {
  return LOCAL_BAR_H + Math.max(0, fixedRows) + MIN_TRACK_ROWS_H;
}

/** The most the bottom section can take of a centre column `centerH` tall, leaving the Tracks
 *  area `tracksMin`. A pane remembered taller shows at this height (the stylesheet shrinks it),
 *  and a drag of its divider starts from there. */
export function bottomMax(centerH: number, tracksMin: number): number {
  return Math.max(0, Math.round(centerH - tracksMin));
}

/** Which remembered size a bottom section shows at. The Event List and the Step Sequencer take
 *  the Editors' height, so switching between them doesn't jump. */
export function bottomPaneSize(bottom: string): "mixer" | "editor" | "fx" | null {
  if (bottom === "mixer" || bottom === "fx") return bottom;
  if (bottom === "editor" || bottom === "list" || bottom === "steps") return "editor";
  return null;
}
