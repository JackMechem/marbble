// Controller assignments (Logic's Control Surfaces › Learn Assignment and Controller
// Assignments): the types the engine uses (crates/engine/src/controllers.rs) and how the
// window and the right-click menus show them. Learning and the menus are in learn.ts.
//
// This module only imports types, so it runs under plain Node for the tests in tests/.
import type { AutoParam, Id } from "./api";

export type CtlKind = { type: "cc"; number: number } | { type: "pitch_bend" };

/** A physical control: its port (without ALSA's client:port numbers), channel 0..15 and kind. */
export interface Controller {
  port: string;
  channel: number;
  kind: CtlKind;
}

export type TransportAction = "play" | "record" | "cycle";
export type EqField = "on" | "freq" | "gain" | "q";

/** What a controller drives. Tracks and plug-ins by id; `track` 0 is Stereo Out. */
export type CtlTarget =
  | { type: "volume"; track: Id }
  | { type: "pan"; track: Id }
  | { type: "mute"; track: Id }
  | { type: "solo"; track: Id }
  | { type: "arm"; track: Id }
  | { type: "send"; track: Id; slot: number }
  /** `plugin` may be left empty: the engine fills in the slot's plug-in id. */
  | { type: "plugin"; track: Id; uid: Id; plugin: string; param: number }
  /** Learnt before the Channel EQ was an insert: the engine drives that setting of the strip's
   *  first Channel EQ insert. New assignments to a Channel EQ are "plugin" ones. */
  | { type: "eq"; track: Id; band: number; field: EqField }
  | { type: "eq_output"; track: Id }
  | { type: "master_volume" }
  | { type: "transport"; action: TransportAction };

export type Takeover = "jump" | "pickup";

export interface Assignment {
  controller: Controller;
  target: CtlTarget;
  mode: Takeover;
  /** The stretch of the parameter's travel (0..1) the controller sweeps; min > max inverts. */
  min: number;
  max: number;
  /** Send the target's value back to the controller when its input has a paired output. */
  feedback: boolean;
}

/** A row of the Controller Assignments window (`controller_assignments`). */
export interface AssignmentRow extends Assignment {
  /** "Bass › Volume", or what it was when the track or plug-in isn't in this project. */
  name: string;
  available: boolean;
  /** A switch or transport button: no mode or range. */
  button: boolean;
  /** The output (its port key) feedback goes to, or null when none is paired. */
  output: string | null;
}

export const MODE_OPTIONS: [Takeover, string][] = [
  ["jump", "Jump"],
  ["pickup", "Pickup"],
];

/** The transport buttons the window can learn. */
export const TRANSPORT_TARGETS: { label: string; target: CtlTarget }[] = [
  { label: "Play / Stop", target: { type: "transport", action: "play" } },
  { label: "Record", target: { type: "transport", action: "record" } },
  { label: "Cycle", target: { type: "transport", action: "cycle" } },
];

/** "CC 7" or "Pitch Bend". */
export function controllerLabel(c: Controller): string {
  return c.kind.type === "cc" ? `CC ${c.kind.number}` : "Pitch Bend";
}

/** The Feedback checkbox's tooltip: where the value goes, or why nothing does. */
export function feedbackTitle(row: Pick<AssignmentRow, "feedback" | "output" | "controller">): string {
  if (!row.output) return `No MIDI output is paired with ${row.controller.port}: choose one in Settings › MIDI, and feedback starts`;
  return `${row.feedback ? "Sends" : "Send"} the parameter's value back to ${row.output}, for button lights and motor faders`;
}

/** A MIDI port as Settings › MIDI lists it (`midi_ports`, `midi_outputs`). */
export interface MidiPort {
  name: string;
  /** The name without ALSA's client:port numbers, as assignments and pairings name ports. */
  key: string;
  enabled: boolean;
  connected: boolean;
  /** For an input: the output its feedback goes to on Automatic. */
  auto_output: string | null;
}

/** What an input's Feedback pop-up shows: "auto", "none" or "out:" and an output's key. */
export function feedbackChoice(chosen: Record<string, string> | undefined, input: string): string {
  const c = chosen?.[input];
  return c === undefined ? "auto" : c === "" ? "none" : `out:${c}`;
}

/** The pairing to save for a pop-up choice: undefined for Automatic, "" for none. */
export function feedbackSetting(choice: string): string | undefined {
  return choice === "auto" ? undefined : choice === "none" ? "" : choice.slice(4);
}

/** An input's Feedback pop-up: Automatic (naming what it picks), none, and every output. A chosen
 *  output that isn't connected now stays listed so the choice shows. */
export function feedbackOptions(input: Pick<MidiPort, "auto_output">, outputs: Pick<MidiPort, "key">[], current: string): [string, string][] {
  const keys = outputs.map((o) => o.key).filter((k, i, all) => all.indexOf(k) === i);
  if (current.startsWith("out:") && !keys.includes(current.slice(4))) keys.push(current.slice(4));
  return [
    ["auto", input.auto_output ? `Automatic (${input.auto_output})` : "Automatic (none found)"],
    ["none", "No feedback"],
    ...keys.map((k): [string, string] => [`out:${k}`, k]),
  ];
}

/** "Ch 1" (channels are shown 1-based). */
export function channelLabel(c: Controller): string {
  return `Ch ${c.channel + 1}`;
}

/** A range end (0..1) as a whole percentage of the parameter's travel. */
export function toPercent(v: number): number {
  return Math.round(Math.min(1, Math.max(0, v)) * 100);
}

export function fromPercent(p: number): number {
  return Math.min(100, Math.max(0, p)) / 100;
}

/** The target for an automation lane's parameter (how VST3 parameters are learned from a lane). */
export function autoParamTarget(track: Id, p: AutoParam): CtlTarget {
  // Stereo Out (the Output track, id 0) has its own fader target.
  if (track === 0 && p.type === "volume") return { type: "master_volume" };
  switch (p.type) {
    case "volume":
      return { type: "volume", track };
    case "pan":
      return { type: "pan", track };
    case "mute":
      return { type: "mute", track };
    case "send":
      return { type: "send", track, slot: p.value };
    case "plugin":
      return { type: "plugin", track, uid: p.value.uid, plugin: "", param: p.value.param };
  }
}

/**
 * The channel strip controls the right-click Learn item knows, found by their class names. The
 * Mute, Solo and Record buttons switch on any mouse button, so they are learned with Learn
 * Assignment (click, then the key) instead.
 */
const STRIP_CONTROLS: [selector: string, kind: "volume" | "pan"][] = [
  [".strip-fader", "volume"],
  [".strip-pan .knob", "pan"],
];

/**
 * The target of a channel strip control that was right-clicked: `matches(selector)` says
 * whether the click was inside the control (`Element.closest`). `track` is null on Stereo Out,
 * which has no pan knob.
 */
export function stripTarget(track: Id | null, matches: (selector: string) => boolean): CtlTarget | null {
  const hit = STRIP_CONTROLS.find(([sel]) => matches(sel));
  if (!hit) return null;
  if (track === null) return hit[1] === "volume" ? { type: "master_volume" } : null;
  return { type: hit[1], track };
}
