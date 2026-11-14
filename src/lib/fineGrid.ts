// The beat grid finer than a beat, for zooming in far: steps counted in ticks (960 a beat, as the
// LCD counts them), the finest one far enough apart at a zoom (for Smart snap and the grid
// lines), and the ruler's labels for them ("5.2.3.121").

/** Ticks in a beat: 4 divisions of 240. */
export const TICKS_PER_BEAT = 960;

/** The steps finer than a beat, in ticks, coarsest first: a division, then steps that divide one
 *  evenly, down to a single tick. */
export const FINE_STEPS = [240, 120, 60, 30, 15, 5, 1];

/** The Tracks area's grid lines show the finest step at least this far apart (Smart snaps to
 *  finer ones, lib/time.ts). */
export const FINE_MIN_PX = 60;

/** The finest step (ticks) at least `minPx` apart at `pxPerBeat`, or null when even a division
 *  is closer than that. */
export function fineStep(pxPerBeat: number, minPx: number): number | null {
  let best: number | null = null;
  for (const t of FINE_STEPS) if ((pxPerBeat * t) / TICKS_PER_BEAT >= minPx) best = t;
  return best;
}

/**
 * The ruler's label for the grid line `tick` ticks into bar `bar`, as precise as `step` needs:
 * "5.2" for beats, "5.2.3" for divisions, "5.2.3.121" for finer steps (1-based, as the LCD).
 */
export function fineLabel(bar: number, tick: number, step: number): string {
  const beat = Math.floor(tick / TICKS_PER_BEAT) + 1;
  const inBeat = tick % TICKS_PER_BEAT;
  const div = Math.floor(inBeat / 240) + 1;
  if (step % TICKS_PER_BEAT === 0) return `${bar}.${beat}`;
  if (step % 240 === 0) return `${bar}.${beat}.${div}`;
  return `${bar}.${beat}.${div}.${(inBeat % 240) + 1}`;
}
