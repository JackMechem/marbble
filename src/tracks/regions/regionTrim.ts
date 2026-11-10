// Trimming regions in the Tracks area, as in Logic: dragging one selected region's start or end
// moves that edge of every selected region by the same amount (the dragged region's snapped
// move), each held within its own limits. Pure, tested in tests/tracks/regions/regionTrim.test.ts;
// tracks/canvas/pointer.ts drives the drag.
import type { Id } from "../../engine/project";
import type { Region } from "../../engine/regions";

export type TrimEdge = "left" | "right";

/** Where a region is, or will be once trimmed. */
export interface Span {
  start: number;
  length: number;
}

/** How far a region can be trimmed out: its earliest start, and its longest length from its
 *  current start. */
export interface TrimLimits {
  minStart: number;
  maxLength: number;
}

/** A region's limits. An audio region can't reach past the ends of its file (`frames` long,
 *  when its waveform is loaded) and a take folder past the ends of its takes; MIDI regions
 *  only stop at the project's start. */
export function trimLimits(r: Region, frames: number | undefined): TrimLimits {
  if (r.content.type !== "audio") return { minStart: 0, maxLength: Infinity };
  const { offset } = r.content;
  const minStart = r.start - offset;
  if (r.takes) return { minStart, maxLength: Math.max(0, ...r.takes.takes.map((t) => t.end)) - offset };
  return { minStart, maxLength: frames !== undefined ? frames - offset : Infinity };
}

/** `r` with its `edge` moved by `delta` frames, held within `lim`, the project's start and
 *  `minLength`. Trimming the start keeps the end where it is. */
export function trimmed(r: Span, lim: TrimLimits, edge: TrimEdge, delta: number, minLength: number): Span {
  if (edge === "right") return { start: r.start, length: Math.min(lim.maxLength, Math.max(minLength, r.length + delta)) };
  const start = Math.min(r.start + r.length - minLength, Math.max(0, lim.minStart, r.start + delta));
  return { start, length: r.length - (start - r.start) };
}

/** How far the dragged region's edge moves: from where it is to the snapped pointer. */
export function trimDelta(r: Span, edge: TrimEdge, dx: number, snap: (pos: number) => number): number {
  const at = edge === "left" ? r.start : r.start + r.length;
  return snap(at + dx) - at;
}

/** Every region's new span for a trim of `edge` by `delta`; regions it leaves unchanged are
 *  left out. */
export function trimAll(regions: { id: Id; span: Span; lim: TrimLimits }[], edge: TrimEdge, delta: number, minLength: number): Map<Id, Span> {
  const out = new Map<Id, Span>();
  for (const { id, span, lim } of regions) {
    const s = trimmed(span, lim, edge, delta, minLength);
    if (s.start !== span.start || s.length !== span.length) out.set(id, s);
  }
  return out;
}
