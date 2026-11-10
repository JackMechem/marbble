// Control surfaces (Settings › MIDI › Control Surfaces): a Mackie Control with its extenders, or
// a HUI, mirrored from crates/engine/src/control/surface/mod.rs. Pure, tested.
import type { MidiPort } from "./controllers";

export type SurfaceKind = "mcu" | "mcu_extender" | "hui";

export interface SurfaceConfig {
  kind: SurfaceKind;
  /** Input and output port keys ("" for none). */
  input: string;
  output: string;
  /** The unit's first strip from the bank's first track: 0 for the main unit, 8 for an extender
   *  to its right. */
  offset: number;
  /** F1–F8: the key command each runs ("" for none). */
  functions: string[];
}

/** A unit's state (`control_surfaces`). */
export interface SurfaceStatus {
  input: boolean;
  output: boolean;
  online: boolean;
  /** "Tracks 1–8", or "No tracks". */
  showing: string;
}

export const STRIPS = 8;
export const FUNCTION_KEYS = 8;

export const SURFACE_KINDS: [SurfaceKind, string][] = [
  ["mcu", "Mackie Control"],
  ["mcu_extender", "Mackie Control Extender"],
  ["hui", "HUI"],
];

export const kindName = (k: SurfaceKind) => SURFACE_KINDS.find((x) => x[0] === k)?.[1] ?? k;

/** Whether the unit has F1–F8 (an extender has none). */
export const hasFunctionKeys = (k: SurfaceKind) => k !== "mcu_extender";

/** How many strips the units cover together. */
export function stripsOf(units: SurfaceConfig[]): number {
  return units.reduce((n, u) => Math.max(n, u.offset + STRIPS), STRIPS);
}

/** Where a new unit goes: right of every unit so far. */
export function nextOffset(units: SurfaceConfig[]): number {
  return units.length ? stripsOf(units) : 0;
}

/** A guess at a new unit's port among `ports` (keys): one whose name says it's a surface of this
 *  kind and isn't taken by another unit, else none. */
export function guessPort(kind: SurfaceKind, ports: string[], taken: string[]): string {
  const free = ports.filter((p) => !taken.includes(p));
  const ext = /ext|xt\b|extender/i;
  const hui = /hui/i;
  const pick = free.find((p) => (kind === "hui" ? hui.test(p) : /mackie|mcu|x-?touch|qcon|platform m/i.test(p) && ext.test(p) === (kind === "mcu_extender") && !hui.test(p)));
  return pick ?? "";
}

/** A new unit: a Mackie Control first, extenders after it; ports guessed from the names. */
export function newSurface(units: SurfaceConfig[], inputs: string[], outputs: string[]): SurfaceConfig {
  const kind: SurfaceKind = units.some((u) => u.kind === "mcu") ? "mcu_extender" : units.some((u) => u.kind === "hui") ? "hui" : "mcu";
  return {
    kind,
    input: guessPort(kind, inputs, units.map((u) => u.input)),
    output: guessPort(kind, outputs, units.map((u) => u.output)),
    offset: nextOffset(units),
    functions: [],
  };
}

/** A port pop-up's options: "" for none, every port (by key, once each), and the chosen one even
 *  when it isn't there now. */
export function portOptions(ports: Pick<MidiPort, "key">[], current: string): [string, string][] {
  const keys = ports.map((p) => p.key).filter((k, i, all) => all.indexOf(k) === i);
  if (current && !keys.includes(current)) keys.push(current);
  return [["", "None"], ...keys.map((k): [string, string] => [k, current === k && !ports.some((p) => p.key === k) ? `${k} (not found)` : k])];
}

/** What the status line beside a unit says. */
export function statusText(s: SurfaceStatus | undefined, cfg: SurfaceConfig): string {
  if (!cfg.input || !cfg.output) return "Choose its input and output";
  if (!s) return "";
  if (!s.input && !s.output) return "Not connected";
  if (!s.input) return "Input not connected";
  if (!s.output) return "Output not connected";
  if (!s.online) return cfg.kind === "hui" ? "Waiting for the HUI to answer" : "Waiting for the surface to answer";
  return `Online · ${s.showing}`;
}

/** The F-key list with key `i` set to `id` ("" clears it), without trailing empty keys. */
export function setFunction(functions: string[], i: number, id: string): string[] {
  const next = [...functions];
  while (next.length <= i) next.push("");
  next[i] = id;
  while (next.length && !next[next.length - 1]) next.pop();
  return next;
}
