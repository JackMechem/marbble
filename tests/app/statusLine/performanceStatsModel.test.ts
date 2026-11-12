// Unit tests for src/app/statusLine/performanceStatsModel.ts: the status line's memory, threads and
// late-block figures and their tooltips.
import { test } from "node:test";
import assert from "node:assert/strict";
import { lateText, lateTitle, memoryText, memoryTitle, threadsText, threadsTitle } from "../../../src/app/statusLine/performanceStatsModel.ts";

const MB = 1024 * 1024;

test("memory is whole megabytes under a gigabyte, then gigabytes to a tenth", () => {
  assert.equal(memoryText(640 * MB), "640 MB");
  assert.equal(memoryText(999.4 * MB), "999 MB");
  assert.equal(memoryText(1000 * MB), "1.0 GB");
  assert.equal(memoryText(2.84 * 1024 * MB), "2.8 GB");
  assert.equal(memoryText(12.3 * 1024 * MB), "12.3 GB");
  // Never "0 MB" for a process that is running.
  assert.equal(memoryText(100 * 1024), "1 MB");
});

test("the memory tooltip names the figure and the click", () => {
  const t = memoryTitle(2.84 * 1024 * MB);
  assert.match(t, /2\.8 GB in RAM/);
  assert.match(t, /Click for the Diagnostics window\./);
});

test("threads are counted, singular for one", () => {
  assert.equal(threadsText(1), "1 thread");
  assert.equal(threadsText(4), "4 threads");
  assert.match(threadsTitle(1), /audio thread alone/);
  assert.match(threadsTitle(4), /4 threads: the audio thread and 3 more\./);
  assert.match(threadsTitle(4), /Settings › Audio/);
});

test("late blocks say how many and that a click clears them", () => {
  assert.equal(lateText(3), "3 late");
  assert.match(lateTitle(1), /^1 block over 100%/);
  assert.match(lateTitle(3), /^3 blocks over 100%/);
  assert.match(lateTitle(3), /Click to clear\.$/);
});
