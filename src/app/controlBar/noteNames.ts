// Note names for the LCD's Tuner and Chord modes, spelt to suit the project's key.

/** Sharps (G major, E minor...), flats (F major, D minor...) or, in C major and A minor, the
 *  usual mix: C♯, E♭, F♯, A♭, B♭. */
export type Spelling = "sharps" | "flats" | "mixed";

const NAMES: Record<Spelling, string[]> = {
  sharps: ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"],
  flats: ["C", "D♭", "D", "E♭", "E", "F", "G♭", "G", "A♭", "A", "B♭", "B"],
  mixed: ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"],
};

const LETTERS: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };

/** The spelling for a project key such as "E♭ minor": a root written with a sharp or flat keeps
 *  it, a natural one goes by its key signature. */
export function spellingFor(key: string): Spelling {
  const [root = "", mode] = key.split(" ");
  const letter = LETTERS[root[0]];
  if (letter === undefined) return "mixed";
  if (root.includes("♯") || root.includes("#")) return "sharps";
  if (root.includes("♭") || root.includes("b")) return "flats";
  // Sharps in the major key's signature, by the circle of fifths: 1 to 5 are sharps, 7 to 11 flats.
  const major = (letter + (mode === "minor" ? 3 : 0)) % 12;
  const sharps = (major * 7) % 12;
  return sharps === 0 ? "mixed" : sharps < 6 ? "sharps" : "flats";
}

/** A pitch class's name (0 is C). */
export function pitchName(pc: number, sp: Spelling): string {
  return NAMES[sp][((pc % 12) + 12) % 12];
}

/** A MIDI note's name with its octave, as Logic numbers them: middle C (60) is C3. */
export function noteName(note: number, sp: Spelling): string {
  return pitchName(note, sp) + (Math.floor(note / 12) - 2);
}
