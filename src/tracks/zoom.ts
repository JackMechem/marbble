// The Tracks area's horizontal zoom (the store's `pxPerBeat`): its limits and how far each step
// goes, shared by View › Zoom In and Out, Ctrl+wheel over the tracks and the overview strip, and
// the zoom slider's scale.
import { clamp } from "../lib/clamp.ts";

/** The widest zoom, in px per beat. */
export const ZOOM_MIN = 2;
/** The zoom slider's far end, in px per beat; Zoom In and Ctrl+wheel go on past it. */
export const SLIDER_MAX = 400;
/** The deepest zoom: samples this many px apart, as Logic goes in to single samples. */
export const MAX_PX_PER_SAMPLE = 32;
/** The factor of one Zoom In or Zoom Out (the menu and key commands). */
const COMMAND_STEP = 1.4;
/** The factor of one wheel notch. */
const WHEEL_STEP = 1.15;

/** The deepest zoom in px per beat for a project of `spb` samples a beat (samplesPerBeat). */
export const zoomMax = (spb: number) => Math.max(SLIDER_MAX, MAX_PX_PER_SAMPLE * spb);

export const clampZoom = (pxPerBeat: number, spb: number) => clamp(pxPerBeat, ZOOM_MIN, zoomMax(spb));

/** One Zoom In (`dir` 1) or Zoom Out (-1), within the limits. */
export function stepZoom(pxPerBeat: number, dir: 1 | -1, spb: number): number {
  return clampZoom(dir > 0 ? pxPerBeat * COMMAND_STEP : pxPerBeat / COMMAND_STEP, spb);
}

/** One wheel notch of zoom (in for a wheel turned away from the user), within the limits. */
export function wheelZoom(pxPerBeat: number, deltaY: number, spb: number): number {
  return clampZoom(deltaY < 0 ? pxPerBeat * WHEEL_STEP : pxPerBeat / WHEEL_STEP, spb);
}

/** The zoom slider's position, 0 to 1 on a log scale from ZOOM_MIN to SLIDER_MAX; zoomed in past
 *  that it stays at its end. */
export const zoomToSlider = (pxPerBeat: number) => clamp(Math.log(pxPerBeat / ZOOM_MIN) / Math.log(SLIDER_MAX / ZOOM_MIN), 0, 1);

/** The zoom at slider position `pos` (0 to 1). */
export const sliderToZoom = (pos: number) => ZOOM_MIN * Math.pow(SLIDER_MAX / ZOOM_MIN, pos);
