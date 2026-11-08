// Unit tests for what an instrument panel's section shows (src/instSections.ts).
import { test } from "node:test";
import assert from "node:assert/strict";
import { sectionView, shortName, type SectionParam } from "../src/instSections.ts";

const knob = (id: number, name: string): SectionParam => ({ id, name, kind: { type: "continuous" } });
const toggle = (id: number, name: string): SectionParam => ({ id, name, kind: { type: "toggle" } });
const choice = (id: number, name: string): SectionParam => ({ id, name, kind: { type: "choice", labels: ["A", "B"] } });

test("a name doesn't repeat its section's", () => {
  assert.equal(shortName("Oscillator 1", "Osc 1 Range"), "Range");
  assert.equal(shortName("Filter Contour", "Filter Attack"), "Attack");
  assert.equal(shortName("Operator 3 Envelope", "Op 3 R2"), "R2");
  assert.equal(shortName("Oscillator A", "A Position"), "Position");
  assert.equal(shortName("Voices", "Voice Variation"), "Variation");
  assert.equal(shortName("Wheel Mod", "Mod Amount"), "Amount");
  assert.equal(shortName("Accent", "Accent at Velocity"), "At Velocity");
  // Names that don't start with the section's words stay whole.
  assert.equal(shortName("Mixer", "Osc 1"), "Osc 1");
  assert.equal(shortName("Controllers", "Mod Mix"), "Mod Mix");
  assert.equal(shortName("Poly Mod Sources", "Filter Env"), "Filter Env");
  assert.equal(shortName("HPF", "HPF"), "");
});

test("an On switch folds into the knob it switches", () => {
  const v = sectionView("builtin:tre", "Mixer", [toggle(9, "Osc 1 On"), knob(10, "Osc 1"), toggle(15, "Noise On"), knob(16, "Noise"), choice(17, "Noise Color")]);
  assert.equal(v.power, null);
  assert.deepEqual(v.cells, [
    { type: "switched", id: 10, toggle: 9, label: "Osc 1" },
    { type: "switched", id: 16, toggle: 15, label: "Noise" },
    { type: "control", id: 17, label: "Noise Color" },
  ]);
  assert.equal(v.rows, 1);
});

test("a section's own switch goes in its header", () => {
  const v = sectionView("builtin:sedici", "Operator 1", [toggle(1, "Op 1 On"), knob(2, "Op 1 Level"), choice(3, "Op 1 Mode")]);
  assert.equal(v.power, 1);
  assert.deepEqual(
    v.cells.map((c) => c.label),
    ["Level", "Mode"],
  );
});

test("a switch with no knob of its name drops the On", () => {
  const v = sectionView("builtin:cinque", "Performance", [toggle(1, "Release On"), knob(2, "Glide")]);
  assert.deepEqual(v.cells, [
    { type: "control", id: 1, label: "Release" },
    { type: "control", id: 2, label: "Glide" },
  ]);
});

test("a name that is the section's own gets a word for what it is", () => {
  assert.equal(sectionView("builtin:sei", "VCA", [choice(1, "VCA"), knob(2, "Level")]).cells[0].label, "Mode");
  assert.equal(sectionView("builtin:uno", "Accent", [knob(1, "Accent"), knob(2, "Accent at Velocity")]).cells[0].label, "Amount");
  // Not when another parameter already shows that word: then it keeps its whole name.
  assert.deepEqual(
    sectionView("x", "Chorus", [choice(1, "Chorus"), choice(2, "Chorus Mode")]).cells.map((c) => c.label),
    ["Chorus", "Mode"],
  );
});

test("the labels the rules get wrong are fixed by name", () => {
  const v = sectionView("builtin:tre", "Voices", [choice(1, "Voice Mode"), choice(2, "Voices")]);
  assert.deepEqual(
    v.cells.map((c) => c.label),
    ["Mode", "Count"],
  );
  assert.equal(sectionView("builtin:sampler", "Filter", [choice(1, "Filter")]).cells[0].label, "Type");
});

test("more than four cells take two lines", () => {
  const five = [1, 2, 3, 4, 5].map((i) => knob(i, `K${i}`));
  assert.equal(sectionView("x", "S", five).rows, 2);
  assert.equal(sectionView("x", "S", five.slice(0, 4)).rows, 1);
  // Counted after folding: six parameters, three cells.
  const pairs = [1, 2, 3].flatMap((i) => [toggle(10 + i, `P${i} On`), knob(i, `P${i}`)]);
  assert.equal(sectionView("x", "S", pairs).rows, 1);
});
