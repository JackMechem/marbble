// A press-and-drag: the window's listeners from the press to the release, so the drag carries on
// outside the element pressed, and how far the pointer goes before a press counts as a drag.

/** How far (px) the pointer moves from the press before it's a drag rather than a click. */
export const DRAG_THRESHOLD_PX = 3;

/** Whether a pointer `dx`, `dy` px from the press has gone far enough for a drag. */
export const pastDragThreshold = (dx: number, dy = 0) => Math.abs(dx) >= DRAG_THRESHOLD_PX || Math.abs(dy) >= DRAG_THRESHOLD_PX;

const MODIFIERS = ["Shift", "Control", "Alt", "Meta"];

/** Calls `move` on every mouse move until the button is released, then stops listening and
 *  calls `up` if given. With `modifiers`, pressing or releasing Shift, Ctrl, Alt or Meta calls
 *  `move` again where the pointer last was, so a drag that reads them (snapping, copying)
 *  updates without the mouse moving. */
export function dragWith(move: (e: MouseEvent) => void, up?: (e: MouseEvent) => void, opts?: { modifiers?: boolean }) {
  let last: MouseEvent | null = null;
  const onMove = (e: MouseEvent) => {
    last = e;
    move(e);
  };
  const onKey = (e: KeyboardEvent) => {
    if (!last || !MODIFIERS.includes(e.key)) return;
    const { clientX, clientY, screenX, screenY, buttons } = last;
    move(new MouseEvent("mousemove", { clientX, clientY, screenX, screenY, buttons, shiftKey: e.shiftKey, ctrlKey: e.ctrlKey, altKey: e.altKey, metaKey: e.metaKey }));
  };
  const keys = opts?.modifiers ? (["keydown", "keyup"] as const) : [];
  const release = (e: MouseEvent) => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", release);
    for (const k of keys) window.removeEventListener(k, onKey);
    up?.(e);
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", release);
  for (const k of keys) window.addEventListener(k, onKey);
}
