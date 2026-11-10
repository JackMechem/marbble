// Edit › Snap Edits to Zero Crossings (Logic): a project setting. With it on, the engine moves
// splits and trims of audio regions to the nearest zero crossing (session/zero_crossings.rs).
import { act } from "../../engine/commands";
import type { MenuItem } from "../../ui/Menu";
import { keyHint } from "../../keyboard/keymap";
import { getState, showToast } from "../../state/store";

export function toggleZeroCrossings() {
  const on = !getState().snap?.project.snap_zero_crossings;
  act("set_snap_zero_crossings", { on });
  showToast(`Snap Edits to Zero Crossings ${on ? "on" : "off"}`, 1500);
}

/** The Edit menu item, with its tick. */
export function zeroCrossingsItem(): MenuItem {
  return { label: "Snap Edits to Zero Crossings", hint: keyHint("zero-crossings"), checked: !!getState().snap?.project.snap_zero_crossings, onClick: toggleZeroCrossings };
}
