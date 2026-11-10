// Unit tests for clamp (src/lib/clamp.ts), including which bound wins when they cross.
import { test } from "node:test";
import assert from "node:assert/strict";
import { clamp } from "../../src/lib/clamp.ts";

test("clamp holds a value within its bounds", () => {
  assert.equal(clamp(5, 0, 10), 5);
  assert.equal(clamp(-3, 0, 10), 0);
  assert.equal(clamp(12, 0, 10), 10);
});

test("when the bounds cross, the upper one wins", () => {
  assert.equal(clamp(5, 10, 0), 0);
});
