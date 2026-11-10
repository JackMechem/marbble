// The Piano Roll's maths with no screen in it: its lanes and tools, the note area's mapping, what a
// point hits, and grid arithmetic in musical time. Tested in tests/editors/pianoRoll/model.test.ts;
// the rest of this folder calls these with the live values.
import type { Id, Project, Track } from "../../engine/project";
import type { CcKind, Region } from "../../engine/regions";
import { barFloor, ccMax, sameKind, snapTime, type PCc, type PNote, type TimeBase } from "../midiEdit.ts";
import type { Warp } from "../../lib/tempo";
import { clamp } from "../../lib/clamp.ts";

export type Tool = "pointer" | "pencil" | "eraser" | "velocity" | "mute" | "scissors" | "glue" | "line";

// ---- lanes and tools ------------------------------------------------------------------------

export const LANES: { id: string; label: string; short: string }[] = [
  { id: "velocity", label: "Velocity", short: "Velocity" },
  { id: "cc:1", label: "Modulation (CC 1)", short: "Modulation" },
  { id: "cc:11", label: "Expression (CC 11)", short: "Expression" },
  { id: "cc:64", label: "Sustain (CC 64)", short: "Sustain" },
  { id: "pb", label: "Pitch Bend", short: "Pitch Bend" },
  { id: "at", label: "Aftertouch", short: "Aftertouch" },
  { id: "cc:7", label: "Volume (CC 7)", short: "Volume" },
  { id: "cc:10", label: "Pan (CC 10)", short: "Pan" },
];

export function laneKind(id: string): CcKind | null {
  if (id === "pb") return { type: "pitch_bend" };
  if (id === "at") return { type: "aftertouch" };
  const m = /^cc:(\d+)$/.exec(id);
  return m ? { type: "controller", cc: Math.min(127, parseInt(m[1])) } : null;
}

export const laneShort = (id: string) => LANES.find((l) => l.id === id)?.short ?? `CC ${id.slice(3)}`;

export const TOOLS: { id: Tool; label: string; key: string }[] = [
  { id: "pointer", label: "Pointer", key: "T" },
  { id: "pencil", label: "Pencil", key: "P" },
  { id: "eraser", label: "Eraser", key: "E" },
  { id: "velocity", label: "Velocity", key: "V" },
  { id: "mute", label: "Mute", key: "M" },
  { id: "scissors", label: "Scissors", key: "I" },
  { id: "glue", label: "Glue", key: "G" },
  { id: "line", label: "Line (lanes)", key: "L" },
];

export const timeBase = (p: Project): TimeBase => ({ sampleRate: p.sample_rate, tempo: p.tempo, sigNum: p.sig_num, sigDen: p.sig_den });

// A pattern region plays its steps, not its notes: the Step Sequencer edits it.
export const midiRegions = (t: Track) => t.regions.filter((r) => r.content.type === "midi" && !r.pattern);

// ---- the note area's mapping ----------------------------------------------------------------

/** Horizontal and vertical mapping of the note area (CSS pixels). */
export interface PrView {
  /** Pixels per sample. */
  pps: number;
  noteH: number;
  left: number;
  top: number;
  W: number;
  H: number;
}

export const xOf = (v: PrView, t: number) => t * v.pps - v.left;
export const yOf = (v: PrView, p: number) => (127 - p) * v.noteH - v.top;
export const timeAt = (v: PrView, x: number) => (x + v.left) / v.pps;
export const pitchAt = (v: PrView, y: number) => Math.max(0, Math.min(127, 127 - Math.floor((y + v.top) / v.noteH)));

export const BLACK = new Set([1, 3, 6, 8, 10]);
export const LANE_PAD = 6;
/** Lane y for a value (0–max) and back. */
export const laneY = (H: number, value: number, max: number) => LANE_PAD + (1 - value / max) * (H - 2 * LANE_PAD);
export const laneValue = (H: number, y: number, max: number) => Math.round(Math.max(0, Math.min(1, 1 - (y - LANE_PAD) / (H - 2 * LANE_PAD))) * max);

// ---- what a point hits ----------------------------------------------------------------------

export type NoteZone = "left" | "right" | "body";

