// The TypeScript mirror of the engine's saved document (crates/engine/src/model/; field names are
// its serde names): the project, its tracks, plug-in slots, sends, groups, VCAs, markers and
// automation. Regions are in regions.ts; what the engine sends back in snapshot.ts. engine/ is the
// UI's side of the engine: these types, devices.ts and listings.ts, and the commands (commands.ts).
import type { SigEvent, TempoEvent } from "../lib/tempo";
import type { DragMode } from "../tracks/regions/fades";
import type { Region } from "./regions";

export type Id = number;
export type TrackKind = "audio" | "instrument" | "folder" | "summing" | "aux";

export const isStack = (k: TrackKind) => k === "folder" || k === "summing";
/** Tracks with an R button. */
export const canRecord = (k: TrackKind) => k === "audio" || k === "instrument";
export type InputSel = { type: "none" } | { type: "mono"; ch: number } | { type: "stereo"; ch: number };

/** An input as the strips and menus name it: "Input 1", "Input 1–2" or "No Input". */
export function inputLabel(sel: { type: string; ch?: number }): string {
  if (sel.type === "mono") return `Input ${(sel.ch ?? 0) + 1}`;
  if (sel.type === "stereo") return `Input ${(sel.ch ?? 0) + 1}–${(sel.ch ?? 0) + 2}`;
  return "No Input";
}

export interface PluginSlot {
  uid: Id;
  plugin_id: string;
  name: string;
  bypass: boolean;
  /** A built-in effect's values: [parameter id, plain value]; missing ids are at their default. */
  params?: [number, number][];
  /** The track keying the plug-in's side-chain (a built-in effect's, or a VST3's aux input). */
  sidechain?: Id | null;
  /** The side-chain key is taken before the source's fader (absent: after it). */
  sidechain_pre?: boolean;
  /** A VST3's state, only in what `copy_plugin` returns (for the clipboard). In a snapshot, a
   * sampler's has a fingerprint of its zones, which tells its panel when to read them again. */
  state?: string | null;
  /** The preset a built-in plug-in last loaded or saved, for its preset box. */
  preset?: PresetRef | null;
}

/** A preset by name: a factory one or one of the user's. */
export interface PresetRef {
  name: string;
  factory: boolean;
}

export interface Marker {
  id: Id;
  pos: number;
  name: string;
  color?: string | null;
  length?: number | null;
}

/** A section on the Arrangement track (see arrangement.ts). Sections tile the timeline. */
export interface ArrangementMarker {
  id: Id;
  name: string;
  start: number;
  length: number;
  /** None: coloured by its name. */
  color?: string | null;
}

export type BandKind = "low_cut" | "low_shelf" | "bell" | "high_shelf" | "high_cut";

export interface EqBand {
  kind: BandKind;
  on: boolean;
  freq: number;
  gain: number;
  q: number;
  slope: number;
}

/** A Channel EQ's settings as the editor works on them: the eight bands of a Channel EQ or Linear
 *  Phase EQ insert's parameters (eqParams.ts). */
export interface ChannelEq {
  on: boolean;
  bands: EqBand[];
  output_db: number;
}

export interface Track {
  id: Id;
  kind: TrackKind;
  name: string;
  color: string;
  volume_db: number;
  pan: number;
  mute: boolean;
  solo: boolean;
  armed: boolean;
  monitor: boolean;
  /** Track On/Off, switched off (saved only when true). A stack's subtracks go off with it. */
  off?: boolean;
  input: InputSel;
  instrument: PluginSlot | null;
  inserts: (PluginSlot | null)[];
  regions: Region[];
  /** The track stack this track is in. */
  parent: Id | null;
  /** For stack main tracks: subtracks hidden. */
  collapsed: boolean;
  group: Id | null;
  /** Hidden with Hide Track (still plays). */
  hidden: boolean;
  /** SEND_SLOTS send slots (older projects may have fewer until edited). */
  sends: (Send | null)[];
  /** An aux, MASTER_ID for Stereo Out, or null for the default route (the summing stack the
   *  track is in, else Stereo Out). */
  output: Id | null;
  /** For auxes: the bus feeding them ("Bus 3"). */
  bus: number;
  /** For auxes: the extra output of an instrument that feeds them too (see multiOut.ts). */
  instrument_output?: { track: Id; bus: number } | null;
  automation: AutoLane[];
  auto_mode: AutoMode;
  /** Set while frozen: the track plays this render, its plug-ins unloaded. */
  frozen?: FreezeInfo | null;
  /** The VCA whose level offsets this track's fader. */
  vca?: Id | null;
  /** Track Alternatives: the inactive ones (their regions aren't sent to the UI). Absent when
   *  the track has just one. */
  alternatives?: { name: string }[];
  /** The active track alternative's name; absent means "A". */
  alternative?: string;
  /** Extra automation lanes shown below the main lane. */
  auto_lanes?: AutoParam[];
  /** The icon chosen for the track (trackIcons.ts ids); absent shows the name's or kind's. */
  icon?: string | null;
  /** For an aux on an instrument's output, while the instrument is frozen: the render of that
   *  output, which the aux plays in its place. */
  frozen_output?: FreezeInfo | null;
}

