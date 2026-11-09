// A pan position as text: the value under the channel strips' pan knobs and what the knob says
// to a screen reader. Pure, tested in tests/panText.test.ts.

/** Pan runs -1..1 (`Track::pan`) and the knob moves in this many steps either side, so it reads
 *  L 64..C..R 64, the same distance each way. (Logic's -64..+63 has one step fewer on the right,
 *  which our symmetric range has no place for.) */
export const PAN_STEPS = 64;

/** A pan position (-1..1) in knob steps, -64..64: rounded, clamped, and 0 for anything not a
 *  number. */
export function panSteps(v: number): number {
  if (!Number.isFinite(v)) return 0;
  const n = Math.round(Math.min(1, Math.max(-1, v)) * PAN_STEPS);
  return n === 0 ? 0 : n; // not -0
}

/** The value under a pan knob: "C" at the centre, "L 23" and "R 40" either side. */
export function panLabel(v: number): string {
  const n = panSteps(v);
  return n === 0 ? "C" : n < 0 ? `L ${-n}` : `R ${n}`;
}

/** The knob's spoken value (`aria-valuetext`) and tooltip: its label, with the centre spelt out. */
export function panText(v: number): string {
  return panSteps(v) === 0 ? "Centre" : panLabel(v);
}