/** The note under a point, and which part of it: an edge to resize or the body. */
export function noteAt(v: PrView, notes: PNote[], x: number, y: number): { n: PNote; zone: NoteZone } | null {
  const p = pitchAt(v, y);
  let found: PNote | null = null;
  // Selected notes are drawn on top, so they're hit first.
  for (let pass = 0; pass < 2 && !found; pass++) {
    for (let i = notes.length - 1; i >= 0; i--) {
      const n = notes[i];
      if (n.pitch !== p || n.sel !== (pass === 0)) continue;
      const x0 = xOf(v, n.start);
      const x1 = x0 + Math.max(3, n.length * v.pps);
      if (x >= x0 - 1 && x <= x1 + 1) {
        found = n;
        break;
      }
    }
  }
  if (!found) return null;
  const x0 = xOf(v, found.start);
  const w = Math.max(3, found.length * v.pps);
  const edge = Math.min(6, w / 3);
  if (x > x0 + w - edge) return { n: found, zone: "right" };
  if (x < x0 + edge && w > 14) return { n: found, zone: "left" };
  return { n: found, zone: "body" };
}

/** The velocity stalk nearest `x`, within a few pixels, selected notes first. */
export function stalkAt(v: PrView, notes: PNote[], x: number): PNote | null {
  let hit: PNote | null = null;
  let best = 5;
  for (const n of notes) {
    const d = Math.abs(xOf(v, n.start) - x) - (n.sel ? 0.5 : 0);
    if (d < best) [hit, best] = [n, d];
  }
  return hit;
}

/** The controller event dot under a point in a lane `H` high. */
export function ccDotAt(v: PrView, all: PCc[], kind: CcKind, x: number, y: number, H: number): PCc | null {
  const max = ccMax(kind);
  let hit: PCc | null = null;
  let best = 7;
  for (const c of all) {
    if (!sameKind(c.kind, kind)) continue;
    const d = Math.hypot(xOf(v, c.start) - x, laneY(H, c.value, max) - y);
    if (d < best) [hit, best] = [c, d];
  }
  return hit;
}

/** The key played at a point in the keyboard column `keyW` wide, and how hard: further right plays
 *  louder, as on Logic's keyboard. Right of a black key, the row belongs to the white keys either
 *  side. */
export function keyAtPoint(v: PrView, x: number, y: number, keyW: number) {
  let p = pitchAt(v, y);
  if (BLACK.has(p % 12) && x > keyW * 0.6) p += y + v.top - (127 - p) * v.noteH < v.noteH / 2 ? 1 : -1;
  return { p: Math.max(0, Math.min(127, p)), vel: Math.round(40 + (87 * Math.min(keyW, Math.max(0, x))) / keyW) };
}

/** What a press on the ruler's cycle strip at `x` grabs, with the cycle's ends at `sx` and `ex`. */
export function cycleGrab(has: boolean, x: number, sx: number, ex: number): "new" | "move" | "start" | "end" {
  if (has && Math.abs(x - sx) < 6) return "start";
  if (has && Math.abs(x - ex) < 6) return "end";
  if (has && x > sx && x < ex) return "move";
  return "new";
}

// ---- grid arithmetic ------------------------------------------------------------------------

/** What the grid sums need of the Piano Roll's grid. */
export interface GridLengths {
  bar: number;
  grid: number;
}

/** Runs `f` on the notes in musical time, so grid-based edits follow the tempo map. */
export function inMusicalTime(w: Warp | null, notes: PNote[], f: (n: PNote[]) => PNote[]): PNote[] {
  if (!w) return f(notes);
  const v = notes.map((n) => {
    const s = w.toV(n.start);
    return { ...n, start: s, length: w.toV(n.start + n.length) - s };
  });
  return f(v).map((n) => {
    const s = Math.round(w.fromV(n.start));
    return { ...n, start: s, length: Math.max(1, Math.round(w.fromV(n.start + n.length)) - s) };
  });
}

/** A grid-length span starting at `t`, in samples there. */
export function gridSpanAt(w: Warp | null, t: number, len: number) {
  return w ? Math.round(w.fromV(w.toV(t) + len) - t) : Math.round(len);
}

