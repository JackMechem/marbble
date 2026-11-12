// Unit tests for src/diagnostics/logLines.ts: the window's copy of the log and its filter.
import { test } from "node:test";
import assert from "node:assert/strict";
import { addLines, filterLines, isActivity, lastSeq, levelCounts, shortTarget } from "../../src/diagnostics/logLines.ts";
import type { LogLevel, LogLine } from "../../src/diagnostics/snapshot.ts";

const line = (seq: number, level: LogLevel = "info", message = `line ${seq}`, target = "daw_engine::session"): LogLine => ({ seq, ms: seq * 10, level, target, message });

test("new lines go after the ones held, once, and the oldest go past the limit", () => {
  let lines = addLines([], [line(1), line(2)]);
  assert.equal(lastSeq(lines), 2);
  lines = addLines(lines, [line(2), line(3)]);
  assert.deepEqual(lines.map((l) => l.seq), [1, 2, 3], "a line sent twice is kept once");
  const same = addLines(lines, []);
  assert.equal(same, lines, "nothing new: the same array, so React doesn't redraw");
  assert.deepEqual(addLines(lines, [line(4), line(5)], 3).map((l) => l.seq), [3, 4, 5]);
  assert.equal(lastSeq([]), 0);
});

test("the filter keeps the level and worse, and every word of the search", () => {
  const lines = [line(1, "error", "audio failed to start"), line(2, "warn", "device gaps"), line(3, "info", "memory locked"), line(4, "debug", "audio idle")];
  assert.deepEqual(filterLines(lines, "trace", "").map((l) => l.seq), [1, 2, 3, 4]);
  assert.deepEqual(filterLines(lines, "warn", "").map((l) => l.seq), [1, 2]);
  assert.deepEqual(filterLines(lines, "trace", "AUDIO").map((l) => l.seq), [1, 4]);
  assert.deepEqual(filterLines(lines, "trace", "audio start").map((l) => l.seq), [1]);
  assert.deepEqual(filterLines(lines, "trace", "session").map((l) => l.seq), [1, 2, 3, 4], "the module counts too");
  assert.deepEqual(levelCounts(lines), { error: 1, warn: 1, info: 1, debug: 1, trace: 0 });
});

test("module paths lose the crate's name", () => {
  assert.equal(shortTarget("daw_engine::session::engine::device"), "session::engine::device");
  assert.equal(shortTarget("daw::startup"), "startup");
  assert.equal(shortTarget("wgpu"), "wgpu");
});

test("a growing run of one command replaces the line it was", () => {
  const run = (seq: number, n: number): LogLine => ({ ...line(seq, "info", `update_track ×${n}`, "activity::command"), run: 1 });
  let lines = addLines([], [run(1, 1), line(2)]);
  lines = addLines(lines, [run(3, 2)]);
  assert.deepEqual(lines.map((l) => [l.seq, l.message]), [[2, "line 2"], [3, "update_track ×2"]]);
});

test("the Log shows everything, only what was done, or only the log", () => {
  const lines = [line(1), line(2, "warn", "device gap at 0:01", "activity::gap"), line(3, "info", "save", "activity::menu")];
  assert.deepEqual(filterLines(lines, "trace", "", "activity").map((l) => l.seq), [2, 3]);
  assert.deepEqual(filterLines(lines, "trace", "", "log").map((l) => l.seq), [1]);
  assert.deepEqual(filterLines(lines, "warn", "", "all").map((l) => l.seq), [2], "a gap is a warning");
  assert.ok(isActivity(lines[2]) && !isActivity(lines[0]));
  assert.equal(shortTarget("activity::menu"), "menu");
});
