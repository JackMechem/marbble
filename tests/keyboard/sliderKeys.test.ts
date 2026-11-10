// Unit tests for src/keyboard/sliderKeys.ts: knobs, faders and sliders from the keyboard.
import { test } from "node:test";
import assert from "node:assert/strict";
import { keyTarget, keyValue, sliderAria } from "../../src/keyboard/sliderKeys.ts";

const near = (a: number | null, b: number) => assert.ok(a !== null && Math.abs(a - b) < 1e-9, `${a} ≠ ${b}`);

test("arrows step a hundredth, Shift a tenth of that, Page keys ten times", () => {
  near(keyTarget("ArrowUp", 0.5), 0.51);
  near(keyTarget("ArrowRight", 0.5), 0.51);
  near(keyTarget("ArrowDown", 0.5), 0.49);
  near(keyTarget("ArrowLeft", 0.5), 0.49);
  near(keyTarget("ArrowUp", 0.5, true), 0.501);
  near(keyTarget("PageUp", 0.5), 0.6);
  near(keyTarget("PageDown", 0.5), 0.4);
});

test("Home and End go to the ends, and steps stop there", () => {
  assert.equal(keyTarget("Home", 0.4), 0);
  assert.equal(keyTarget("End", 0.4), 1);
  assert.equal(keyTarget("PageUp", 0.95), 1);
  assert.equal(keyTarget("ArrowDown", 0), 0);
});

test("other keys are left alone", () => {
  assert.equal(keyTarget("a", 0.5), null);
  assert.equal(keyTarget(" ", 0.5), null);
  assert.equal(keyTarget("Enter", 0.5), null);
});

test("a step that rounds back to the same value keeps going until it changes", () => {
  // Whole numbers over 0..100: a fine step (0.001 of the travel = 0.1) rounds away on its own.
  const whole = (p: number) => Math.round(p * 100);
  assert.equal(keyValue("ArrowUp", 0.5, 50, whole, true), 51);
  assert.equal(keyValue("ArrowDown", 0.5, 50, whole, true), 49);
  // A switch with three positions.
  const sw = (p: number) => [0, 1, 2][Math.round(p * 2)];
  assert.equal(keyValue("ArrowUp", 0, 0, sw), 1);
  assert.equal(keyValue("ArrowUp", 0.5, 1, sw), 2);
  assert.equal(keyValue("ArrowDown", 1, 2, sw), 1);
});

test("no value at an end, or for a key the control doesn't take", () => {
  const lin = (p: number) => Math.round(p * 1000) / 1000;
  assert.equal(keyValue("ArrowUp", 1, 1, lin), null);
  assert.equal(keyValue("End", 1, 1, lin), null);
  assert.equal(keyValue("Home", 0, 0, lin), null);
  assert.equal(keyValue("Home", 0.3, 0.3, lin), 0);
  assert.equal(keyValue("x", 0.3, 0.3, lin), null);
});

test("a slider's ARIA props carry its name, range, value and readable text", () => {
  const a = sliderAria({ label: "Volume", text: "-6.0 dB", min: -96, max: 6, now: -6 });
  assert.equal(a.role, "slider");
  assert.equal(a.tabIndex, 0);
  assert.equal(a["aria-label"], "Volume");
  assert.equal(a["aria-valuemin"], -96);
  assert.equal(a["aria-valuemax"], 6);
  assert.equal(a["aria-valuenow"], -6);
  assert.equal(a["aria-valuetext"], "-6.0 dB");
  assert.equal(sliderAria({ label: "x", text: "", min: 0, max: 1, now: NaN })["aria-valuenow"], 0);
});
