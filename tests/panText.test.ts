// Unit tests for src/panText.ts (the pan knob's value as text). Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { panLabel, panSteps, panText } from "../src/panText.ts";

test("pan positions are knob steps, -64..64", () => {
  assert.equal(panSteps(0), 0);
  assert.equal(panSteps(1), 64);
  assert.equal(panSteps(-1), -64);
  assert.equal(panSteps(23 / 64), 23);
  assert.equal(panSteps(-40 / 64), -40);
  // Rounded to the nearest step, never -0.
  assert.equal(panSteps(0.004), 0);
  assert.ok(Object.is(panSteps(-0.004), 0));
  assert.equal(panSteps(10.4 / 64), 10);
  assert.equal(panSteps(10.6 / 64), 11);
});

test("out of range and not-a-number pan reads as the nearest end or the centre", () => {
  assert.equal(panSteps(1.5), 64);
  assert.equal(panSteps(-3), -64);
  assert.equal(panSteps(NaN), 0);
  assert.equal(panSteps(Infinity), 0);
});

test("the label under the knob: C at the centre, L and R either side", () => {
  assert.equal(panLabel(0), "C");
  assert.equal(panLabel(-0.001), "C");
  assert.equal(panLabel(-23 / 64), "L 23");
  assert.equal(panLabel(40 / 64), "R 40");
  assert.equal(panLabel(-1), "L 64");
  assert.equal(panLabel(1), "R 64");
  assert.equal(panLabel(1 / 64), "R 1");
});

test("the spoken value is the label, with the centre spelt out", () => {
  assert.equal(panText(0), "Centre");
  assert.equal(panText(-23 / 64), "L 23");
  assert.equal(panText(40 / 64), "R 40");
  for (const v of [-1, -0.5, -1 / 64, 1 / 64, 0.3, 1]) assert.equal(panText(v), panLabel(v));
});
