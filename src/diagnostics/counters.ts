// The counts of what went wrong (late blocks, device gaps, overloads…), counted from the window's
// last reset and watched for rising between two snapshots. The engine's counters only ever grow
// (but a reopened device counts its own afresh), so a reset keeps where they stood and the window
// shows what came since, as the CPU meter's click does for late blocks.
import type { Diagnostics } from "./snapshot.ts";

export interface Counters {
  late_blocks: number;
  overloads: number;
  engine_errors: number;
  bad_audio: number;
  cmds_dropped: number;
  overruns: number;
  midi_rec_lost: number;
  device_gaps: number;
  device_events: number;
  stream_errors: number;
  restarts: number;
}

export type Counter = keyof Counters;

export function countersOf(d: Diagnostics): Counters {
  const { load: l, audio: a } = d;
  return {
    late_blocks: l.late_blocks,
    overloads: l.overloads,
    engine_errors: l.engine_errors,
    bad_audio: l.bad_audio,
    cmds_dropped: l.cmds_dropped,
    overruns: l.overruns,
    midi_rec_lost: l.midi_rec_lost,
    device_gaps: a.device_gaps,
    device_events: a.device_events,
    stream_errors: a.stream_errors,
    restarts: a.restarts,
  };
}

/** `now` counted from `base`. A counter below its base started afresh (a new device), so all of
 *  it is new. */
export function since(now: Counters, base: Counters | null): Counters {
  if (!base) return now;
  const out = { ...now };
  for (const k of Object.keys(now) as Counter[]) out[k] = now[k] >= base[k] ? now[k] - base[k] : now[k];
  return out;
}

/** How long a counter that grew stays marked as rising. */
export const RISE_MS = 5000;

/** When each counter last grew, in the window's clock (ms). */
export type Rises = Partial<Record<Counter, number>>;

/** `rises` with the counters that grew from `prev` to `now` stamped `at`. */
export function noteRises(rises: Rises, prev: Counters | null, now: Counters, at: number): Rises {
  if (!prev) return rises;
  const out = { ...rises };
  for (const k of Object.keys(now) as Counter[]) if (now[k] > prev[k]) out[k] = at;
  return out;
}

/** The counters that grew within `RISE_MS` before `at`. */
export function risingAt(rises: Rises, at: number): Set<Counter> {
  return new Set((Object.keys(rises) as Counter[]).filter((k) => at - (rises[k] ?? -Infinity) < RISE_MS));
}
