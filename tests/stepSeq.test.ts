// Unit tests for src/stepSeq.ts: step edits, rows, paint values for the edit modes, names and
// the playing step of each row.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as ss from "../src/stepSeq.ts";
import { TempoMap } from "../src/tempo.ts";
import type { Pattern, Region } from "../src/api.ts";

const SR = 48000;
const Q = 24000; // a quarter at 120 BPM
const S16 = Q / 4;
const map = TempoMap.constant(SR, 120, 4, 4);

const kit = (): Pattern => ({ length: 16, rate: 16, swing: 0, rows: [{ pitch: 36, steps: [] }, { pitch: 42, steps: [] }] });

const region = (pattern: Pattern, start = 0, length = 16 * S16): Region => ({
  id: 1,
  name: "Pattern",
  start,
  length,
  muted: false,
  content: { type: "midi", notes: [] },
  pattern,
});

test("setStep pads the row and stores only what differs", () => {
  const p = ss.setStep(kit(), 0, 3, { on: true });
  assert.deepEqual(p.rows[0].steps, [{}, {}, {}, { on: true }]);
  const q = ss.setStep(p, 0, 3, { velocity: 300, gate: 0 });
  assert.deepEqual(q.rows[0].steps[3], { on: true, velocity: 127, gate: 1 });
  assert.deepEqual(ss.stepAt(q.rows[0], 9), { on: false, velocity: 100, gate: 100, tie: false });
  // The original is untouched; other rows keep their identity.
  assert.equal(kit().rows[0].steps.length, 0);
  assert.equal(q.rows[1], p.rows[1]);
  // Out of range: no change.
  assert.equal(ss.setStep(p, 5, 0, { on: true }), p);
  assert.equal(ss.setStep(p, 0, ss.MAX_STEPS, { on: true }), p);
});

test("rows: add, remove, clear, lengths and columns", () => {
  let p = ss.addRow(kit());
  assert.deepEqual(
    p.rows.map((r) => r.pitch),
    [36, 42, 38],
  );
  p = ss.addRow(p, 0, 50);
  assert.deepEqual(
    p.rows.map((r) => r.pitch),
    [36, 50, 42, 38],
  );
  p = ss.removeRow(p, 1);
  assert.deepEqual(
    p.rows.map((r) => r.pitch),
    [36, 42, 38],
  );
  p = ss.setStep(ss.setStep(p, 0, 1, { on: true, velocity: 80, tie: true }), 1, 2, { on: true });
  const cleared = ss.clearSteps(p, 0);
  assert.deepEqual(cleared.rows[0].steps[1], { velocity: 80 });
  assert.deepEqual(cleared.rows[1].steps[2], { on: true });
  assert.ok(ss.clearSteps(p).rows.every((r) => r.steps.every((s) => !s.on)));
  // Row lengths and rates fall back to the pattern's; the grid is as wide as the longest row.
  p = ss.setRow(p, 1, { length: 24, rate: 24 });
  assert.equal(ss.rowLength(p, p.rows[0]), 16);
  assert.equal(ss.rowLength(p, p.rows[1]), 24);
  assert.equal(ss.rowRate(p, p.rows[1]), 24);
  assert.equal(ss.columns(p), 24);
});

test("paint values for each edit mode", () => {
  const p = ss.setStep(kit(), 0, 2, { on: true, tie: false });
  assert.deepEqual(ss.paintFor("on", p, 0, 2, 0.5), { on: false });
  assert.deepEqual(ss.paintFor("on", p, 0, 3, 0.5), { on: true });
  assert.deepEqual(ss.paintFor("tie", p, 0, 2, 0.5), { tie: true });
  assert.deepEqual(ss.paintFor("velocity", p, 0, 5, 1), { on: true, velocity: 127 });
  assert.deepEqual(ss.paintFor("velocity", p, 0, 5, -1), { on: true, velocity: 1 });
  assert.deepEqual(ss.paintFor("gate", p, 0, 5, 0.5), { on: true, gate: 51 });
  assert.equal(ss.barFraction("velocity", { on: true, velocity: 127, gate: 1, tie: false }), 1);
  assert.equal(ss.barFraction("gate", { on: true, velocity: 127, gate: 1, tie: false }), 0);
  assert.ok(ss.isBarMode("gate") && !ss.isBarMode("tie"));
});

