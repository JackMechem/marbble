// How a bar of controls fits its width on one row: it gives way one step at a time, in an order
// the bar lists (captions go, then gaps tighten, then the least used controls move into a menu),
// for as many steps as it takes. Each bar measures what it needs (useBarFit, which also remembers
// what each step needed so it can go back when the room comes back). Pure, so it can be unit
// tested.

/**
 * The steps in effect after the bar showing `fit` steps measured `need` pixels in `room`. One
 * step at a time: more when the row overflows, one fewer when the step before was measured to
 * need no more than the room there is. `needs[i]` is what the row needed with `i` steps, or
 * undefined when that hasn't been measured since its content changed, which is tried.
 */
export function nextFit(fit: number, need: number, room: number, needs: readonly (number | undefined)[], steps: number): number {
  // A bar that isn't laid out (a hidden pane) has nothing to fit.
  if (room <= 0) return fit;
  if (need > room + 0.5) return Math.min(steps, fit + 1);
  if (fit > 0) {
    const before = needs[fit - 1];
    if (before === undefined || before <= room + 0.5) return fit - 1;
  }
  return fit;
}

/** A bar's steps in effect: whether one is, and the class names its stylesheet keys on
 *  (`fit-<step>` for each). */
export interface Fit<S extends string> {
  fit: number;
  has: (step: S) => boolean;
  className: string;
}

export function fitOf<S extends string>(steps: readonly S[], fit: number): Fit<S> {
  const taken = steps.slice(0, Math.max(0, fit));
  return { fit, has: (step) => taken.includes(step), className: taken.map((s) => ` fit-${s}`).join("") };
}

/** One edge of a box in the row, as measured: its right edge and the margin after it. */
export interface Edge {
  right: number;
  marginRight: number;
}

/**
 * The width a row needs: from the bar's left edge to past its rightmost box and the bar's own end
 * padding and border. With too little room nothing in the row shrinks (its boxes don't), so the
 * rightmost box runs past the end by what's missing; with room to spare it ends at the padding.
 */
export function rowNeed(left: number, edges: readonly Edge[], endPad: number): number {
  let right = left;
  for (const e of edges) right = Math.max(right, e.right + Math.max(0, e.marginRight));
  return right - left + endPad;
}
