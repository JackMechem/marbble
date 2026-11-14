// The pickup's length as Project Settings shows it: whole beats of the first signature plus
// ticks (960 a beat, as the LCD counts them). The project stores it in quarter notes (tempo.ts),
// in whole 32nd notes, so a MIDI file can give it a signature of its own (as the engine's
// `clamp_pickup` does).
import { TICKS_PER_BEAT } from "./fineGrid.ts";

/** The pickup's grid in quarters: a 32nd note. */
const STEP_Q = 0.125;

export interface PickupLength {
  beats: number;
  ticks: number;
}

/** `quarters` of pickup in beats of `den` and ticks, to the nearest tick. */
export function pickupLength(quarters: number, den: number): PickupLength {
  const total = Math.max(0, Math.round(((quarters * den) / 4) * TICKS_PER_BEAT));
  return { beats: Math.floor(total / TICKS_PER_BEAT), ticks: total % TICKS_PER_BEAT };
}

/** The ticks in a 32nd note, the pickup's step, with beats of `den`. */
export function pickupTickStep(den: number): number {
  return ((STEP_Q * den) / 4) * TICKS_PER_BEAT;
}

/** A pickup of `beats` beats of `num`/`den` and `ticks`, in quarters: whole 32nds (to the
 *  nearest), from none to a 32nd short of a bar. */
export function pickupQuarters(beats: number, ticks: number, num: number, den: number): number {
  const q = ((beats + ticks / TICKS_PER_BEAT) * 4) / den;
  const steps = Math.round(q / STEP_Q);
  return Math.min((num * 4) / den - STEP_Q, Math.max(0, steps * STEP_Q));
}