test("names", () => {
  assert.equal(ss.noteName(36), "C1");
  assert.equal(ss.noteName(60), "C3");
  assert.equal(ss.rowName(36), "Kick");
  assert.equal(ss.rowName(42), "Closed Hi-Hat");
  assert.equal(ss.rowName(90), "F♯5");
  assert.equal(Object.keys(ss.GM_DRUMS).length, 81 - 35 + 1);
  assert.deepEqual(
    ss.RATES.map(ss.rateLabel),
    ["1/4", "1/8", "1/16", "1/32", "1/4T", "1/8T", "1/16T", "1/32T"],
  );
});

test("the playing step follows each row's rate and length, and the region's loop", () => {
  const p = ss.setRow(ss.setRow(kit(), 1, { rate: 8, length: 3 }), 0, {});
  const r = region(p, 4 * Q);
  const end = r.start + 3 * r.length; // looped twice more
  const at = (row: number, pos: number) => ss.playingStep(map, r, p, p.rows[row], pos, end);
  assert.equal(at(0, r.start - 1), null);
  assert.equal(at(0, r.start), 0);
  assert.equal(at(0, r.start + 5 * S16 + 10), 5);
  // Eighths, three to a cycle.
  assert.equal(at(1, r.start + 5 * S16), 2);
  assert.equal(at(1, r.start + 6 * S16), 0);
  // A loop repetition starts the pattern over.
  assert.equal(at(0, r.start + r.length + S16), 1);
  assert.equal(at(0, end), null);
  // Not looped: nothing after the pattern's own length.
  const once = { ...p, looped: false, length: 8 };
  assert.equal(ss.playingStep(map, r, once, once.rows[0], r.start + 9 * S16, end), null);
  // A piece cut off a longer region starts partway in: here five sixteenths, in every pass.
  const cut = { ...p, offset: 5 / 4 };
  assert.equal(ss.playingStep(map, r, cut, cut.rows[0], r.start, end), 5);
  assert.equal(ss.playingStep(map, r, cut, cut.rows[0], r.start + r.length + S16, end), 6);
  assert.equal(ss.playingStep(map, r, { ...once, offset: 7 / 4 }, once.rows[0], r.start + S16, end), null);
});

test("a pattern region made at the playhead goes in the first bar no region covers", () => {
  const BAR = 4 * Q;
  const at = (start: number, length: number, loop_end: number | null = null) => ({ start, length, loop_end });
  // An empty track: the bar holding the playhead.
  assert.deepEqual(ss.freePatternBar(map, [], 5 * BAR + Q), [5 * BAR, BAR]);
  // The playhead inside a region (bars 1 to 8 here): the bar after its end.
  assert.deepEqual(ss.freePatternBar(map, [at(0, 8 * BAR)], 5 * BAR + Q), [8 * BAR, BAR]);
  // A region ending inside a bar: the next whole bar.
  assert.deepEqual(ss.freePatternBar(map, [at(0, 8 * BAR + Q)], 5 * BAR), [9 * BAR, BAR]);
  // Regions one after another: past all of them; a loop counts to its loop end.
  assert.deepEqual(ss.freePatternBar(map, [at(4 * BAR, 2 * BAR), at(6 * BAR, BAR, 10 * BAR)], 4 * BAR), [10 * BAR, BAR]);
  // A region starting later in the playhead's bar is in the way too.
  assert.deepEqual(ss.freePatternBar(map, [at(5 * BAR + 2 * Q, Q)], 5 * BAR), [6 * BAR, BAR]);
  // A free bar before a later region is used.
  assert.deepEqual(ss.freePatternBar(map, [at(9 * BAR, BAR)], 5 * BAR), [5 * BAR, BAR]);
});

