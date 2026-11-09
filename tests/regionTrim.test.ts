// Unit tests for src/regionTrim.ts: trimming one selected region trims them all.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { Region } from "../src/api.ts";
import { trimAll, trimDelta, trimLimits, trimmed } from "../src/regionTrim.ts";

const MIN = 32;
const audio = (start: number, length: number, offset: number, takes?: { end: number }[]) =>
  ({ id: 1, start, length, content: { type: "audio", clip: 1, offset }, takes: takes ? { takes, comp: [] } : undefined }) as unknown as Region;
const midi = (start: number, length: number) => ({ id: 2, start, length, content: { type: "midi", notes: [], cc: [] } }) as unknown as Region;

test("an audio region reaches back to its file's start and on to its end", () => {
  // Starts 1000 frames into a 10000-frame file.
  assert.deepEqual(trimLimits(audio(5000, 2000, 1000), 10000), { minStart: 4000, maxLength: 9000 });
  // The file isn't loaded yet: no end to stop at.
  assert.equal(trimLimits(audio(5000, 2000, 1000), undefined).maxLength, Infinity);
  // A take folder reaches as far as its takes do.
  assert.equal(trimLimits(audio(5000, 2000, 1000, [{ end: 6000 }, { end: 8000 }]), 3000).maxLength, 7000);
  assert.deepEqual(trimLimits(midi(5000, 2000), undefined), { minStart: 0, maxLength: Infinity });
});

test("a trim moves one edge and keeps the other", () => {
  const lim = { minStart: 0, maxLength: Infinity };
  assert.deepEqual(trimmed({ start: 1000, length: 500 }, lim, "right", 200, MIN), { start: 1000, length: 700 });
  assert.deepEqual(trimmed({ start: 1000, length: 500 }, lim, "left", 200, MIN), { start: 1200, length: 300 });
  assert.deepEqual(trimmed({ start: 1000, length: 500 }, lim, "left", -200, MIN), { start: 800, length: 700 });
});

test("each region stops at its own limits", () => {
  const span = { start: 1000, length: 500 };
  // The file starts 100 frames before the region and ends 300 after it.
  const lim = { minStart: 900, maxLength: 800 };
  assert.deepEqual(trimmed(span, lim, "left", -400, MIN), { start: 900, length: 600 });
  assert.deepEqual(trimmed(span, lim, "right", 900, MIN), { start: 1000, length: 800 });
  // Never shorter than the minimum, from either side, and never before the project's start.
  assert.deepEqual(trimmed(span, lim, "right", -900, MIN), { start: 1000, length: MIN });
  assert.deepEqual(trimmed(span, lim, "left", 900, MIN), { start: 1500 - MIN, length: MIN });
  assert.deepEqual(trimmed({ start: 100, length: 500 }, { minStart: -1000, maxLength: Infinity }, "left", -400, MIN), { start: 0, length: 600 });
});

test("the dragged region's edge snaps, and the move is what the others get", () => {
  const snap = (v: number) => Math.round(v / 1000) * 1000;
  assert.equal(trimDelta({ start: 1000, length: 1500 }, "right", 1400, snap), 1500, "2500 + 1400 snaps to 4000");
  assert.equal(trimDelta({ start: 1000, length: 1500 }, "left", 300, snap), 0);
  assert.equal(trimDelta({ start: 1000, length: 1500 }, "left", 700, snap), 1000);
});

test("trimming several regions moves each by the same amount, within its own limits", () => {
  const free = { minStart: 0, maxLength: Infinity };
  const regions = [
    { id: 1, span: { start: 1000, length: 1000 }, lim: free },
    // Audio with only 100 frames of file before it.
    { id: 2, span: { start: 5000, length: 1000 }, lim: { minStart: 4900, maxLength: 3000 } },
    // Already at the project's start.
    { id: 3, span: { start: 0, length: 1000 }, lim: free },
  ];
  const out = trimAll(regions, "left", -300, MIN);
  assert.deepEqual(out.get(1), { start: 700, length: 1300 });
  assert.deepEqual(out.get(2), { start: 4900, length: 1100 });
  assert.equal(out.has(3), false, "a region the trim can't change is left out");
  const ends = trimAll(regions, "right", 2500, MIN);
  assert.deepEqual(ends.get(1), { start: 1000, length: 3500 });
  assert.deepEqual(ends.get(2), { start: 5000, length: 3000 });
  assert.deepEqual(ends.get(3), { start: 0, length: 3500 });
});
