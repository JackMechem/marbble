// Unit tests for src/tracks/regions/loops.ts: a loop drag on one selected region loops them all.
import { test } from "node:test";
import assert from "node:assert/strict";
import { loopsDragged, regionEnd } from "../../../src/tracks/regions/loops.ts";

const region = (id: number, start: number, length: number, loop_end: number | null = null) => ({ id, start, length, loop_end });

test("every region's loop end moves as far as the grabbed one's", () => {
  // One not looping (ends at 1000), one looping to 5000.
  const regions = [region(1, 0, 1000), region(2, 2000, 500, 5000)];
  assert.deepEqual([...loopsDragged(regions, 1500)], [
    [1, 2500],
    [2, 6500],
  ]);
});

test("dragged back, a loop stops at the region's own end", () => {
  const regions = [region(1, 0, 1000, 1800), region(2, 2000, 500, 5000)];
  const loops = loopsDragged(regions, -1000);
  // The first's loop is gone (its own end); the second's is 1000 shorter.
  assert.equal(loops.get(1), 1000);
  assert.equal(loops.get(2), 4000);
  assert.equal(regionEnd({ ...regions[0], loop_end: loops.get(1)! }), 1000);
});
