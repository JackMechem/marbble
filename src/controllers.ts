// Controller assignments (Logic's Control Surfaces › Learn Assignment and Controller
// Assignments): the types the engine uses (crates/engine/src/controllers.rs) and how the
// window and the right-click menus show them. Learning and the menus are in learn.ts.
//
// This module only imports types, so it runs under plain Node for the tests in tests/.
import type { AutoParam, Id } from "./api";

export type CtlKind =
  | { type: "cc"; number: number }
  | { type: "pitch_bend" }
  | { type: "note"; number: number }
  | { type: "program"; number: number }
  /** CC `number` (0..31) and `number` + 32 as one 14-bit controller. */
  | { type: "cc14"; number: number }
  /** A non-registered parameter number, 0..16383. */
  | { type: "nrpn"; number: number };

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
  | { type: "transport"; action: TransportAction }
  /** A key command (keymap.ts id) or a menu item with a key, run by the main window as its key. */
  | { type: "command"; id: string }
  | { type: "monitor"; track: Id }
  /** An Audio FX slot's bypass. */
  | { type: "bypass"; track: Id; slot: number }
  | { type: "vca_level"; vca: Id }
  | { type: "vca_mute"; vca: Id }
  /** A control of whichever track is selected when the controller moves. */
  | { type: "selected"; param: SelectedParam };

/** A Selected Track control (`plugin_param`: the focused plug-in's parameter by position). */
export type SelectedParam =
  | { type: "volume" }
  | { type: "pan" }
  | { type: "mute" }
  | { type: "solo" }
  | { type: "arm" }
  | { type: "monitor" }
  | { type: "send"; slot: number }
  | { type: "bypass"; slot: number }
  | { type: "plugin_param"; index: number };

export type Takeover = "jump" | "pickup";

/** How a control change is read: a position, or a relative encoder's steps in one of three codes. */
export type Encoding = "absolute" | "twos_complement" | "sign_magnitude" | "binary_offset";

/** What a button's press and release do: Toggle flips, Momentary is on while held, Trigger
 *  switches on (or fires) once per press. */
export type ButtonMode = "toggle" | "momentary" | "trigger";

export interface Assignment {
  controller: Controller;
  target: CtlTarget;
  mode: Takeover;
  /** The stretch of the parameter's travel (0..1) the controller sweeps; min > max inverts. */
  min: number;
  max: number;
  /** For a note or program change, or a switch. */
  button_mode: ButtonMode;
  /** A note on a continuous parameter: its velocity picks the value in the range (else the top). */
  velocity: boolean;
  /** The button latches by itself (on at one press, off at the next): every message is a press. */
  latching: boolean;
  /** Send the target's value back to the controller when its input has a paired output. */
  feedback: boolean;
  encoding: Encoding;
  /** A relative encoder's speed: at 1 a step is 1/127 of the range. */
  sensitivity: number;
}

/** The settings `update_controller_assignment` changes; the ones left out stay. */
export type AssignmentPatch = Partial<Pick<Assignment, "mode" | "min" | "max" | "button_mode" | "velocity" | "latching" | "encoding" | "sensitivity">>;

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

/** The Mode pop-up of a 7-bit control change: a takeover mode, or a relative encoding. */
export type CcMode = Takeover | Exclude<Encoding, "absolute">;

export const CC_MODE_OPTIONS: [CcMode, string][] = [
  ...MODE_OPTIONS,
  ["twos_complement", "Rel. 2's Compl."],
  ["sign_magnitude", "Rel. Signed Bit"],
  ["binary_offset", "Rel. Offset 64"],
];

/** A row's Mode pop-up value: its relative encoding, else its takeover mode. */
export const ccMode = (a: Pick<Assignment, "mode" | "encoding">): CcMode => (a.encoding === "absolute" ? a.mode : a.encoding);

/** The change choosing `m` in the Mode pop-up makes. */
export function ccModePatch(m: CcMode): AssignmentPatch {
  return m === "jump" || m === "pickup" ? { mode: m, encoding: "absolute" } : { encoding: m };
}

/** Only 7-bit control changes can be relative encoders. */
export const canBeRelative = (k: CtlKind) => k.type === "cc";

