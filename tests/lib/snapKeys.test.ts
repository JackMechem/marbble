// Unit tests for src/lib/snapKeys.ts: Ctrl snaps a drag finer, Ctrl+Shift not at all.
import { test } from "node:test";
import assert from "node:assert/strict";
import { snapModeWith } from "../../src/lib/snapKeys.ts";

const keys = (ctrlKey: boolean, shiftKey: boolean) => ({ ctrlKey, shiftKey });

test("no keys: the Snap menu's mode", () => {
  assert.equal(snapModeWith("smart"), "smart");
  assert.equal(snapModeWith("bar", keys(false, false)), "bar");
  // Shift alone is left to the drags that use it.
  assert.equal(snapModeWith("beat", keys(false, true)), "beat");
});

test("Ctrl snaps to ticks, Ctrl+Shift not at all", () => {
  for (const mode of ["smart", "bar", "beat", "division", "ticks"] as const) {
    assert.equal(snapModeWith(mode, keys(true, false)), "ticks");
    assert.equal(snapModeWith(mode, keys(true, true)), "off");
  }
});

test("with Snap off, Ctrl doesn't snap", () => {
  assert.equal(snapModeWith("off", keys(true, false)), "off");
  assert.equal(snapModeWith("off", keys(true, true)), "off");
});
