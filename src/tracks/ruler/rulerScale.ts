// What the ruler marks between bar lines at a zoom: zoomed in, labels for beats and finer steps
// ("5.2", "5.2.3", "5.2.3.121"), unlabelled marks for the next steps down, and a mark at every
// sample once they are far enough apart.
import { FINE_STEPS, TICKS_PER_BEAT } from "../../lib/fineGrid.ts";

/** Labels at least this far apart, in px, so they don't collide. */
export const LABEL_MIN_PX = 64;
/** Unlabelled marks at least this far apart. */
export const MARK_MIN_PX = 16;
/** A mark at every sample once samples are this far apart. */
export const SAMPLE_MARK_PX = 6;

/** A beat and the steps finer than one, in ticks, coarsest first. */
const STEPS = [TICKS_PER_BEAT, ...FINE_STEPS];

/** The finest step (ticks: a beat or finer) at least `minPx` apart at `pxPerBeat`, or null when
 *  even beats are closer. */
export function rulerStep(pxPerBeat: number, minPx: number): number | null {
  let best: number | null = null;
  for (const t of STEPS) if ((pxPerBeat * t) / TICKS_PER_BEAT >= minPx) best = t;
  return best;
}

export interface RulerScale {
  /** The labelled step in ticks, or null: bar numbers only. */
  label: number | null;
  /** The unlabelled step finer than a beat, or null: none (beats have their own marks). */
  mark: number | null;
  /** Whether each sample gets a mark. */
  samples: boolean;
}

/** The ruler's marks at `pxPerBeat` and `pps` (px per sample). */
export function rulerScale(pxPerBeat: number, pps: number): RulerScale {
  const label = rulerStep(pxPerBeat, LABEL_MIN_PX);
  const mark = rulerStep(pxPerBeat, MARK_MIN_PX);
  return { label, mark: mark !== null && mark < TICKS_PER_BEAT && mark !== label ? mark : null, samples: pps >= SAMPLE_MARK_PX };
}
