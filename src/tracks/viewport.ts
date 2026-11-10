// The Tracks area's scroll position and size, shared by the ruler, headers and canvas without going
// through React state on every scroll event; the ruler's height and the headers' width; and bringing
// the playhead or a track into view.

export interface Viewport {
  left: number;
  top: number;
  width: number;
  height: number;
}

let vp: Viewport = { left: 0, top: 0, width: 800, height: 400 };
const subs = new Set<(v: Viewport) => void>();

export function viewport(): Viewport {
  return vp;
}

export function setViewport(p: Partial<Viewport>) {
  vp = { ...vp, ...p };
  subs.forEach((s) => s(vp));
}

export function onViewport(f: (v: Viewport) => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}

/** Set by the arrange view so other components can scroll it. */
export let scrollTo: (left: number, top?: number) => void = () => {};
export function registerScroller(f: (left: number, top?: number) => void) {
  scrollTo = f;
}

/** Cycle strip, bar numbers, Autopunch strip and marker lane (see Ruler.tsx). */
export const RULER_H = 52;

// ---- revealing the playhead -------------------------------------------------------------------

const revealSubs = new Set<(pos: number) => void>();

/**
 * Asks every view that follows the playhead (the Tracks area and the open editor) to scroll
 * horizontally so `pos` (samples) is in view. Used by the transport commands that jump the
 * playhead while stopped, where Catch, which follows only during playback, doesn't help.
 */
export function revealPlayhead(pos: number) {
  revealSubs.forEach((f) => f(pos));
}

export function onRevealPlayhead(f: (pos: number) => void): () => void {
  revealSubs.add(f);
  return () => revealSubs.delete(f);
}

/**
 * The horizontal scroll that shows the playhead at `x` (px from the timeline's start) in a
 * view scrolled to `left` and `width` px wide, or null when it's already visible. Like Catch,
 * it puts the playhead a little in from the left edge (`margin`), or at the very start.
 */
export function revealLeft(x: number, left: number, width: number, margin = 60): number | null {
  if (x >= left && x <= left + width - 30) return null;
  return Math.max(0, x - margin);
}

/**
 * The vertical scroll that shows a track lane `top`..`top + height` (px from the first track)
 * in a view scrolled to `scrollTop` and `viewH` px tall, or null when it's already in full view.
 * Scrolls as little as it can: a lane above comes in at the top edge, one below at the bottom
 * edge (its top edge, when it's taller than the view).
 */
export function revealTop(top: number, height: number, scrollTop: number, viewH: number): number | null {
  if (top >= scrollTop && top + height <= scrollTop + viewH) return null;
  if (top < scrollTop || height >= viewH) return Math.max(0, top);
  return Math.max(0, top + height - viewH);
}
