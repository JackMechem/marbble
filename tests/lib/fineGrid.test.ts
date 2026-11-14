// Unit tests for the grid finer than a beat (src/lib/fineGrid.ts): which step a zoom shows and
// the ruler's labels for it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fineLabel, fineStep, TICKS_PER_BEAT } from "../../src/lib/fineGrid.ts";

test("the finest step far enough apart", () => {
  // A division (240 ticks) is a quarter of a beat: 60 px apart at 240 px a beat.
  assert.equal(fineStep(239, 60), null);
  assert.equal(fineStep(240, 60), 240);
  assert.equal(fineStep(480, 60), 120);
  // A single tick 60 px apart.
  assert.equal(fineStep(60 * TICKS_PER_BEAT, 60), 1);
  assert.equal(fineStep(1e9, 60), 1);
  // 5 ticks: 60 px at 11 520 px a beat.
  assert.equal(fineStep(11_520, 60), 5);
});

test("labels are as precise as the step", () => {
  assert.equal(fineLabel(5, 960, 960), "5.2");
  assert.equal(fineLabel(5, 960 + 480, 240), "5.2.3");
  assert.equal(fineLabel(5, 960 + 480 + 120, 60), "5.2.3.121");
  assert.equal(fineLabel(1, 15, 15), "1.1.1.16");
  // The last tick of a beat.
  assert.equal(fineLabel(2, 959, 1), "2.1.4.240");
});
