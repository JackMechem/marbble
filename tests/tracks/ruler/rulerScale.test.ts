// Unit tests for what the ruler marks between bar lines at a zoom (src/tracks/ruler/rulerScale.ts).
import { test } from "node:test";
import assert from "node:assert/strict";
import { rulerScale, rulerStep } from "../../../src/tracks/ruler/rulerScale.ts";

test("zoomed out, bar numbers only", () => {
  assert.deepEqual(rulerScale(22, 22 / 24_000), { label: null, mark: null, samples: false });
});

test("zoomed in, beats get labels and divisions marks", () => {
  assert.equal(rulerStep(63, 64), null);
  assert.deepEqual(rulerScale(64, 64 / 24_000), { label: 960, mark: 240, samples: false });
  // Divisions labelled, eighths of a beat marked.
  assert.deepEqual(rulerScale(256, 256 / 24_000), { label: 240, mark: 60, samples: false });
});

test("zoomed in to samples, each one is marked", () => {
  // 8 px a sample at 120 bpm, 48 kHz.
  const s = rulerScale(8 * 24_000, 8);
  assert.equal(s.label, 1, "single ticks labelled");
  assert.equal(s.mark, null, "nothing finer than a tick to mark");
  assert.equal(s.samples, true);
});
