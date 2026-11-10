// What the engine sends the UI: the snapshot of the session after each command (`act` stores it,
// commands.ts) and the tick about 30 times a second with the playhead, meters and what changed
// (state/store.ts keeps it outside React state).
import type { Crossfade } from "../tracks/regions/fades";
import type { Id, Project } from "./project";
import type { Note } from "./regions";

/** An edit's snapshot plus the ids of what it created. */
export interface Made {
  snapshot: Snapshot;
  ids: Id[];
}

export interface AudioStatus {
  running: boolean;
  host: string;
  input_name: string | null;
  output_name: string | null;
  input_channels: number;
  output_channels: number;
  sample_rate: number;
  buffer_size: number;
  error: string | null;
}

export interface Snapshot {
  project: Project;
  selected: Id | null;
  path: string;
  saved: boolean;
  dirty: boolean;
  audio: AudioStatus;
  midi_inputs: string[];
  open_editors: Id[];
  /** Instance ids of loaded VST3 inserts with a side-chain input. */
  key_inputs?: Id[];
  /** Track that live MIDI (hardware or Musical Typing) plays. */
  midi_target: string | null;
  /** Labels of the next undo / redo steps. */
  undo: string | null;
  redo: string | null;
  /** Plug-in latency and delay compensation. */
  pdc?: PdcReport;
  /** Why the edit was refused and put back (it touched a frozen track). */
  refused?: string | null;
  /** Something the command did went wrong without failing it (an inserted plug-in's window didn't open). */
  warning?: string | null;
  /** Where regions crossfade, as playback hears them. */
  crossfades?: Crossfade[];
  /** Instance ids of VST3 slots whose plug-in isn't installed (their settings are kept). */
  missing?: Id[];
  /** The notes of MIDI regions with playback parameters, as they play (region-relative). */
  played?: { region: Id; notes: Note[] }[];
  /** Loaded instruments with extra output buses. */
  instrument_outputs?: InstrumentOutputs[];
}

/** An output bus of an instrument; 0 is the main output, which plays through its strip. */
export interface OutputBusInfo {
  index: number;
  name: string;
  channels: number;
  active: boolean;
}

export interface InstrumentOutputs {
  track: Id;
  outputs: OutputBusInfo[];
}

/** session::StripLatency: frames. */
export interface StripLatency {
  track: Id;
  /** The strip's plug-ins, bypassed ones included. */
  latency: number;
  /** Compensation delay on its output. */
  delay: number;
  /** [instance uid, latency] per latent plug-in. */
  slots: [Id, number][];
}

export interface PdcReport {
  enabled: boolean;
  low_latency: boolean;
  /** How far Stereo Out lags the timeline. */
  total: number;
  total_ms: number;
  strips: StripLatency[];
}

export interface TakePreview {
  track: Id;
  start: number;
  frames: number;
  from: number;
  peaks: number[];
}

export interface Tick {
  /** Something went wrong out of sight (a recording couldn't be written); shown once. */
  warning?: string | null;
  pos: number;
  playing: boolean;
  recording: boolean;
  cpu: number;
  /** The worst recent single block's load: held for two seconds, then falling. */
  cpu_peak: number;
  /** Blocks so far that took longer than their own audio. Only ever grows. */
  late_blocks: number;
  /** Threads playing tracks right now: 1 until a project needs more. */
  threads: number;
  meters: [Id, number, number][];
  master: [number, number];
  inputs: number[];
  previews: TakePreview[];
  midi_activity: number;
  overruns: number;
  latency: { input_ms: number; output_ms: number; compensation_ms: number };
  spectrum: [Id, number[]] | null;
  /** Meters and running values of the built-in effects whose panels are open. */
  fx?: { uid: Id; meters: number[]; values: [number, number][] }[];
  /** Clip indicators: each strip's highest peak (linear) since reset; MASTER_ID for Stereo Out. */
  peak_holds?: [Id, number][];
  /** Gain reduction (dB) of the strips with a built-in dynamics effect. */
  gain_reduction?: [Id, number][];
  /** The parameter a controller assignment is being learned for (MIDI Learn). */
  learn?: string | null;
  /** What the learn that just finished assigned; sent once. */
  learned?: string | null;
  /** Key commands (keymap.ts ids, or menu item ids) MIDI controllers fired since the last tick;
   *  the main window runs them as their keys would. */
  commands?: string[];
  /** Messages from hardware MIDI inputs since the last tick (Step Input listens to them). */
  midi_in?: [number, number, number][];
}
