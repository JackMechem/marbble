// A press-and-drag: the window's listeners from the press to the release, so the drag carries on
// outside the element pressed, and how far the pointer goes before a press counts as a drag.

/** How far (px) the pointer moves from the press before it's a drag rather than a click. */
export const DRAG_THRESHOLD_PX = 3;

/** Whether a pointer `dx`, `dy` px from the press has gone far enough for a drag. */
export const pastDragThreshold = (dx: number, dy = 0) => Math.abs(dx) >= DRAG_THRESHOLD_PX || Math.abs(dy) >= DRAG_THRESHOLD_PX;

/** Calls `move` on every mouse move until the button is released, then stops listening and
 *  calls `up` if given. */
export function dragWith(move: (e: MouseEvent) => void, up?: (e: MouseEvent) => void) {
  const release = (e: MouseEvent) => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", release);
    up?.(e);
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", release);
}
