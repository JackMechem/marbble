// Unit tests for src/stripRows.ts (the channel strips' shared insert and send rows). Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { filledBeyond, FX_ROWS_MAX, SEND_ROWS_MAX, slotsWanted, stripRows, stripWants } from "../src/stripRows.ts";

const slots = (...filled: number[]) => Array.from({ length: 8 }, (_, i) => (filled.includes(i) ? { uid: i } : null));

test("a strip wants its slots up to the last filled one, plus an empty one", () => {
  assert.equal(slotsWanted(slots(), 8), 1);
  assert.equal(slotsWanted(slots(0), 8), 2);
  assert.equal(slotsWanted(slots(0, 3), 8), 5);
  assert.equal(slotsWanted(slots(0, 1, 2, 3, 4, 5, 6, 7), 8), 8);
  assert.equal(slotsWanted(undefined, 8), 1);
  // Older projects have fewer send slots than there are.
  assert.equal(slotsWanted([{}, {}], 8), 3);
});

test("strips side by side share the rows of the one that wants the most", () => {
  const rows = stripRows([
    { inserts: 2, sends: 1 },
    { inserts: 1, sends: 3 },
    { inserts: 1, sends: 0 },
  ]);
  assert.deepEqual(rows, { inserts: 2, sends: 3 });
});

test("rows are at least one and at most the maximum; past it a strip scrolls", () => {
  assert.deepEqual(stripRows([]), { inserts: 1, sends: 1 });
  assert.deepEqual(stripRows([{ inserts: 0, sends: 0 }]), { inserts: 1, sends: 1 });
  assert.deepEqual(stripRows([{ inserts: 8, sends: 8 }]), { inserts: FX_ROWS_MAX, sends: SEND_ROWS_MAX });
});

test("what each kind of strip wants", () => {
  // Stereo Out has no sends; a folder has neither inserts nor sends.
  assert.deepEqual(stripWants(null, slots(0), undefined, 8), { inserts: 2, sends: 0 });
  assert.deepEqual(stripWants("folder", slots(0), slots(0), 8), { inserts: 0, sends: 0 });
  assert.deepEqual(stripWants("audio", slots(0, 1), slots(2), 8), { inserts: 3, sends: 4 });
  assert.deepEqual(stripWants("aux", slots(), [], 8), { inserts: 1, sends: 1 });
});

test("the +N counts filled slots below the shown rows", () => {
  assert.equal(filledBeyond(slots(0, 1, 6, 7), 6), 2);
  assert.equal(filledBeyond(slots(0, 1), 6), 0);
  assert.equal(filledBeyond(undefined, 1), 0);
});
