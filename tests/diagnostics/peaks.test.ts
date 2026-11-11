// Unit tests for src/diagnostics/peaks.ts: each track's peak held for a moment.
import { test } from "node:test";
import assert from "node:assert/strict";
import { holdPeaks, PEAK_HOLD_MS } from "../../src/diagnostics/peaks.ts";
import { track } from "./sample.ts";

test("a peak rises at once, holds, then falls to the latest", () => {
  let held = holdPeaks(new Map(), [track(1, "A", { peak: 0.5 })], 0);
  held = holdPeaks(held, [track(1, "A", { peak: 0.2 })], 500);
  assert.deepEqual(held.get(1), { peak: 0.5, at: 0 });
  held = holdPeaks(held, [track(1, "A", { peak: 0.7 })], 600);
  assert.deepEqual(held.get(1), { peak: 0.7, at: 600 });
  held = holdPeaks(held, [track(1, "A", { peak: 0.1 })], 600 + PEAK_HOLD_MS);
  assert.equal(held.get(1)?.peak, 0.1);
  assert.equal(holdPeaks(held, [], 0).size, 0, "a track gone is dropped");
});
