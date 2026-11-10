// The Tracks area ruler's strips, and the playhead's head in it: where a press grabs the head,
// and where the playhead's line falls on the screen's pixels. Pure, so tests can import it.
// Ruler.tsx draws the strips and hit-tests presses with this; `.ruler-playhead` in
// tracks-area.css draws the head at the sizes below.

/** Height of the cycle strip at the top of the ruler. */
export const CYCLE_H = 12;
/** Bottom of the bar numbers; the Autopunch strip and the marker lane are below. */
export const BARS_H = 30;

/** The head as drawn, from the cycle strip's lower edge: an odd width so the line runs down
 *  its middle. */
export const HEAD_W = 13;
export const HEAD_H = 11;
/** How far either side of the line a press still grabs the head: wider than it's drawn. */
export const HEAD_HIT_HALF_W = 9;
/** How far above the head a press still grabs it, into the cycle strip's lower edge. */
const HEAD_HIT_ABOVE = 4;

/**
 * Whether a press `dx` px right of the playhead line and `y` px from the ruler's top grabs the
 * head: a box around it, wider than the drawn head and down to the bottom of the bar numbers.
 */
export function onPlayheadHead(dx: number, y: number): boolean {
  return Math.abs(dx) <= HEAD_HIT_HALF_W && y >= CYCLE_H - HEAD_HIT_ABOVE && y < BARS_H;
}

/**
 * Where the playhead's line goes, `x` CSS px from the left of the element it's drawn in, which
 * is `origin` CSS px from the window's left, at `dpr` device pixels per CSS pixel (the
 * interface size included): on a whole device pixel of the screen, so the ruler, the global
 * tracks and the Tracks area draw it in the same column, solid, not spread over two.
 */
export function playheadX(x: number, dpr: number, origin = 0): number {
  return Number((Math.round((origin + x) * dpr) / dpr - origin).toFixed(4));
}

/** The line's width in CSS px: whole device pixels, one at 100 % on a standard display. */
export function playheadWidth(dpr: number): number {
  return Math.max(1, Math.round(dpr)) / dpr;
}

/** The transform and width that put a playhead line at `x`, for the elements that draw it. */
export function playheadStyle(x: number, dpr: number, origin = 0): { transform: string; width: string } {
  return { transform: `translate3d(${playheadX(x, dpr, origin)}px,0,0)`, width: `${Number(playheadWidth(dpr).toFixed(4))}px` };
}
