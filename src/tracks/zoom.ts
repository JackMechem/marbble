// The Tracks area's horizontal zoom (the store's `pxPerBeat`): its limits and how far each step
// goes, shared by View › Zoom In and Out, Ctrl+wheel over the tracks and the overview strip.
import { clamp } from "../lib/clamp.ts";

/** The zoom limits in px per beat. */
export const ZOOM_MIN = 2;
export const ZOOM_MAX = 400;
/** The factor of one Zoom In or Zoom Out (the menu and key commands). */
const COMMAND_STEP = 1.4;
/** The factor of one wheel notch. */
const WHEEL_STEP = 1.15;

export const clampZoom = (pxPerBeat: number) => clamp(pxPerBeat, ZOOM_MIN, ZOOM_MAX);

/** One Zoom In (`dir` 1) or Zoom Out (-1), within the limits. */
export function stepZoom(pxPerBeat: number, dir: 1 | -1): number {
  return clampZoom(dir > 0 ? pxPerBeat * COMMAND_STEP : pxPerBeat / COMMAND_STEP);
}

/** One wheel notch of zoom (in for a wheel turned away from the user), within the limits. */
export function wheelZoom(pxPerBeat: number, deltaY: number): number {
  return clampZoom(deltaY < 0 ? pxPerBeat * WHEEL_STEP : pxPerBeat / WHEEL_STEP);
}
