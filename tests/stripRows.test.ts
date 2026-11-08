// Unit tests for src/stripRows.ts (the channel strips' shared insert and send rows). Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { FADER_WANTED, fitStripRows, FX_ROWS_MAX, SEND_ROWS_MAX, SLOT_ROW_H, slotLayout, slotsWanted, stripRows, stripWants } from "../src/stripRows.ts";

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

test("rows are at least one and at most the maximum", () => {
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

test("filled slots past the rows go behind a \"+N more\" in the last row", () => {
  // Room to spare: empty slots fill the rows, up to the slots there are.
  assert.deepEqual(slotLayout(slots(0), 3, 8), { shown: 3, more: [] });
  assert.deepEqual(slotLayout(undefined, 1, 8), { shown: 1, more: [] });
  assert.deepEqual(slotLayout([{}], 3, 1), { shown: 1, more: [] });
  // Exactly full: every plug-in shows, with no empty slot after them.
  assert.deepEqual(slotLayout(slots(0, 1), 2, 8), { shown: 2, more: [] });
  // One row, two plug-ins: the row is "+2 more".
  assert.deepEqual(slotLayout(slots(0, 1), 1, 8), { shown: 0, more: [0, 1] });
  // Past the maximum: five rows as they are, the sixth stands for the rest, gaps left out.
  assert.deepEqual(slotLayout(slots(0, 1, 2, 3, 4, 5, 7), 6, 8), { shown: 5, more: [5, 7] });
  // A plug-in after a gap past the rows.
  assert.deepEqual(slotLayout(slots(0, 4), 2, 8), { shown: 1, more: [4] });
});

test("a short Mixer shows fewer slot rows so the fader keeps its height", () => {
  // A strip 300 px tall without slot rows, its fader at least 60 px.
  const fixed = 300;
  const fader = (r: { inserts: number; sends: number }, view: number) => view - fixed - (r.inserts + r.sends) * SLOT_ROW_H + 60;
  // Room for everything: nothing changes.
  assert.deepEqual(fitStripRows({ inserts: 3, sends: 2 }, 800, fixed, 60), { inserts: 3, sends: 2 });
  // Room for a few rows: the larger section gives way first, sends on a tie.
  const r = fitStripRows({ inserts: 4, sends: 4 }, 430, fixed, 60);
  assert.deepEqual(r, { inserts: 2, sends: 1 });
  assert.ok(fader(r, 430) >= FADER_WANTED);
  assert.ok(fader({ inserts: 2, sends: 2 }, 430) < FADER_WANTED, "one more row would squeeze the fader");
  assert.deepEqual(fitStripRows({ inserts: 1, sends: 4 }, 430, fixed, 60), { inserts: 1, sends: 2 });
  // No room at all: one row each, never none (the strips scroll instead).
  assert.deepEqual(fitStripRows({ inserts: 6, sends: 4 }, 100, fixed, 60), { inserts: 1, sends: 1 });
  assert.deepEqual(fitStripRows({ inserts: 1, sends: 1 }, 100, fixed, 60), { inserts: 1, sends: 1 });
});
