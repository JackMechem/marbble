// Unit tests for src/diagnostics/history.ts: the last minute the Diagnostics window's graphs draw.
import { test } from "node:test";
import assert from "node:assert/strict";
import { addSample, HISTORY_MS, threadNames } from "../../src/diagnostics/history.ts";
import { sample, track } from "./sample.ts";

test("samples keep a minute, with the buffers that ran late since the one before", () => {
  const d = sample();
  let h = addSample([], d, 0);
  assert.equal(h[0].late, 0, "the first can't tell");
  d.load.late_blocks += 3;
  h = addSample(h, d, 250);
  assert.equal(h[1].late, 3);
  h = addSample(h, d, 500);
  assert.equal(h[2].late, 0);
  h = addSample(h, d, HISTORY_MS + 300);
  assert.deepEqual(h.map((s) => s.at), [500, HISTORY_MS + 300], "older than a minute goes");
});

test("threads come Audio first, then by number, from every sample", () => {
  const d = sample();
  d.tracks = [track(1, "A", { thread: 10 }), track(2, "B", { thread: 2 })];
  let h = addSample([], d, 0);
  d.tracks = [track(3, "C", { thread: 0 })];
  h = addSample(h, d, 250);
  assert.deepEqual(threadNames(h), ["Audio", "DSP 2", "DSP 10"]);
});
