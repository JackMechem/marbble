// The engine's regions as the UI receives them (crates/engine/src/model.rs; field names are its serde
// names): their content (notes, controller events, audio), patterns, playback parameters and take
// folders. The rest of the document is in project.ts.
import type { FadeShape } from "../tracks/regions/fades";
import type { Id } from "./project";

export interface Note {
  start: number;
  length: number;
  pitch: number;
  velocity: number;
  /** Muted notes stay in the region but don't play. */
  muted?: boolean;
}

/** What a controller event drives: a CC number, pitch bend (0–16383) or channel pressure. */
export type CcKind = { type: "controller"; cc: number } | { type: "pitch_bend" } | { type: "aftertouch" };

export interface CcEvent {
  /** Offset from the region start, in samples. */
  start: number;
  kind: CcKind;
  value: number;
}

export type RegionContent =
  | { type: "audio"; clip: Id; offset: number; gain_db: number; fade_in: number; fade_out: number; fade_shape?: FadeShape }
  | { type: "midi"; notes: Note[]; cc?: CcEvent[] };

export interface Region {
  id: Id;
  name: string;
  start: number;
  length: number;
  muted: boolean;
  /** Overrides the track colour. */
  color?: string | null;
  content: RegionContent;
  /** Timeline sample where the loop stops (absent / null = not looping). */
  loop_end?: number | null;
  /** Present on take folders (see TakeFolder). */
  takes?: TakeFolder | null;
  /** Playback parameters (Region inspector); absent = the defaults. See regionParams.ts. */
  params?: RegionParams;
  /** Present on pattern regions (Step Sequencer), which play the notes it makes. See editors/stepSequencer/model.ts. */
  pattern?: Pattern | null;
}

/** One step of a pattern row (crates/engine/src/pattern.rs). Absent fields are the defaults:
 *  off, velocity 100, gate 100 %, not tied. */
export interface PatternStep {
  on?: boolean;
  velocity?: number;
  /** Note length in percent of the step, 1–100. */
  gate?: number;
  /** Carries the note before on through this step. */
  tie?: boolean;
}

export interface PatternRow {
  pitch: number;
  muted?: boolean;
  solo?: boolean;
  /** Steps per whole note (16 = 1/16, 24 = 1/16 triplet); absent = the pattern's. */
  rate?: number | null;
  /** Steps before the row repeats; absent = the pattern's length. */
  length?: number | null;
  /** Missing steps at the end are off. */
  steps: PatternStep[];
}

/** A pattern region's pattern (Logic's Step Sequencer). */
export interface Pattern {
  /** Steps. */
  length: number;
  /** Steps per whole note. */
  rate: number;
  /** Percent; 0 = straight. */
  swing: number;
  /** Absent = true: repeats to the region's end. */
  looped?: boolean;
  rows: PatternRow[];
  /** Quarter notes into the pattern where the region starts (a piece an Arrangement move or a
   *  section edit cut off a longer pattern region); absent = 0. The engine keeps it. */
  offset?: number;
}

/** A region's playback parameters, applied as it plays (crates/engine/src/region_params.rs). */
export interface RegionParams {
  /** Grid as a fraction of a whole note (16 = 1/16, 12 = 1/12, an eighth triplet); 0 = off. */
  quantize: number;
  /** Q-Swing, percent (50 = straight). */
  swing: number;
  transpose: number;
  velocity: number;
  /** Ratio × 100 (300 = 3:1, 100 = off, 50 = 0.5:1); 0 = Fixed. */
  dynamics: number;
  /** Percent (10–200, 100 = unchanged); 0 = Fixed, 201 = Legato. */
  gate: number;
  /** Ticks (960 to a quarter note), or milliseconds with delay_ms. */
  delay: number;
  delay_ms: boolean;
}

/** A take folder. Takes and the comp are in take time: a timeline position `p` in the folder
 *  region is take time `p - region.start + content.offset`, and a take plays clip frame
 *  `take.offset + t` at take time `t`. */
export interface TakeFolder {
  /** Oldest first. */
  takes: Take[];
  /** Sorted, non-overlapping; gaps are silent. */
  comp: CompSegment[];
  /** Saved comps; `comp` is the one at `current`. Absent: one comp, "Comp 1". */
  comps?: Comp[];
  current?: number;
  /** A MIDI take folder's take time at the region's start. */
  offset?: number;
}

/** A saved comp of a take folder. */
export interface Comp {
  name: string;
  comp: CompSegment[];
}

export interface Take {
  id: Id;
  name: string;
  clip: Id;
  offset: number;
  /** Take-time range with audio. */
  start: number;
  end: number;
  gain_db: number;
  /** A MIDI take's notes and controllers, in take time. */
  notes?: Note[];
  cc?: CcEvent[];
  /** Marked good or bad; unrated takes leave it out. */
  rating?: TakeRating;
  /** The take's own colour in its row and the comp (else the folder's). */
  color?: string | null;
  /** The recording and which of its passes (grouped folders match takes by it). */
  pass?: [Id, number];
}

export type TakeRating = "none" | "good" | "bad";

export interface CompSegment {
  start: number;
  end: number;
  take: Id;
}
