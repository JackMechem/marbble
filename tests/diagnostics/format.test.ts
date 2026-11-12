// Unit tests for src/diagnostics/format.ts: how the Diagnostics window writes its figures.
import { test } from "node:test";
import assert from "node:assert/strict";
import { asked, bytes, DASH, frames, kb, list, ms, num, pct, plural, sinceLaunch, threadName, yesNo } from "../../src/diagnostics/format.ts";

test("unknown values are a dash, not 0", () => {
  for (const f of [pct, ms, num, kb, bytes, threadName]) assert.equal(f(null), DASH);
  assert.equal(frames(null, 48000), DASH);
  assert.equal(yesNo(null), DASH);
  assert.equal(list([]), DASH);
  assert.equal(pct(NaN), DASH);
});

test("shares, times and sizes", () => {
  assert.equal(pct(0.234), "23%");
  assert.equal(pct(1.2346, 1), "123.5%");
  assert.equal(ms(5.33), "5.3 ms");
  assert.equal(kb(512), "512 kB");
  assert.equal(kb(204800), "200.0 MB");
  assert.equal(kb(3 * 1024 * 1024), "3.00 GB");
  assert.equal(bytes(11534336), "11.0 MB");
  assert.equal(frames(512, 48000), "512 (10.7 ms)");
  assert.equal(frames(512, 0), "512");
});

test("threads are named as the window shows them", () => {
  assert.equal(threadName(0), "Audio");
  assert.equal(threadName(3), "DSP 3");
});

test("times since launch", () => {
  assert.equal(sinceLaunch(0), "0:00.000");
  assert.equal(sinceLaunch(62345), "1:02.345");
  assert.equal(sinceLaunch(3723456), "1:02:03.456");
});

test("what was given says what was asked when they differ", () => {
  assert.equal(asked(256, 256), "256");
  assert.equal(asked(512, 256), "512 (asked 256)");
  assert.equal(asked(0, 256), `${DASH} (asked 256)`);
});

test("one of a thing is singular", () => {
  assert.equal(plural(1, "error"), "1 error");
  assert.equal(plural(0, "error"), "0 errors");
  assert.equal(plural(2, "warning"), "2 warnings");
});
