// The pickup's length as Project Settings shows it: whole beats of the first signature plus
// ticks (960 a beat, as the LCD counts them). The project stores it in quarter notes (tempo.ts).
import { TICKS_PER_BEAT } from "./fineGrid.ts";

export interface PickupLength {
  beats: number;
  ticks: number;
}

/** `quarters` of pickup in beats of `den` and ticks, to the nearest tick. */
export function pickupLength(quarters: number, den: number): PickupLength {
  const total = Math.max(0, Math.round(((quarters * den) / 4) * TICKS_PER_BEAT));
  return { beats: Math.floor(total / TICKS_PER_BEAT), ticks: total % TICKS_PER_BEAT };
}

/** A pickup of `beats` beats of `num`/`den` and `ticks`, in quarters: shorter than a bar, so at
 *  most a tick under it. */
export function pickupQuarters(beats: number, ticks: number, num: number, den: number): number {
  const total = Math.min(num * TICKS_PER_BEAT - 1, Math.max(0, Math.round(beats) * TICKS_PER_BEAT + Math.round(ticks)));
  return ((total / TICKS_PER_BEAT) * 4) / den;
}
