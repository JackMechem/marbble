// Zoomed in to single samples, the Tracks area's timeline is far wider than a browser can scroll
// (an element tops out at a few million px). Its scroller's content is then a window onto the
// timeline: `origin` (timeline px) is where the window starts, and it moves along when a scroll
// nears one of its ends. Everything else works in timeline px (viewport.ts `left`, `timelineLeft`).
import { clamp } from "../../lib/clamp.ts";

/** The widest the scroller's content gets, well inside every browser's limit. */
export const MAX_SCROLL_W = 4_000_000;

export interface ScrollWindow {
  /** Where the scroller's content starts on the timeline, in px. */
  origin: number;
  /** The scroller's content width, in px. */
  width: number;
}

/** `left` (timeline px at the view's left edge) held where a view `viewW` wide stays on a
 *  timeline `contentW` px wide. */
export const clampLeft = (left: number, viewW: number, contentW: number) => clamp(left, 0, Math.max(0, contentW - viewW));

/** The window that shows timeline px `left`, a view `viewW` wide, on a timeline `contentW` px
 *  wide: the whole timeline when it fits, otherwise MAX_SCROLL_W px with the view in the middle. */
export function windowAt(left: number, viewW: number, contentW: number): ScrollWindow {
  if (contentW <= MAX_SCROLL_W) return { origin: 0, width: contentW };
  const origin = Math.round(clamp(left + viewW / 2 - MAX_SCROLL_W / 2, 0, contentW - MAX_SCROLL_W));
  return { origin, width: MAX_SCROLL_W };
}

/** Whether a view at timeline px `left` has scrolled within a quarter window of an end of `w`
 *  that the timeline goes on past, so the window should move. */
export function nearWindowEnd(w: ScrollWindow, left: number, viewW: number, contentW: number): boolean {
  const margin = w.width / 4;
  const end = w.origin + w.width;
  return (w.origin > 0 && left - w.origin < margin) || (end < contentW && end - (left + viewW) < margin);
}
