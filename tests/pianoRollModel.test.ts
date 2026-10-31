// Unit tests for src/pianoRollModel.ts: the Piano Roll's lanes, what a point hits and its grid sums.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ccDotAt, cycleGrab, gridFloor, gridSpanAt, inMusicalTime, keyAtPoint, laneKind, laneShort, midiRegions, noteAt, pitchAt, regionForTime, snapToGrid, stalkAt, timeAt, xOf, type PrView } from "../src/pianoRollModel.ts";
import type { PCc, PNote } from "../src/midiEdit.ts";
import type { Region, Track } from "../src/api.ts";
import type { Warp } from "../src/tempo.ts";

// 1 px per sample and 10 px per row, unscrolled: x is the time and row 0 is pitch 127.
const v: PrView = { pps: 1, noteH: 10, left: 0, top: 0, W: 800, H: 400 };
const note = (start: number, length: number, pitch: number, sel = false): PNote => ({ region: 1, start, length, pitch, velocity: 100, muted: false, sel });
const rowY = (pitch: number) => (127 - pitch) * v.noteH + 5;

test("lane ids", () => {
  assert.deepEqual(laneKind("cc:11"), { type: "controller", cc: 11 });
  assert.deepEqual(laneKind("cc:300"), { type: "controller", cc: 127 });
  assert.deepEqual(laneKind("pb"), { type: "pitch_bend" });
  assert.deepEqual(laneKind("at"), { type: "aftertouch" });
  assert.equal(laneKind("velocity"), null);
  assert.equal(laneShort("cc:1"), "Modulation");
  assert.equal(laneShort("cc:74"), "CC 74");
});

test("the mapping goes both ways", () => {
  const scrolled = { ...v, pps: 0.5, left: 100, top: 40 };
  assert.equal(timeAt(scrolled, xOf(scrolled, 900)), 900);
  assert.equal(pitchAt(v, rowY(60)), 60);
  assert.equal(pitchAt(v, -5000), 127);
  assert.equal(pitchAt(v, 5000), 0);
});

test("noteAt finds the part of a note under a point", () => {
  const n = note(100, 90, 60);
  assert.equal(noteAt(v, [n], 150, rowY(60))?.zone, "body");
  assert.equal(noteAt(v, [n], 188, rowY(60))?.zone, "right");
  assert.equal(noteAt(v, [n], 101, rowY(60))?.zone, "left");
  assert.equal(noteAt(v, [n], 150, rowY(61)), null);
  assert.equal(noteAt(v, [n], 300, rowY(60)), null);
});

test("noteAt: a narrow note has no left edge, and selected notes are hit first", () => {
  assert.equal(noteAt(v, [note(100, 12, 60)], 100, rowY(60))?.zone, "body");
  const under = note(100, 50, 60);
  const selected = note(100, 50, 60, true);
  assert.equal(noteAt(v, [selected, under], 120, rowY(60))?.n, selected);
  // Unselected, the last drawn wins.
  const top = note(100, 50, 60);
  assert.equal(noteAt(v, [under, top], 120, rowY(60))?.n, top);
});

test("stalkAt takes the nearest within reach, favouring the selection", () => {
  const a = note(100, 10, 60);
  const b = note(104, 10, 62, true);
  assert.equal(stalkAt(v, [a, b], 90), null);
  assert.equal(stalkAt(v, [a, b], 101), a);
  // Equally far from both: the selected one.
  assert.equal(stalkAt(v, [a, b], 102), b);
});

test("ccDotAt only looks at the lane's kind", () => {
  const kind = { type: "controller", cc: 1 } as const;
  const c: PCc = { region: 1, start: 100, kind, value: 127, sel: false };
  const other: PCc = { ...c, kind: { type: "controller", cc: 2 } };
  // Value 127 sits at the lane's top margin of 6 px.
  assert.equal(ccDotAt(v, [other, c], kind, 100, 6, 100), c);
  assert.equal(ccDotAt(v, [other], kind, 100, 6, 100), null);
  assert.equal(ccDotAt(v, [c], kind, 100, 50, 100), null);
});

