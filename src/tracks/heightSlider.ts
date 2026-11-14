// The Tracks area's Height slider. It runs on a log scale, as Taller and Shorter Tracks and
// Alt+wheel step by a factor: on a straight scale from 22 to 260 px the default 64 px sat a sixth
// of the way along, so the slider's low end looked like the default.

/** The slider's position (0..1) for track height `h`, between `min` and `max`. */
export function heightToSlider(h: number, min: number, max: number): number {
  const p = Math.log(h / min) / Math.log(max / min);
  return Math.min(1, Math.max(0, p));
}

/** The track height (whole px) at slider position `p` (0..1). */
export function sliderToHeight(p: number, min: number, max: number): number {
  const h = Math.round(min * Math.pow(max / min, Math.min(1, Math.max(0, p))));
  return Math.min(max, Math.max(min, h));
}
