// App-wide settings (everything in Settings except the audio devices), mirrored from
// src-tauri/src/settings.rs: reading, watching and saving them. The launch sequence and crash
// recovery they drive are in project/startup.ts.
import { useSyncExternalStore } from "react";
import { type BitDepth, type ClickSettings, type Project } from "../engine/project";
import type { PluginDesc } from "../engine/listings";
import { act, call } from "../engine/commands";
import { setState, type Snap } from "../state/store";
import { sampleStep } from "../tracks/regions/musicalMove";
import { tempoMap } from "../lib/tempo";
import type { SurfaceConfig } from "../controllers/surfaces";
import type { UpdateChannel } from "../app/updates/updateCheck";
import type { WheelScrolls } from "../ui/wheel";
import type { MeterPlace } from "../app/cpuMeter/cpuMeterModel";

export type AudioOverlap = "on_top" | "new_track" | "replace" | "take_folders";
export type MidiOverlap = "new_region" | "merge" | "replace" | "take_folders";
export type TakeNames = "track_number" | "track" | "project_track";
export type VelocityCurve = "linear" | "soft" | "hard" | "fixed";
export type Startup = "chooser" | "last_project" | "empty_project";
export type Nudge = "bar" | "beat" | "8" | "16" | "32" | "ms10" | "ms1" | "smp50" | "smp10" | "smp5" | "smp2" | "smp1";
export type PdcMode = "all" | "off";
export type ClickLocates = "anywhere" | "empty" | "off";
export type MoveAutomation = "never" | "always" | "ask";

/** Session behaviour (crates/engine/src/prefs.rs). */
export interface Prefs {
  general: { project_folder: string | null; autosave_minutes: number; undo_limit: number; autosave_after_recording: boolean; backups: number };
  recording: {
    bit_depth: BitDepth;
    count_in_bars: number;
    pre_roll_secs: number;
    audio_overlap: AudioOverlap;
    midi_overlap: MidiOverlap;
    take_names: TakeNames;
    folder: string | null;
    /** The control bar's Replace button: every recording replaces what it overlaps. */
    replace?: boolean;
    /** Give each new take in a take folder its own colour. */
    color_takes?: boolean;
  };
  midi: {
    /** Input ports left unconnected, by port key (`portKey`; older settings: full names). */
    disabled_inputs: string[];
    /** Output ports left unconnected (controller feedback), by port key. */
    disabled_outputs?: string[];
    /** Input port key → the output key its controllers' feedback goes to ("" for none); inputs
     *  left out pair automatically. */
    feedback_outputs?: Record<string, string>;
    thru: boolean;
    velocity: VelocityCurve;
    fixed_velocity: number;
    /** Chase: Notes and Controllers when playback starts or jumps mid-region. */
    chase_notes?: boolean;
    chase_controllers?: boolean;
    /** Settings › MIDI › Control Surfaces (surfaces.ts). */
    surfaces?: SurfaceConfig[];
  };
  /** Crossfade Time (ms) and Curve (-1..1) for comp boundaries, X-Fade and Crossfade. */
  editing: { fade_ms: number; move_automation?: MoveAutomation; crossfade_ms?: number; crossfade_curve?: number };
  /** Metronome settings for new projects. */
  click: ClickSettings;
  /** Settings › Audio: plug-in delay compensation and Low Latency Mode. */
  latency: { compensation: PdcMode; low_latency: boolean; low_latency_limit_ms: number };
}

export interface UiPrefs {
  startup: Startup;
  confirm_unsaved: boolean;
  snap: Snap;
  nudge: Nudge;
  /** Key command overrides: command id -> keys (an empty list removes the default). */
  keymap: Record<string, string[]>;
  /** Settings › Appearance (see appearance.ts); null until first saved. */
  theme?: string | null;
  ui_scale?: number | null;
  wave_style?: string | null;
  /** Settings › Editing: what a click in the tracks area does to the playhead. */
  click_locates?: ClickLocates;
  /** Settings › Editing: what the mouse wheel scrolls in the Tracks area and Piano Roll. */
  wheel_scrolls?: WheelScrolls;
  /** Settings › General: where built-in effect panels open (fxDockModel.ts). */
  fx_panels?: "docked" | "floating" | "window";
  /** Settings › General: the update channel; null until chosen, when it follows the build
   * (updateCheck.ts `effectiveChannel`). */
  update_channel?: UpdateChannel | null;
  /** Settings › General: check for updates at launch (updates.ts). */
  check_updates?: boolean;
  /** Settings › Appearance: where the CPU meter shows (cpuMeterModel.ts `meterPlace`). */
  cpu_meter?: MeterPlace;
}

