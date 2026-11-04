// Unit tests for src/slotName.ts (what a plug-in slot calls its plug-in). Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { slotName } from "../src/slotName.ts";

test("built-in instruments show their model", () => {
  assert.equal(slotName("builtin:tre", "Marbble Tre"), "Tre");
  assert.equal(slotName("builtin:sei", "Marbble Sei"), "Sei");
  assert.equal(slotName("builtin:synth", "Marbble Synth"), "Synth");
  assert.equal(slotName("builtin:sampler", "Marbble Sampler"), "Sampler");
  // Saved before the rename.
  assert.equal(slotName("builtin:uno", "Fermata Uno"), "Uno");
});

test("effects and other plug-ins keep their names", () => {
  assert.equal(slotName("builtin:compressor", "Compressor"), "Compressor");
  assert.equal(slotName("builtin:linear-phase-eq", "Linear Phase EQ"), "Linear Phase EQ");
  assert.equal(slotName("vst3:abc", "Marbble Clone"), "Marbble Clone");
  assert.equal(slotName("builtin:x", "Marbble"), "Marbble");
});
