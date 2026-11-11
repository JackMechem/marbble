// Unit tests for src/diagnostics/report.ts: the Copy Report text.
import { test } from "node:test";
import assert from "node:assert/strict";
import { countersOf } from "../../src/diagnostics/counters.ts";
import { countsAt } from "../../src/diagnostics/facts.ts";
import { buildReport, columns, REPORT_LOG_LINES } from "../../src/diagnostics/report.ts";
import type { LogLine } from "../../src/diagnostics/snapshot.ts";
import { sample } from "./sample.ts";

const ctx = { version: "0.1.0-alpha.14", os: "linux", arch: "x86_64", taken: "11/11/2026, 10:00:00", resetAt: null };
const log = (n: number): LogLine[] => Array.from({ length: n }, (_, i) => ({ seq: i + 1, ms: i * 1000, level: i === 2 ? "warn" : "info", target: "daw_engine::session", message: `line ${i + 1}` }));

test("the report has every section, the tracks and the log", () => {
  const d = sample();
  d.load.late_blocks = 2;
  const text = buildReport(ctx, d, countsAt(countersOf(d), null, {}, 0), log(3));
  assert.match(text, /^Marbble 0\.1\.0-alpha\.14 diagnostics, linux x86_64, taken 11\/11\/2026, 10:00:00\n/);
  for (const title of ["Audio device", "Engine load", "Threads and cores", "Memory", "MIDI", "Tracks", "Log \\(3 lines\\)"]) assert.match(text, new RegExp(`== ${title} ==`));
  assert.match(text, /\nLate buffers +2  <--\n/, "a marked value is flagged without colour");
  assert.match(text, /\nBass +Audio +DSP 2 +30\.0% +35\.0%/);
  assert.match(text, /Share of the latest buffer by thread: Audio 10\.0%, DSP 2 30\.0%/);
  assert.match(text, /\n0:02\.000 WARN  daw_engine::session: line 3\n/);
  assert.doesNotMatch(text, /since the window's last reset|Counts and load figures since/);
});

test("a reset is mentioned, and the log is cut to its newest lines", () => {
  const d = sample();
  const text = buildReport({ ...ctx, resetAt: "10:00:00" }, d, countsAt(countersOf(d), null, {}, 0), log(REPORT_LOG_LINES + 5));
  assert.match(text, /Counts and load figures since 10:00:00\./);
  assert.match(text, new RegExp(`== Log \\(the last ${REPORT_LOG_LINES} of ${REPORT_LOG_LINES + 5} lines\\) ==`));
  assert.doesNotMatch(text, /: line 5\n/);
  assert.match(text, /: line 6\n/);
});

test("before the session answers, the report says so and still has the log", () => {
  const text = buildReport(ctx, null, null, log(1));
  assert.match(text, /The session hadn't answered yet/);
  assert.match(text, /line 1/);
});

test("columns line up, the last left ragged", () => {
  assert.deepEqual(columns([["a", "bb", "c"], ["ccc", "d", "e"]]), ["a    bb  c", "ccc  d   e"]);
});
