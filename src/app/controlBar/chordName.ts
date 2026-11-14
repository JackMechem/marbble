// Names the chord held on MIDI input, for the LCD's Chord mode: "Am7", "G/B", "Cmaj7(♯11)". Each
// note of the chord is tried as its root; the simplest reading wins, a reading over its own root
// before one over another bass note.
import { noteName, pitchName, type Spelling } from "./noteNames.ts";

/** A chord over one root, from the intervals above it: its suffix and how unusual it is. */
interface Reading {
  suffix: string;
  cost: number;
}

/** What a chord costs beyond its triad: the less, the likelier the name. */
const COST = { seventh: 1, sixth: 1.1, natural: 1, altered: 1.5, added: 1.2, noFifth: 0.5, slash: 1.2 };

/** The chord over a root, from the set of intervals above it (bit `i` for `i` semitones; bit 0,
 *  the root, is set). Null when it isn't a chord with a name. */
function reading(set: number): Reading | null {
  const has = (i: number) => ((set >> i) & 1) === 1;
  if (set === (1 | (1 << 7))) return { suffix: "5", cost: 0.5 };
  let used = 1;
  const third = has(4) ? 4 : has(3) ? 3 : 0;
  let fifth = 0;
  if (has(7)) fifth = 7;
  else if (third === 3 && has(6)) fifth = 6;
  else if (third === 4 && has(8)) fifth = 8;
  else if (third === 4 && has(6)) fifth = 6;
  // Without a third, a fourth or second takes its place.
  const sus = third ? 0 : has(5) ? 4 : has(2) ? 2 : 0;
  let seventh = 0;
  if (has(10)) seventh = 10;
  else if (has(11)) seventh = 11;
  else if (third === 3 && fifth === 6 && has(9)) seventh = 9;
  if (!third && !sus) return null;
  if (!fifth && !seventh) return null;
  for (const i of [third, fifth, seventh, sus === 4 ? 5 : sus]) if (i) used |= 1 << i;

  // What's left is extensions: natural 9, 11 and 13, altered ones, or a sixth.
  const naturals: number[] = [];
  const altered: string[] = [];
  let sixth = false;
  for (let i = 1; i < 12; i++) {
    if (!(((set & ~used) >> i) & 1)) continue;
    if (i === 1) altered.push("♭9");
    else if (i === 2) naturals.push(9);
    else if (i === 3 && third === 4) altered.push("♯9");
    else if (i === 5) naturals.push(11);
    else if (i === 6 && fifth !== 0) altered.push("♯11");
    else if (i === 8 && fifth === 7) altered.push("♭13");
    else if (i === 9 && seventh) naturals.push(13);
    else if (i === 9) sixth = true;
    else return null;
  }

  const kind = !third ? "sus" : third === 3 ? (fifth === 6 ? "dim" : "min") : fifth === 8 ? "aug" : fifth === 6 ? "flat5" : "maj";
  let cost = { maj: 0, min: 0, dim: 0.5, aug: 0.5, flat5: 1.5, sus: 1 }[kind] + (fifth ? 0 : COST.noFifth);
  cost += naturals.length * COST.natural + altered.length * COST.altered;
  const marks = [...altered];
  let suffix: string;
  if (seventh) {
    cost += COST.seventh;
    // The highest natural extension names the chord ("9", "m11", "maj13"), implying those below.
    const ext = String(naturals.length ? Math.max(...naturals) : 7);
    const maj = seventh === 11 ? "maj" : "";
    if (kind === "maj") suffix = maj + ext;
    else if (kind === "min") suffix = seventh === 11 ? `m(maj${ext})` : "m" + ext;
    else if (kind === "sus") suffix = `${maj}${ext}sus${sus}`;
    else if (kind === "aug") suffix = `${maj}${ext}♯5`;
    else if (kind === "flat5") suffix = `${maj}${ext}♭5`;
    else {
      // Diminished: the extensions go in brackets. A major seventh over it reads better as a
      // ♭9 over another root, or not at all.
      if (seventh === 11) return null;
      suffix = seventh === 9 ? "dim7" : "m7♭5";
      if (naturals.length) marks.unshift(...naturals.map(String));
    }
  } else {
    // No seventh: alterations make no common name, and natural extensions are added notes.
    if (altered.length) return null;
    if (sixth && kind !== "maj" && kind !== "min" && kind !== "sus") return null;
    const base = { maj: "", min: "m", dim: "dim", aug: "aug", flat5: "(♭5)", sus: `sus${sus}` }[kind];
    let adds = naturals;
    if (sixth) {
      cost += COST.sixth;
      const nine = adds.includes(9);
      if (nine) adds = adds.filter((n) => n !== 9);
      suffix = kind === "sus" ? `6${base}` : `${base}6${nine ? "/9" : ""}`;
    } else suffix = base;
    cost += adds.length * (COST.added - COST.natural);
    if (adds.length === 1 && kind === "maj" && !sixth) suffix += `add${adds[0]}`;
    else marks.push(...adds.map((n) => `add${n}`));
  }
  return { suffix: marks.length ? `${suffix}(${marks.join(",")})` : suffix, cost };
}

/** The chord's name for a set of MIDI notes, or null when no chord name fits (fewer than two
 *  pitch classes, a cluster, an interval other than a fifth). */
export function chordName(notes: number[], sp: Spelling): string | null {
  if (!notes.length) return null;
  const bass = ((Math.min(...notes) % 12) + 12) % 12;
  const pcs = [...new Set(notes.map((n) => ((n % 12) + 12) % 12))];
  if (pcs.length < 2) return null;
  let best: { name: string; cost: number } | null = null;
  // The bass first, so a tie goes to the reading over it.
  for (const root of [bass, ...pcs.filter((p) => p !== bass)]) {
    const set = pcs.reduce((s, p) => s | (1 << ((p - root + 12) % 12)), 0);
    const r = reading(set);
    if (!r) continue;
    const slash = root !== bass;
    // A power chord over another note is just a fourth.
    if (slash && r.suffix === "5") continue;
    const cost = r.cost + (slash ? COST.slash : 0);
    if (!best || cost < best.cost) best = { name: pitchName(root, sp) + r.suffix + (slash ? "/" + pitchName(bass, sp) : ""), cost };
  }
  return best?.name ?? null;
}

/** What the Chord mode shows for the keys held: the chord's name, a single note's name, or (no
 *  chord fitting) the pitch classes, lowest first; and every note with its octave. */
export function describeHeld(notes: number[], sp: Spelling): { main: string; kind: "chord" | "note" | "notes"; notes: string } {
  const sorted = [...notes].sort((a, b) => a - b);
  const all = sorted.map((n) => noteName(n, sp)).join(" ");
  if (!sorted.length) return { main: "", kind: "notes", notes: "" };
  const pcs = [...new Set(sorted.map((n) => n % 12))];
  if (pcs.length === 1) return { main: noteName(sorted[0], sp), kind: "note", notes: sorted.length > 1 ? all : "" };
  const name = chordName(sorted, sp);
  if (name) return { main: name, kind: "chord", notes: all };
  return { main: pcs.map((p) => pitchName(p, sp)).join(" "), kind: "notes", notes: all };
}
