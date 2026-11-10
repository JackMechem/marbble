// What a built-in instrument's panel section shows (plugins/instruments/Instrument.tsx): its parameters with names
// that don't repeat the section's ("Range" under Oscillator 1, not "Osc 1 Range"), an "X On"
// switch folded into the knob it switches, and the section's own on/off in its header. The
// parameters keep their full names everywhere else (automation lanes, tooltips, MIDI learn).
import type { FxParamKind } from "../builtinEffects.ts";

/** What a section needs to know of a parameter. */
export interface SectionParam {
  id: number;
  name: string;
  kind: FxParamKind;
}

/** One place in a section's grid. */
export type SectionCell =
  /** A knob, choice or switch. */
  | { type: "control"; id: number; label: string }
  /** A knob with the switch that turns its part on and off in its label (Tre's mixer, glide). */
  | { type: "switched"; id: number; toggle: number; label: string };

export interface SectionView {
  /** The switch that turns the section's own part on and off (Sedici's Op 1 On), shown in its header. */
  power: number | null;
  cells: SectionCell[];
  /** Up to four cells in one line, more in two. */
  rows: 1 | 2;
}

/**
 * Labels the generic rules get wrong, by plug-in and parameter name: a name that is the section's
 * own where "Mode" would be wrong, or a short name that another parameter in the section has.
 */
const LABELS: Record<string, Record<string, string>> = {
  // Voices (how many) sits beside Voice Mode's "Mode"; the Decay switch, which makes key-up
  // release at the Decay time, beside the Decay knob.
  "builtin:tre": { Voices: "Count", Decay: "Release" },
  "builtin:sampler": { Filter: "Type" },
  "builtin:onda": { "A Unison": "Voices", "B Unison": "Voices" },
};

/** Words that name the same thing, so "Osc 1 Range" sits under "Oscillator 1". */
const SAME: Record<string, string> = { osc: "oscillator", op: "operator", env: "envelope", voice: "voices" };
const norm = (w: string) => {
  const l = w.toLowerCase();
  return SAME[l] ?? l;
};

/**
 * A parameter's name without the leading words its section's title already says: "Osc 1 Range"
 * under Oscillator 1 is "Range", "Filter Attack" under Filter Contour is "Attack". A name that's
 * all title words comes back empty.
 */
export function shortName(title: string, name: string): string {
  const said = new Set(title.split(/\s+/).map(norm));
  const words = name.split(/\s+/).filter(Boolean);
  let i = 0;
  while (i < words.length && said.has(norm(words[i]))) i++;
  const rest = words.slice(i).join(" ");
  // "Accent at Velocity" under Accent: "At Velocity".
  return rest.charAt(0).toUpperCase() + rest.slice(1);
}

const ON = / On$/;

/** Lays out one section of `plugin`'s panel. */
export function sectionView(plugin: string, title: string, params: SectionParam[]): SectionView {
  const fixed = LABELS[plugin] ?? {};
  let power: number | null = null;
  const label = new Map<number, string>();
  for (const p of params) label.set(p.id, fixed[p.name] ?? shortName(title, p.name));
  // The section's own switch: a toggle that says no more than "On" here ("Op 1 On" in Operator 1).
  const own = params.find((p) => p.kind.type === "toggle" && label.get(p.id) === "On");
  if (own) power = own.id;
  // "Osc 1 On" switches the knob "Osc 1" in the same section.
  const switchOf = new Map<number, number>();
  const folded = new Set<number>();
  for (const p of params) {
    const l = label.get(p.id)!;
    if (p.kind.type !== "toggle" || p.id === power || !ON.test(l)) continue;
    const knob = params.find((k) => k.kind.type === "continuous" && !switchOf.has(k.id) && label.get(k.id) === l.replace(ON, ""));
    if (knob) {
      switchOf.set(knob.id, p.id);
      folded.add(p.id);
    } else label.set(p.id, l.replace(ON, ""));
  }
  // A name that is all the title's: a choice is the section's mode, a knob its amount.
  const taken = new Set(label.values());
  for (const p of params) {
    if (label.get(p.id) !== "") continue;
    const l = p.kind.type === "choice" ? "Mode" : p.kind.type === "continuous" ? "Amount" : "On";
    label.set(p.id, taken.has(l) ? p.name : l);
    taken.add(l);
  }
  const cells: SectionCell[] = [];
  for (const p of params) {
    if (p.id === power || folded.has(p.id)) continue;
    const toggle = switchOf.get(p.id);
    cells.push(toggle === undefined ? { type: "control", id: p.id, label: label.get(p.id)! } : { type: "switched", id: p.id, toggle, label: label.get(p.id)! });
  }
  return { power, cells, rows: cells.length > 4 ? 2 : 1 };
}