/** Snaps to the grid unless snapping is off or `free` (a modifier) is held. */
export function snapToGrid(g: GridLengths, w: Warp | null, snap: boolean, t: number, free = false) {
  if (!snap || free) return Math.round(t);
  return w ? Math.round(w.fromV(snapTime(w.toV(t), g.grid, w.bars))) : snapTime(t, g.grid, g.bar);
}

/** The grid line at or before `t`, where the pencil puts a note. */
export function gridFloor(g: GridLengths, w: Warp | null, snap: boolean, t: number, free = false) {
  if (!snap || free) return Math.round(t);
  const v = w ? w.toV(t) : t;
  const b = barFloor(v, w?.bars ?? g.bar);
  const at = b + Math.floor((v - b) / g.grid + 1e-6) * g.grid;
  return w ? Math.round(w.fromV(at)) : at;
}

/** The region a note or event at `t` goes in: one covering it (the edited one first), else
 *  one ending or starting within a bar of it, which then grows to hold it. Null when there
 *  is none, so a new region is made there. */
export function regionForTime(regs: Region[], f: Id | null, bar: number, t: number): Id | null {
  const covers = (r: Region) => t >= r.start && t < r.start + r.length;
  const fr = regs.find((r) => r.id === f);
  if (fr && covers(fr)) return fr.id;
  const c = [...regs].reverse().find(covers);
  if (c) return c.id;
  const dist = (r: Region) => Math.min(Math.abs(t - r.start), Math.abs(t - (r.start + r.length)));
  if (fr && dist(fr) <= bar) return fr.id;
  let best: Region | null = null;
  let d = bar;
  for (const r of regs) if (dist(r) <= d) [best, d] = [r, dist(r)];
  return best?.id ?? null;
}

// ---- opening a region: the rows in view, the lane's share ------------------------------------

/** Rows kept clear above and below a region's notes when it opens. */
export const FIT_MARGIN_ROWS = 2;

/**
 * Where the note area scrolls to when a region opens (`scrollTop`): its notes' pitch range
 * (`lo`..`hi`) centred, with a little room when there is some, or, when the range is taller than
 * the view, from its highest note down. No notes: middle C in the middle.
 */
export function fitPitchScroll(lo: number | null, hi: number | null, noteH: number, viewH: number): number {
  const max = Math.max(0, 128 * noteH - viewH);
  const clamp = (at: number) => Math.round(Math.max(0, Math.min(max, at)));
  if (lo === null || hi === null) return clamp((127 - 60) * noteH - viewH / 2);
  const rows = hi - lo + 1;
  const margin = (rows + 2 * FIT_MARGIN_ROWS) * noteH <= viewH ? FIT_MARGIN_ROWS : 0;
  const top = (127 - hi - margin) * noteH;
  const height = (rows + 2 * margin) * noteH;
  return clamp(height <= viewH ? top - (viewH - height) / 2 : top);
}

/** The lane's share of the height under the ruler, by default and at the least and most. */
export const LANE_SHARE = { initial: 1 / 3, min: 0.1, max: 0.8 };
/** The lane's least height in pixels, however short the editor. */
export const LANE_MIN_PX = 40;

/** The lane's share after dragging its divider: `laneH` px of `areaH` (lane and notes together). */
export function laneShareOf(laneH: number, areaH: number): number {
  if (!(areaH > 0)) return LANE_SHARE.initial;
  return Math.max(LANE_SHARE.min, Math.min(LANE_SHARE.max, laneH / areaH));
}

// ---- horizontal zoom ------------------------------------------------------------------------

/** The horizontal zoom limits in px per beat. */
export const ZOOM_MIN = 6;
export const ZOOM_MAX = 1600;

export const clampZoom = (pxPerBeat: number) => clamp(pxPerBeat, ZOOM_MIN, ZOOM_MAX);

/** The zoom slider's position (0–1) for a zoom. The scale is logarithmic, so every stretch of the
 *  slider zooms by the same factor. */
export const zoomToSlider = (pxPerBeat: number) => Math.log(pxPerBeat / ZOOM_MIN) / Math.log(ZOOM_MAX / ZOOM_MIN);

/** The zoom at a slider position (0–1). */
export const sliderToZoom = (pos: number) => ZOOM_MIN * Math.pow(ZOOM_MAX / ZOOM_MIN, pos);
