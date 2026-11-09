// Take folder comps and MIDI take folders: what the folder menu lists and what the folder's
// name bar says. The comp logic itself is in the engine (crates/engine/src/takes.rs).
import type { CompSegment, Id, Region, TakeFolder } from "./api";

/** A MIDI take folder: whole takes only, no swipe comping (as in Logic). */
export const isMidiFolder = (r: Region): boolean => !!r.takes && r.content.type === "midi";

/** The folder's comps by name. One that never had a second comp has "Comp 1". */
export function compNames(f: TakeFolder): string[] {
  return f.comps?.length ? f.comps.map((c) => c.name) : ["Comp 1"];
}

/** Index of the comp being heard. */
export function currentComp(f: TakeFolder): number {
  return Math.min(f.current ?? 0, compNames(f).length - 1);
}

/** The take heard throughout when the comp is one whole take, else null. */
function wholeTakeName(f: TakeFolder): string | null {
  if (f.comp.length !== 1) return null;
  const s = f.comp[0];
  const t = f.takes.find((x) => x.id === s.take);
  return t && t.start === s.start && t.end === s.end ? t.name : null;
}

/** What the folder's name bar says after its name: the take heard throughout, or the comp
 *  (Logic's "Audio 1: Comp 2"). A MIDI folder names its take. */
export function folderLabel(r: Region): string | null {
  const f = r.takes;
  if (!f) return null;
  if (isMidiFolder(r)) {
    const id = f.comp[0]?.take ?? f.takes[f.takes.length - 1]?.id;
    return f.takes.find((t) => t.id === id)?.name ?? null;
  }
  return wholeTakeName(f) ?? compNames(f)[currentComp(f)];
}

// ---- the folder's name bar: its disclosure triangle and take badge -----------------------------

/** Width of the triangle's part of the name bar, from the folder's visible start: a press
 *  anywhere in it, over the bar's full height, opens or closes the folder. Much wider than the
 *  glyph, which sits at FOLDER_TRIANGLE_X. */
export const FOLDER_TRIANGLE_W = 20;
/** Centre of the triangle glyph, from the folder's visible start. */
export const FOLDER_TRIANGLE_X = 10;
/** The take badge's box, after the triangle's part. */
export const FOLDER_BADGE_X = 21;
export const FOLDER_BADGE_BOX_W = 16;
/** Width of the triangle and take badge together; the folder's name starts after it. A press
 *  between the triangle's part and here opens the take folder menu. */
export const FOLDER_BADGE_W = 37;

/**
 * What a press `dx` px right of a folder's visible start and `y` px below the top of its lane
 * hits in a name bar `bar` px tall (the region is drawn 1 px into the lane): the triangle, the
 * badge or neither.
 */
export function folderBarHit(dx: number, y: number, bar: number): "triangle" | "badge" | null {
  if (dx < 0 || y < 0 || y > bar + 1) return null;
  if (dx < FOLDER_TRIANGLE_W) return "triangle";
  return dx < FOLDER_BADGE_W ? "badge" : null;
}

// ---- comp sections: what the engine's comp section edits will do, for drawing a drag ----------

/** The comp section heard at take time `at`. */
export function sectionAt(f: TakeFolder, at: number): CompSegment | null {
  return f.comp.find((s) => s.start <= at && at < s.end) ?? null;
}

/** The end of one of `take`'s comp sections within `tol` of take time `at` (the nearest), or
 *  null: where a drag in that take's row moves a section edge. */
export function edgeNear(f: TakeFolder, take: Id, at: number, tol: number): number | null {
  let best: number | null = null;
  for (const s of f.comp) {
    if (s.take !== take) continue;
    for (const e of [s.start, s.end]) if (Math.abs(e - at) <= tol && (best === null || Math.abs(e - at) < Math.abs(best - at))) best = e;
  }
  return best;
}

/** The comp with its edge at take time `at` dragged to `to`, as the engine's `move_edge` does it:
 *  where one section hands over to the next both move, an outer edge moves its own section, and
 *  no section passes its neighbour, vanishes or reaches past its take's audio. */
export function movedEdge(f: TakeFolder, at: number, to: number): CompSegment[] {
  const comp = f.comp.map((s) => ({ ...s }));
  const l = comp.findIndex((s) => s.end === at);
  const r = comp.findIndex((s) => s.start === at);
  const span = (s: CompSegment) => f.takes.find((t) => t.id === s.take) ?? s;
  let lo = -Infinity;
  let hi = Infinity;
  if (l >= 0) {
    lo = comp[l].start + 1;
    hi = Math.min(hi, span(comp[l]).end, r < 0 && l + 1 < comp.length ? comp[l + 1].start : Infinity);
  }
  if (r >= 0) {
    lo = Math.max(lo, span(comp[r]).start, l < 0 && r > 0 ? comp[r - 1].end : -Infinity);
    hi = Math.min(hi, comp[r].end - 1);
  }
  if ((l < 0 && r < 0) || lo > hi) return comp;
  const x = Math.min(hi, Math.max(lo, to));
  if (l >= 0) comp[l].end = x;
  if (r >= 0) comp[r].start = x;
  return comp;
}

/** The take `delta` rows from section `s`'s take (up when negative) that has audio somewhere in
 *  it, where Move Section to Take Above or Below sends it; null when there is none. */
export function neighbourTake(f: TakeFolder, s: CompSegment, delta: number): Id | null {
  const k = f.takes.findIndex((t) => t.id === s.take);
  if (k < 0 || !delta) return null;
  for (let i = k + delta; i >= 0 && i < f.takes.length; i += Math.sign(delta)) {
    const t = f.takes[i];
    if (t.start < s.end && t.end > s.start) return t.id;
  }
  return null;
}