// ---- the rows of a new pattern --------------------------------------------------------------

const inst = (plugin_id: string, name: string) => ({ uid: 1, plugin_id, name, bypass: false });
const midi = (...pitches: number[]) => ({ id: 1, name: "R", start: 0, length: 1, muted: false, content: { type: "midi" as const, notes: pitches.map((pitch) => ({ start: 0, length: 1, pitch, velocity: 100 })) } }) as unknown as Region;
const trk = (name: string, instrument: ReturnType<typeof inst> | null, ...regions: Region[]) => ({ name, instrument, regions });

test("a kit is a sampler, or an instrument or track named like one; anything else plays pitches", () => {
  assert.ok(ss.isKitTrack(trk("Drums", inst("builtin:synth", "Marbble Synth"))));
  assert.ok(ss.isKitTrack(trk("Beat", null)));
  assert.ok(ss.isKitTrack(trk("Track 3", inst("vst3:abc", "Drum Machine"))));
  assert.ok(ss.isKitTrack(trk("Bells", inst("builtin:sampler", "Marbble Sampler"))));
  assert.ok(!ss.isKitTrack(trk("Bass", inst("builtin:uno", "Marbble Uno"))));
  assert.ok(!ss.isKitTrack(trk("Keys", null)));
});

test("a new pattern's rows follow the instrument", () => {
  // A bass synth with nothing played yet: an octave of the key's scale from C3, highest first.
  assert.deepEqual(ss.templateRows(trk("Bass", inst("builtin:uno", "Marbble Uno")), "C major"), [72, 71, 69, 67, 65, 64, 62, 60]);
  assert.deepEqual(ss.templateRows(trk("Bass", inst("builtin:uno", "Marbble Uno")), "A minor"), [81, 79, 77, 76, 74, 72, 71, 69]);
  assert.deepEqual(ss.templateRows(trk("Bass", null), "nonsense"), [72, 71, 69, 67, 65, 64, 62, 60]);
  // A kit with nothing played yet: the default kit, from the kick up.
  assert.deepEqual(ss.templateRows(trk("Drums", inst("builtin:sampler", "Marbble Sampler")), "C major"), [...ss.DEFAULT_KIT]);
  // The notes the track already plays, the most played first when there are too many.
  assert.deepEqual(ss.templateRows(trk("Bass", inst("builtin:uno", "Uno"), midi(33, 33, 41, 43, 45)), "C major"), [45, 43, 41, 33]);
  assert.deepEqual(ss.templateRows(trk("Drums", null, midi(42, 36, 38, 42)), "C major"), [36, 38, 42]);
  const many = ss.templateRows(trk("Keys", null, midi(...Array.from({ length: 30 }, (_, k) => 40 + k), 69, 69)), "C major");
  assert.equal(many.length, ss.TEMPLATE_ROWS_MAX);
  assert.ok(many.includes(69), "the most played is kept");
  // Rows of the track's pattern regions count too.
  const pat = { ...midi(), pattern: { length: 16, rate: 16, swing: 0, rows: [{ pitch: 50, steps: [{ on: true }] }] } } as unknown as Region;
  assert.deepEqual(ss.templateRows(trk("Bass", null, pat), "C major"), [50]);
});

test("a new row on a pitched instrument is the next free note down", () => {
  const p: Pattern = { length: 16, rate: 16, swing: 0, rows: [72, 71, 69].map((pitch) => ({ pitch, steps: [] })) };
  assert.equal(ss.newRowPitch(p, false), 68);
  assert.equal(ss.newRowPitch({ ...p, rows: [{ pitch: 0, steps: [] }] }, false), 1);
  assert.equal(ss.newRowPitch({ ...p, rows: [] }, false), 60);
  assert.equal(ss.newRowPitch({ ...p, rows: [] }, true), 36, "a kit starts from the kick");
});

test("rows on a pitched instrument are named by note, not by drum", () => {
  assert.equal(ss.rowName(36, false), "C1");
  assert.equal(ss.rowName(36, true), "Kick");
});