test("keyAtPoint: further right is louder, and right of a black key the white keys take over", () => {
  // Pitch 61 is C♯: its row, on the left, plays it.
  assert.equal(keyAtPoint(v, 10, rowY(61), 64).p, 61);
  // Past 60% of the width: the white key above or below, by which half of the row.
  assert.equal(keyAtPoint(v, 50, (127 - 61) * 10 + 2, 64).p, 62);
  assert.equal(keyAtPoint(v, 50, (127 - 61) * 10 + 8, 64).p, 60);
  assert.equal(keyAtPoint(v, 0, rowY(60), 64).vel, 40);
  assert.equal(keyAtPoint(v, 64, rowY(60), 64).vel, 127);
  assert.equal(keyAtPoint(v, 999, rowY(60), 64).vel, 127);
});

test("cycleGrab: ends, the range, or a new range", () => {
  assert.equal(cycleGrab(true, 102, 100, 200), "start");
  assert.equal(cycleGrab(true, 196, 100, 200), "end");
  assert.equal(cycleGrab(true, 150, 100, 200), "move");
  assert.equal(cycleGrab(true, 50, 100, 200), "new");
  assert.equal(cycleGrab(false, 102, 100, 200), "new");
});

// Musical time runs at half the samples' rate, so a bar of 400 samples is 200 here.
const warp = { toV: (t: number) => t / 2, fromV: (x: number) => x * 2, bars: 200 } as unknown as Warp;
const g = { bar: 400, grid: 100 };

test("snapToGrid", () => {
  assert.equal(snapToGrid(g, null, true, 140), 100);
  assert.equal(snapToGrid(g, null, true, 160), 200);
  assert.equal(snapToGrid(g, null, false, 140.4), 140);
  assert.equal(snapToGrid(g, null, true, 140.4, true), 140);
  // In musical time 140 samples are 70, which snaps to the 100 there: 200 samples.
  assert.equal(snapToGrid(g, warp, true, 140), 200);
});

test("gridFloor is the grid line at or before", () => {
  assert.equal(gridFloor(g, null, true, 199), 100);
  assert.equal(gridFloor(g, null, true, 200), 200);
  assert.equal(gridFloor(g, null, false, 199.6), 200);
  assert.equal(gridFloor(g, warp, true, 399), 200);
});

test("gridSpanAt", () => {
  assert.equal(gridSpanAt(null, 500, 100.4), 100);
  assert.equal(gridSpanAt(warp, 500, 100), 200);
});

test("inMusicalTime edits in musical time and comes back", () => {
  const n = note(400, 200, 60);
  assert.deepEqual(inMusicalTime(null, [n], (x) => x), [n]);
  const seen: PNote[] = [];
  const out = inMusicalTime(warp, [n], (x) => {
    seen.push(...x);
    return x.map((m) => ({ ...m, start: m.start + 50 }));
  });
  assert.deepEqual([seen[0].start, seen[0].length], [200, 100]);
  assert.deepEqual([out[0].start, out[0].length], [500, 200]);
});

const region = (id: number, start: number, length: number) => ({ id, start, length }) as unknown as Region;

test("regionForTime: covering regions first, the edited one before others", () => {
  const regs = [region(1, 0, 100), region(2, 50, 100)];
  assert.equal(regionForTime(regs, 1, 400, 60), 1);
  assert.equal(regionForTime(regs, null, 400, 60), 2);
  assert.equal(regionForTime(regs, null, 400, 10), 1);
});

test("regionForTime: else one within a bar, which grows to hold it, else none", () => {
  const regs = [region(1, 0, 100), region(2, 1000, 100)];
  assert.equal(regionForTime(regs, null, 400, 300), 1);
  assert.equal(regionForTime(regs, null, 400, 900), 2);
  assert.equal(regionForTime(regs, 2, 400, 700), 2);
  assert.equal(regionForTime(regs, null, 100, 500), null);
});

test("midiRegions leaves out audio and pattern regions", () => {
  const r = (id: number, type: string, pattern?: object) => ({ id, content: { type }, pattern }) as unknown as Region;
  const t = { regions: [r(1, "midi"), r(2, "audio"), r(3, "midi", {})] } as unknown as Track;
  assert.deepEqual(midiRegions(t).map((x) => x.id), [1]);
});
