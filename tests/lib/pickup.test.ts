// The pickup and the first bar's number: src/lib/tempo.ts (mirroring the engine's tests in
// crates/engine/src/model/tempo/tests.rs) and the Project Settings conversions in pickup.ts.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as t from "../../src/lib/tempo.ts";
import { pickupLength, pickupQuarters, pickupTickStep } from "../../src/lib/pickup.ts";
import { bbtToSample, formatPosition, parsePosition } from "../../src/editors/eventList/model.ts";

const SR = 48000;
// 24000 samples a quarter at 120 BPM.
const Q = 24000;
const close = (a: number, b: number, tol = 1e-6) => assert.ok(Math.abs(a - b) <= tol, `${a} != ${b}`);
const ev = (pos: number, bpm: number): t.TempoEvent => ({ pos, bpm, ramp: false });
const numbered = (tempo: t.TempoEvent[], sigs: t.SigEvent[], base: [number, number], pickup: number, firstBar: number) =>
  new t.TempoMap(SR, tempo, sigs, 120, base, { pickup, firstBar });

test("no pickup and bar 1 change nothing", () => {
  const tempo = [ev(0, 120), { pos: 96000, bpm: 60, ramp: true }, ev(300000, 150)];
  const sigs = [
    { bar: 3, num: 7, den: 8 },
    { bar: 5, num: 3, den: 4 },
  ];
  const a = new t.TempoMap(SR, tempo, sigs, 120, [4, 4]);
  const b = numbered(tempo, sigs, [4, 4], 0, 1);
  for (let s = 0; s < 900000; s += 12345) {
    assert.deepEqual(a.bbtAt(s), b.bbtAt(s));
    assert.equal(a.barStart(a.barAt(s)), b.barStart(b.barAt(s)));
  }
  assert.deepEqual([...a.bars(0, 900000)], [...b.bars(0, 900000)]);
  assert.ok([...a.bars(0, 900000)].every((s) => s.len === s.end - s.start));
  assert.deepEqual([...a.grid(0, 900000, 2)], [...b.grid(0, 900000, 2)]);
  // A project from before bar numbering.
  assert.deepEqual(t.barNumbering({}), { pickup: 0, firstBar: 1 });
});

test("a pickup is the end of bar 0", () => {
  const m = numbered([], [], [4, 4], 1, 1);
  assert.deepEqual(m.bbtAt(0), { bar: 0, beat: 4, div: 1, tick: 1 });
  assert.deepEqual(m.bbtAt(Q), { bar: 1, beat: 1, div: 1, tick: 1 });
  close(m.barStart(1), Q);
  close(m.barStart(0), -3 * Q);
  close(m.barNumberAt(0), 0.75);
  assert.equal(m.barStartOnTimeline(0), 0);
  assert.deepEqual(m.barSpanAt(Q / 2), [0, Q]);
  assert.deepEqual(m.barSpanAt(Q), [Q, 4 * Q]);
  const bars = [...m.bars(0, 9.5 * Q)].map((b) => [b.bar, b.start, b.end]);
  assert.deepEqual(bars, [
    [0, 0, Q],
    [1, Q, 5 * Q],
    [2, 5 * Q, 9 * Q],
    [3, 9 * Q, 13 * Q],
  ]);
  // Labels and lines space the pickup as a whole bar; rulers label it when it's wide enough.
  const pickupBar = [...m.bars(0, Q)][0];
  assert.equal(pickupBar.len, 4 * Q);
  assert.equal(t.labelEvery((pickupBar.len * 22) / Q), 1);
  assert.ok(t.labelsBar(m, pickupBar, 2, 22 / Q));
  assert.ok(!t.labelsBar(m, pickupBar, 1, 5 / Q));
  // Ruler labels count from the first full bar.
  assert.ok(m.everyNthBar(1, 2) && m.everyNthBar(3, 2) && !m.everyNthBar(0, 2) && !m.everyNthBar(2, 2));
  // The metronome's grid: the pickup's beat, then bar 1's downbeat.
  const g = [...m.grid(0, 6 * Q, 1)];
  assert.deepEqual([g[0].pos, g[0].bar, g[0].sub], [0, 0, 3]);
  assert.deepEqual([g[1].pos, g[1].bar, g[1].sub], [Q, 1, 0]);
  // Snapping never lands before the timeline's start.
  const long = numbered([], [], [4, 4], 3, 1);
  close(long.snapQ(0.4, 4), 0);
  close(long.floorQ(0.4, 4), 0);
  close(m.snapQ(2, 4), 1);
});

test("a part-beat pickup keeps the beat grid", () => {
  const m = numbered([], [], [4, 4], 1.5, 1);
  assert.deepEqual(m.bbtAt(0), { bar: 0, beat: 3, div: 3, tick: 1 });
  assert.deepEqual(
    [...m.grid(0, m.barStart(2), 1)].map((p) => p.q),
    [0.5, 1.5, 2.5, 3.5, 4.5],
  );
});

test("negative first bars with a pickup and changes after it", () => {
  const pre = numbered([], [{ bar: 0, num: 6, den: 8 }], [4, 4], 2, -2);
  const barM1 = pre.barStart(-1);
  close(barM1, 6 * Q);
  const m = numbered([ev(0, 120), ev(barM1, 60)], [{ bar: 0, num: 6, den: 8 }], [4, 4], 2, -2);
  assert.deepEqual(m.sigs[0], { bar: -2, num: 4, den: 4 });
  assert.deepEqual(m.bbtAt(0), { bar: -3, beat: 3, div: 1, tick: 1 });
  assert.deepEqual(m.bbtAt(2 * Q), { bar: -2, beat: 1, div: 1, tick: 1 });
  close(m.barStart(0), barM1 + 8 * Q);
  assert.deepEqual(m.sigAtBar(0), [6, 8]);
  assert.deepEqual(
    [...m.bars(0, m.barStart(2))].map((b) => b.bar),
    [-3, -2, -1, 0, 1],
  );
});

test("the Event List reads and writes positions in the pickup and before bar 1", () => {
  const m = numbered([], [], [4, 4], 1, -1);
  assert.equal(formatPosition(m, 0), "-2 4 1 1");
  assert.equal(formatPosition(m, Q), "-1 1 1 1");
  assert.equal(parsePosition("-2 4", m), 0);
  assert.equal(parsePosition("1", m), Q + 2 * 4 * Q);
  // Nothing lands before the timeline's start.
  assert.equal(bbtToSample(m, { bar: -9, beat: 1, div: 1, tick: 1 }), 0);
  assert.equal(parsePosition("-2 1", m), 0);
});

test("Project Settings shows the pickup in beats and ticks of the first signature", () => {
  assert.deepEqual(pickupLength(1, 4), { beats: 1, ticks: 0 });
  assert.deepEqual(pickupLength(1.5, 4), { beats: 1, ticks: 480 });
  assert.deepEqual(pickupLength(1.5, 8), { beats: 3, ticks: 0 });
  assert.equal(pickupQuarters(1, 480, 4, 4), 1.5);
  assert.equal(pickupQuarters(3, 0, 6, 8), 1.5);
  assert.equal(pickupQuarters(0, 0, 4, 4), 0);
  // Whole 32nds, shorter than a bar, never negative.
  assert.equal(pickupQuarters(1, 100, 4, 4), 1.125);
  assert.equal(pickupQuarters(9, 0, 4, 4), 3.875);
  assert.equal(pickupQuarters(-1, 0, 4, 4), 0);
  assert.equal(pickupTickStep(4), 120);
  assert.equal(pickupTickStep(8), 240);
});