/** Notes and program changes are buttons whatever they drive. */
export const isButtonKind = (k: CtlKind) => k.type === "note" || k.type === "program";

/**
 * What a row of the window edits: a controller moving a continuous parameter has a takeover mode
 * and a range ("takeover"); a button (a note or program change) on one has a button mode and a
 * range ("button-range"); a switch or transport button has only a button mode ("button").
 */
export function rowEditor(row: Pick<AssignmentRow, "controller" | "button">): "takeover" | "button-range" | "button" {
  if (row.button) return "button";
  return isButtonKind(row.controller.kind) ? "button-range" : "takeover";
}

const BUTTON_MODES: [ButtonMode, string][] = [
  ["toggle", "Toggle"],
  ["momentary", "Momentary"],
  ["trigger", "Trigger"],
];

/** The button modes a row offers. A program change has no release, so no Momentary. */
export function buttonOptions(row: Pick<AssignmentRow, "controller">): [ButtonMode, string][] {
  return row.controller.kind.type === "program" ? BUTTON_MODES.filter(([m]) => m !== "momentary") : BUTTON_MODES;
}

/** What a switch's or command's button mode does, in a few words, for the window. */
export function buttonHint(mode: ButtonMode, command = false): string {
  if (command) return mode === "momentary" ? "Runs on press and release" : "Runs on press";
  return mode === "toggle" ? "Press: on or off" : mode === "momentary" ? "On while held" : "Press: on";
}

/** Whether a row offers Latching: a switch driven by a button that can have a release. */
export const offersLatching = (row: Pick<AssignmentRow, "controller" | "button">) => row.button && row.controller.kind.type !== "program";

/** Learn Selected Track…'s menu: the selected track's controls, sends, insert bypasses and its
 *  focused plug-in's first parameters (`plugins` of them). */
export function selectedTargets(plugins = 16): { label: string; target?: CtlTarget; children?: { label: string; target: CtlTarget }[] }[] {
  const sel = (param: SelectedParam): CtlTarget => ({ type: "selected", param });
  const range = (n: number) => [...Array(n).keys()];
  return [
    { label: "Volume", target: sel({ type: "volume" }) },
    { label: "Pan", target: sel({ type: "pan" }) },
    { label: "Mute", target: sel({ type: "mute" }) },
    { label: "Solo", target: sel({ type: "solo" }) },
    { label: "Record Enable", target: sel({ type: "arm" }) },
    { label: "Input Monitoring", target: sel({ type: "monitor" }) },
    { label: "Send", children: range(8).map((i) => ({ label: `Send ${i + 1}`, target: sel({ type: "send", slot: i }) })) },
    { label: "Insert Bypass", children: range(8).map((i) => ({ label: `Insert ${i + 1} Bypass`, target: sel({ type: "bypass", slot: i }) })) },
    { label: "Focused Plug-in", children: range(plugins).map((i) => ({ label: `Parameter ${i + 1}`, target: sel({ type: "plugin_param", index: i }) })) },
  ];
}

/** The transport buttons the window can learn. */
export const TRANSPORT_TARGETS: { label: string; target: CtlTarget }[] = [
  { label: "Play / Stop", target: { type: "transport", action: "play" } },
  { label: "Record", target: { type: "transport", action: "record" } },
  { label: "Cycle", target: { type: "transport", action: "cycle" } },
];

const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/** A note's name, Logic-style: C3 is middle C (60). */
export const noteName = (n: number) => `${NOTE_NAMES[n % 12]}${Math.floor(n / 12) - 2}`;

/** "CC 7", "Pitch Bend", "Note C1" or "Program 5" (numbered from 1, as Settings › MIDI shows them). */
export function controllerLabel(c: Controller): string {
  switch (c.kind.type) {
    case "cc":
      return `CC ${c.kind.number}`;
    case "pitch_bend":
      return "Pitch Bend";
    case "note":
      return `Note ${noteName(c.kind.number)}`;
    case "program":
      return `Program ${c.kind.number + 1}`;
    case "cc14":
      return `CC ${c.kind.number}/${c.kind.number + 32}`;
    case "nrpn":
      return `NRPN ${c.kind.number}`;
  }
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
