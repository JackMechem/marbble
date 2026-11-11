// The Track inspector's MIDI rows (Logic's Transpose, Velocity, Key Limit, Velocity Limit and
// Delay on a software instrument track): which tracks have them, their values and ranges, and
// the limit pairs a change of one end makes. TrackMidiRows.tsx draws them; the engine applies
// them (crates/engine/src/edit/track_midi/).
//
// This module only imports types and pure helpers, so it runs under plain Node for the tests in
// tests/.
import type { Track, TrackMidiSettings } from "../engine/project";
import { noteName, parseNote } from "../editors/eventList/model.ts";

export const DEFAULT_TRACK_MIDI: TrackMidiSettings = { transpose: 0, velocity: 0, key_limit: [0, 127], velocity_limit: [1, 127], delay: 0, delay_ms: false };

/** The ranges the engine keeps values in (TRANSPOSE_RANGE and VELOCITY_RANGE there). Delay has
 *  the Region inspector's (DELAY_TICKS_MAX and DELAY_MS_MAX in regionParams.ts). */
export const TRANSPOSE_MAX = 96;
export const VELOCITY_MAX = 99;

/** Only software instrument tracks play MIDI, so only they have the rows. */
export function hasMidiSettings(track: Track): boolean {
  return track.kind === "instrument";
}

/** A track's settings, the defaults filled in. */
export function midiSettingsOf(track: Track): TrackMidiSettings {
  return { ...DEFAULT_TRACK_MIDI, ...track.midi_settings };
}

/** A limit with one end moved to `value`: the other end follows when it would be passed, so the
 *  range never turns round. */
export function limitWith(limit: [number, number], end: "low" | "high", value: number): [number, number] {
  const [low, high] = limit;
  return end === "low" ? [value, Math.max(high, value)] : [Math.min(low, value), value];
}

/** Key Limit ends as Logic shows them: note names, middle C (60) being C3. */
export const formatKey = noteName;

/** A typed Key Limit end: a note name or a MIDI note number; null when it is neither. */
export const parseKey = parseNote;
