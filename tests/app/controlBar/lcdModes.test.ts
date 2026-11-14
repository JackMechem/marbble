// Unit tests for src/app/controlBar/lcdModes.ts: the saved LCD mode and the tuner's readouts.
import { test } from "node:test";
import assert from "node:assert/strict";
import { centsText, hzText, lcdMode, needlePercent } from "../../../src/app/controlBar/lcdModes.ts";

test("the saved mode, or Beats & Project", () => {
  assert.equal(lcdMode("tuner"), "tuner");
  assert.equal(lcdMode("chord"), "chord");
  assert.equal(lcdMode("time"), "time");
  assert.equal(lcdMode(undefined), "beats");
  assert.equal(lcdMode(null), "beats");
  assert.equal(lcdMode("custom"), "beats");
});

test("the needle spans -50 to +50 cents and stops at the ends", () => {
  assert.equal(needlePercent(0), 50);
  assert.equal(needlePercent(-50), 0);
  assert.equal(needlePercent(25), 75);
  assert.equal(needlePercent(80), 100);
});

test("cents and hertz", () => {
  assert.equal(centsText(3.24), "+3.2");
  assert.equal(centsText(-0.4), "−0.4");
  assert.equal(centsText(-0.01), "0.0");
  assert.equal(hzText(440), "440.0");
  assert.equal(hzText(41.2), "41.20");
});
