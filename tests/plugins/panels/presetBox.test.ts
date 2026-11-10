// Unit tests for src/plugins/panels/presetBox.ts: what a panel's preset box shows.
import { test } from "node:test";
import assert from "node:assert/strict";
import { presetBoxLabel } from "../../../src/plugins/panels/presetBox.ts";

const funk = { name: "Funk Pluck", factory: true };

test("a preset the project has as loaded shows by name", () => {
  // As a project made by `load_preset` (the showcase) opens: the name, before and after its
  // settings are read.
  assert.deepEqual(presetBoxLabel(funk, "[1,2]", null, "[0,0]", "Default"), { label: "Funk Pluck", edited: false });
  assert.deepEqual(presetBoxLabel(funk, "[1,2]", "[1,2]", "[0,0]", "Default"), { label: "Funk Pluck", edited: false });
});

test("changed settings show as edited", () => {
  assert.deepEqual(presetBoxLabel(funk, "[1,3]", "[1,2]", "[0,0]", "Default"), { label: "Funk Pluck", edited: true });
});

test("a preset that is no longer on this computer still shows its name", () => {
  assert.deepEqual(presetBoxLabel({ name: "Mine", factory: false }, "[1,3]", null, null, "Default"), { label: "Mine", edited: false });
});

test("with no preset loaded, settings are the default's or Manual", () => {
  assert.deepEqual(presetBoxLabel(null, "[0,0]", null, "[0,0]", "Flat"), { label: "Flat", edited: false });
  assert.deepEqual(presetBoxLabel(undefined, "[1,0]", null, "[0,0]", "Default"), { label: "Manual", edited: false });
  assert.deepEqual(presetBoxLabel(null, "[0,0]", null, null, "Default"), { label: "Manual", edited: false }, "until the default is read");
});
