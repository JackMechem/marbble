// Unit tests for src/app/controlBar/chordName.ts and noteNames.ts: naming the chord held on MIDI
// input for the LCD's Chord mode, and spelling notes to suit the project's key.
import { test } from "node:test";
import assert from "node:assert/strict";
import { chordName, describeHeld } from "../../../src/app/controlBar/chordName.ts";
import { noteName, spellingFor } from "../../../src/app/controlBar/noteNames.ts";

/** MIDI notes from names in Logic's octaves (middle C is C3), e.g. "C3 E3 G3". */
function n(names: string): number[] {
  const pcs: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
  return names.split(" ").map((s) => {
    const m = /^([A-G])([#b]?)(-?\d)$/.exec(s)!;
    return (Number(m[3]) + 2) * 12 + pcs[m[1]] + (m[2] === "#" ? 1 : m[2] === "b" ? -1 : 0);
  });
}

const name = (notes: string, sp: "sharps" | "flats" | "mixed" = "mixed") => chordName(n(notes), sp);

test("triads in root position", () => {
  assert.equal(name("C3 E3 G3"), "C");
  assert.equal(name("A2 C3 E3"), "Am");
  assert.equal(name("B2 D3 F3"), "Bdim");
  assert.equal(name("C3 E3 G#3"), "Caug");
  assert.equal(name("C3 F3 G3"), "Csus4");
  assert.equal(name("C3 D3 G3"), "Csus2");
  assert.equal(name("C3 G3"), "C5");
});

test("doubled notes and wide voicings name the same chord", () => {
  assert.equal(name("C2 G2 C3 E3 G3 C4"), "C");
  assert.equal(name("A1 E2 C4"), "Am");
});

test("inversions are slash chords over their bass", () => {
  assert.equal(name("B2 D3 G3"), "G/B");
  assert.equal(name("E3 G3 C4"), "C/E");
  assert.equal(name("G2 C3 E3"), "C/G");
  assert.equal(name("B2 D3 F3 G3"), "G7/B");
  assert.equal(name("Bb2 C3 E3 G3"), "C7/B♭");
});

test("sevenths", () => {
  assert.equal(name("C3 E3 G3 Bb3"), "C7");
  assert.equal(name("C3 E3 G3 B3"), "Cmaj7");
  assert.equal(name("A2 C3 E3 G3"), "Am7");
  assert.equal(name("C3 Eb3 G3 B3"), "Cm(maj7)");
  assert.equal(name("B2 D3 F3 A3"), "Bm7♭5");
  assert.equal(name("B2 D3 F3 Ab3"), "Bdim7");
  assert.equal(name("C3 F3 G3 Bb3"), "C7sus4");
  assert.equal(name("C3 E3 G#3 Bb3"), "C7♯5");
  assert.equal(name("C3 E3 Gb3 Bb3"), "C7♭5");
  // A shell voicing without its fifth.
  assert.equal(name("G2 B2 F3"), "G7");
});

test("sixths beat the seventh chord over another root", () => {
  assert.equal(name("C3 E3 G3 A3"), "C6");
  assert.equal(name("A2 C3 E3 G3"), "Am7");
  assert.equal(name("C3 Eb3 G3 A3"), "Cm6");
  assert.equal(name("C3 E3 G3 A3 D4"), "C6/9");
  assert.equal(name("C3 F3 G3 A3"), "C6sus4");
});

test("extensions and alterations", () => {
  assert.equal(name("C3 E3 G3 Bb3 D4"), "C9");
  assert.equal(name("C3 E3 G3 B3 D4"), "Cmaj9");
  assert.equal(name("C3 Eb3 G3 Bb3 D4"), "Cm9");
  assert.equal(name("C3 Eb3 G3 Bb3 D4 F4"), "Cm11");
  assert.equal(name("C3 E3 G3 Bb3 A4"), "C13");
  assert.equal(name("C3 E3 G3 B3 F#4"), "Cmaj7(♯11)");
  assert.equal(name("C3 E3 G3 B3 D4 F#4"), "Cmaj9(♯11)");
  assert.equal(name("C3 E3 G3 Bb3 Db4"), "C7(♭9)");
  assert.equal(name("C3 E3 G3 Bb3 Eb4"), "C7(♯9)");
  assert.equal(name("C3 E3 G3 Bb3 Db4 Ab4"), "C7(♭9,♭13)");
  assert.equal(name("C3 F3 G3 Bb3 D4"), "C9sus4");
  assert.equal(name("B2 D3 F3 A3 C#4"), "Bm7♭5(9)");
});

test("added notes", () => {
  assert.equal(name("C3 D3 E3 G3"), "Cadd9");
  assert.equal(name("C3 Eb3 G3 D4"), "Cm(add9)");
  assert.equal(name("C3 E3 F3 G3"), "Cadd11");
});

test("symmetric chords take their bass as the root", () => {
  assert.equal(name("E3 G#3 C4"), "Eaug");
  assert.equal(name("Ab2 C3 E3"), "A♭aug");
  assert.equal(name("D3 F3 Ab3 B3"), "Ddim7");
});

test("what isn't a chord has no name", () => {
  assert.equal(name("C3"), null);
  assert.equal(name("C3 C4"), null);
  // Thirds and other intervals but the fifth are listed rather than named.
  assert.equal(name("C3 E3"), null);
  assert.equal(name("C3 F3"), null);
  // A fifth over its upper note is a fourth.
  assert.equal(name("G2 C3"), null);
  assert.equal(name("C3 Db3 D3"), null);
  assert.equal(name("C3 E3 G3 Db4"), null);
  assert.equal(chordName([], "mixed"), null);
});

test("roots and basses are spelt for the key", () => {
  assert.equal(name("Bb2 D3 F3", "flats"), "B♭");
  assert.equal(name("Bb2 D3 F3", "sharps"), "A♯");
  assert.equal(name("Eb3 G3 Bb3 Db4", "mixed"), "E♭7");
  assert.equal(name("F#2 A#2 C#3", "flats"), "G♭");
  assert.equal(name("D#3 F#3 B3", "sharps"), "B/D♯");
});

test("what the Chord mode shows", () => {
  assert.deepEqual(describeHeld([], "mixed"), { main: "", kind: "notes", notes: "" });
  assert.deepEqual(describeHeld([60], "mixed"), { main: "C3", kind: "note", notes: "" });
  assert.deepEqual(describeHeld([72, 60], "mixed"), { main: "C3", kind: "note", notes: "C3 C4" });
  assert.deepEqual(describeHeld([67, 64, 60], "mixed"), { main: "C", kind: "chord", notes: "C3 E3 G3" });
  assert.deepEqual(describeHeld([61, 60, 62], "flats"), { main: "C D♭ D", kind: "notes", notes: "C3 D♭3 D3" });
  assert.deepEqual(describeHeld([64, 60], "mixed"), { main: "C E", kind: "notes", notes: "C3 E3" });
});

test("note names follow Logic's octaves", () => {
  assert.equal(noteName(60, "mixed"), "C3");
  assert.equal(noteName(0, "mixed"), "C-2");
  assert.equal(noteName(127, "sharps"), "G8");
  assert.equal(noteName(70, "sharps"), "A♯3");
  assert.equal(noteName(70, "flats"), "B♭3");
});

test("the key decides sharps or flats", () => {
  assert.equal(spellingFor("C major"), "mixed");
  assert.equal(spellingFor("A minor"), "mixed");
  assert.equal(spellingFor("G major"), "sharps");
  assert.equal(spellingFor("B major"), "sharps");
  assert.equal(spellingFor("E minor"), "sharps");
  assert.equal(spellingFor("F major"), "flats");
  assert.equal(spellingFor("D minor"), "flats");
  assert.equal(spellingFor("G minor"), "flats");
  assert.equal(spellingFor("E♭ major"), "flats");
  assert.equal(spellingFor("A♭ minor"), "flats");
  assert.equal(spellingFor("C♯ minor"), "sharps");
  assert.equal(spellingFor("F♯ major"), "sharps");
  assert.equal(spellingFor(""), "mixed");
});