export interface PluginPrefs {
  disabled: string[];
  folders: string[];
}

export interface AppSettings {
  prefs: Prefs;
  ui: UiPrefs;
  plugins: PluginPrefs;
}

export const DEFAULT_CLICK: ClickSettings = {
  sound: "classic",
  volume_db: 0,
  accent: true,
  bar_pitch: 0,
  beat_pitch: 0,
  division: 1,
  while_playing: true,
  while_recording: true,
  only_count_in: false,
};

/** Used until the real settings arrive (and if they can't be read). */
const DEFAULTS: AppSettings = {
  prefs: {
    general: { project_folder: null, autosave_minutes: 2, undo_limit: 100, autosave_after_recording: true, backups: 10 },
    recording: {
      bit_depth: "float32",
      count_in_bars: 1,
      pre_roll_secs: 0,
      audio_overlap: "take_folders",
      midi_overlap: "new_region",
      take_names: "track_number",
      folder: null,
      replace: false,
      color_takes: false,
    },
    midi: { disabled_inputs: [], disabled_outputs: [], feedback_outputs: {}, thru: true, velocity: "linear", fixed_velocity: 100, chase_notes: true, chase_controllers: true, surfaces: [] },
    editing: { fade_ms: 0, move_automation: "never", crossfade_ms: 20, crossfade_curve: 0.5 },
    click: DEFAULT_CLICK,
    latency: { compensation: "all", low_latency: false, low_latency_limit_ms: 5 },
  },
  ui: { startup: "chooser", confirm_unsaved: true, snap: "smart", nudge: "16", keymap: {} },
  plugins: { disabled: [], folders: [] },
};

let settings: AppSettings = DEFAULTS;
const listeners = new Set<() => void>();

export function getSettings(): AppSettings {
  return settings;
}

export function useSettings<T>(sel: (s: AppSettings) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => sel(settings),
  );
}

/** Calls `l` whenever the settings change (detach.ts passes them to the other windows). */
export function onSettingsChange(l: () => void): () => void {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

/** Takes settings another window changed (and already saved). */
export function receiveSettings(s: AppSettings) {
  settings = s;
  listeners.forEach((l) => l());
}

export async function loadSettings() {
  const s = await call<AppSettings>("get_settings");
  if (s) settings = s;
  listeners.forEach((l) => l());
  setState({ snapMode: settings.ui.snap });
}

/** Changes settings, applies them and saves them. */
export async function updateSettings(f: (s: AppSettings) => AppSettings) {
  const before = settings;
  settings = f(settings);
  listeners.forEach((l) => l());
  await act("set_settings", { settings });
  if (JSON.stringify(before.plugins.disabled) !== JSON.stringify(settings.plugins.disabled)) await refreshPlugins();
}

/** Turns the control bar's Replace mode on or off. */
export function toggleReplace() {
  return updateSettings((st) => ({ ...st, prefs: { ...st.prefs, recording: { ...st.prefs.recording, replace: !st.prefs.recording.replace } } }));
}

/** Reloads the plug-ins offered in the Library and slot menus. */
async function refreshPlugins() {
  const p = await call<PluginDesc[]>("list_plugins");
  if (p) setState({ plugins: p });
}

/** The Nudge step in samples for the current project, at the tempo and signature at `pos`. */
export function nudgeSamples(p: Project, pos = 0, n: Nudge = settings.ui.nudge): number {
  const m = tempoMap(p);
  if (n === "bar") return m.samplesPerBarAt(pos);
  if (n === "beat") return m.samplesPerBeatAt(pos);
  if (n === "ms10") return p.sample_rate / 100;
  if (n === "ms1") return p.sample_rate / 1000;
  const smp = sampleStep(n);
  if (smp !== null) return smp;
  // Note values: 8 = eighth note, relative to a quarter.
  return m.samplesPerQuarterAt(pos) * (4 / Number(n));
}

