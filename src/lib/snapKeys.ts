// Overriding the snap while dragging, as in Logic: hold Ctrl (Control on macOS, where Ctrl is
// the Control key on every platform) for a finer grid, ticks, and Ctrl+Shift for no snapping at
// all. Pressed after the drag starts, so on Linux and Windows Ctrl at the press still picks the
// Command-click tool. Pure, tested in tests/lib/snapKeys.test.ts.
import type { Snap } from "../state/store";

/** The modifier keys a drag reads, from its latest mouse or key event. */
export interface SnapKeys {
  ctrlKey: boolean;
  shiftKey: boolean;
}

/** The snap mode a drag uses with `keys` held: Ctrl snaps to ticks, Ctrl+Shift not at all; with
 *  Snap off, Ctrl changes nothing. */
export function snapModeWith(mode: Snap, keys?: SnapKeys): Snap {
  if (!keys?.ctrlKey || mode === "off") return mode;
  return keys.shiftKey ? "off" : "ticks";
}
