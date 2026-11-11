// Unit tests for src/diagnostics/counters.ts: counting from a reset, and counts that rise.
import { test } from "node:test";
import assert from "node:assert/strict";
import { countersOf, noteRises, risingAt, RISE_MS, since } from "../../src/diagnostics/counters.ts";
import { sample } from "./sample.ts";

test("counts run from the last reset, and a counter that started afresh counts whole", () => {
  const d = sample();
  const base = countersOf(d);
  d.load.late_blocks = 5;
  d.audio.device_gaps = 2;
  const now = countersOf(d);
  assert.equal(since(now, null).late_blocks, 5, "no reset: everything");
  base.late_blocks = 3;
  base.device_gaps = 7; // a new device counts from 0 again
  const s = since(now, base);
  assert.equal(s.late_blocks, 2);
  assert.equal(s.device_gaps, 2);
});

test("a counter that grew stays rising for a few seconds", () => {
  const d = sample();
  const prev = countersOf(d);
  d.load.late_blocks = 1;
  const now = countersOf(d);
  assert.deepEqual(noteRises({}, null, now, 0), {}, "nothing to compare with yet");
  const rises = noteRises({}, prev, now, 1000);
  assert.deepEqual([...risingAt(rises, 1000)], ["late_blocks"]);
  assert.deepEqual([...risingAt(rises, 1000 + RISE_MS - 1)], ["late_blocks"]);
  assert.equal(risingAt(rises, 1000 + RISE_MS).size, 0);
  // Unchanged since: the stamp stays.
  assert.equal(noteRises(rises, now, now, 4000).late_blocks, 1000);
});