export interface FreezeInfo {
  clip: Id;
  start: number;
}

/** What an automation lane drives. */
export type AutoParam =
  | { type: "volume" }
  | { type: "pan" }
  | { type: "mute" }
  | { type: "send"; value: number }
  | { type: "plugin"; value: { uid: Id; param: number; name: string } };

/** A breakpoint, in the parameter's units: dB, pan -1..1, mute 0/1, plug-in 0..1. */
export interface AutoPoint {
  pos: number;
  value: number;
  /** The segment to the next point: 0 (or absent) straight, towards 1 the change comes early,
   *  towards -1 late. */
  curve?: number;
}

export interface AutoLane {
  param: AutoParam;
  points: AutoPoint[];
}

export type AutoMode = "off" | "read" | "touch" | "latch" | "write";

/** A plug-in parameter, from `list_plugin_params`. */
export interface ParamInfo {
  id: number;
  name: string;
  units: string;
  default: number;
  steps: number;
}

/** A send slot: a copy of the strip's signal into an aux (`bus` is the aux's track id). */
export interface Send {
  bus: Id;
  level_db: number;
  /** Taps after the plug-ins, before the fader and pan. */
  pre_fader: boolean;
  muted: boolean;
}

export const SEND_SLOTS = 8;

export interface Group {
  id: Id;
  name: string;
  color: string;
  enabled: boolean;
  volume: boolean;
  pan: boolean;
  mute: boolean;
  solo: boolean;
  record: boolean;
  select: boolean;
  editing: boolean;
}

export interface Project {
  name: string;
  sample_rate: number;
  tempo: number;
  sig_num: number;
  sig_den: number;
  key: string;
  tracks: Track[];
  master: {
    volume_db: number;
    inserts: (PluginSlot | null)[];
    /** Stereo Out's automation (volume and plug-in parameters), shown on the Output track. */
    automation?: AutoLane[];
    auto_mode?: AutoMode;
    auto_lanes?: AutoParam[];
  };
  cycle: { enabled: boolean; start: number; end: number };
  metronome: boolean;
  count_in: boolean;
  groups: Group[];
  /** Metronome settings (Project Settings › Metronome). */
  click: ClickSettings;
  /** Overrides Settings › Recording › Bit Depth for this project. */
  bit_depth: BitDepth | null;
  notes: string;
  /** The Marker track, sorted by position. */
  markers: Marker[];
  /** The Autopunch range. */
  punch: { enabled: boolean; start: number; end: number };
  /** The Tempo track; empty means `tempo` throughout (see tempo.ts). */
  tempo_map: TempoEvent[];
  /** The Signature track; empty means `sig_num/sig_den` throughout. */
  signatures: SigEvent[];
  /** The Tracks area's drag mode: "xfade" crossfades audio regions that edits overlap. */
  drag_mode?: DragMode;
  /** The VCA faders, in the Mixer's order. */
  vcas?: Vca[];
  /** This project alternative's name (File › Project Alternatives); empty until the project is
   *  first saved, when it takes the project's name. */
  alternative: string;
  /** The Arrangement track's sections, in order. */
  arrangement: ArrangementMarker[];
  /** Edit › Snap Edits to Zero Crossings: audio splits and trims land on zero crossings. */
  snap_zero_crossings?: boolean;
}

/** A VCA fader: no audio, but its level offsets its strips' faders and its mute mutes them. */
export interface Vca {
  id: Id;
  name: string;
  color: string;
  /** -96 (silence) to +6 dB. */
  level_db: number;
  mute: boolean;
  /** The VCA this one is assigned to. */
  parent: Id | null;
}

export type BitDepth = "int24" | "float32";
export type ClickSound = "classic" | "click" | "woodblock" | "beep";

export interface ClickSettings {
  sound: ClickSound;
  volume_db: number;
  accent: boolean;
  bar_pitch: number;
  beat_pitch: number;
  /** Clicks per beat: 1, 2, 3 or 4. */
  division: number;
  while_playing: boolean;
  while_recording: boolean;
  only_count_in: boolean;
}

export const MASTER_ID = 0;
export const INSTRUMENT_SLOT = -1;
export const BUILTIN_SYNTH = "builtin:synth";

