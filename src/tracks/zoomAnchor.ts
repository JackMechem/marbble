// Where a horizontal zoom of the Tracks area holds still: the point of the view that shows the same
// time before and after, so the zoom neither jumps nor fights Catch.

export interface AnchorInput {
  /** The view's width in px. */
  viewW: number;
  /** The playhead's x in the view (px from its left edge; outside 0..viewW when off screen). */
  playheadX: number;
  /** The pointer's x in the view, for a wheel zoom. */
  pointerX?: number;
  /** Catch Playhead is on. */
  catchOn: boolean;
  /** The transport is playing (Catch only follows during playback). */
  playing: boolean;
}

/**
 * The x in the view (px) a zoom keeps in place. While Catch follows the playhead, the playhead,
 * so the view goes on following it rather than zooming it out of view and paging back. Otherwise
 * the pointer for a wheel zoom; for the menu, keys and slider, the playhead when Catch is on and
 * it is in view, else the middle of the view.
 */
export function zoomAnchorX({ viewW, playheadX, pointerX, catchOn, playing }: AnchorInput): number {
  const playheadInView = playheadX >= 0 && playheadX <= viewW;
  if (catchOn && playheadInView && (playing || pointerX === undefined)) return playheadX;
  return pointerX ?? viewW / 2;
}

/** The view's left edge (timeline px) that keeps the time at `x` in place when the zoom goes
 *  from `pps` to `nextPps` px per sample, the view's left edge being `left`. */
export const anchoredZoomLeft = (left: number, x: number, pps: number, nextPps: number) => ((left + x) / pps) * nextPps - x;
