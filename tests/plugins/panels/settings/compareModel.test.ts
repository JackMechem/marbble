// Unit tests for src/plugins/panels/settings/compareModel.ts: what Compare remembers, when it
// lights, and what a press does.
import { test } from "node:test";
import assert from "node:assert/strict";
import { compareStep, compareView, needsCapture, settingsKey, type CompareRecord } from "../../../../src/plugins/panels/settings/compareModel.ts";
import type { PluginSlot } from "../../../../src/engine/project.ts";

const PARAMS = [
  { id: 0, default: 0.5 },
  { id: 1, default: 100 },
];

const slot = (params: [number, number][], state: string | null = null): PluginSlot => ({ uid: 7, plugin_id: "builtin:tre", name: "Tre", bypass: false, params, state });

const key = (s: PluginSlot) => settingsKey(s, PARAMS);

function record(saved: PluginSlot, edited: PluginSlot | null = null): CompareRecord {
  return { pluginId: "builtin:tre", path: "/p/Song.marbble", saved, savedKey: key(saved), edited };
}

test("a parameter at its default is the same whether listed or not", () => {
  assert.equal(key(slot([])), key(slot([[0, 0.5]])));
  assert.notEqual(key(slot([])), key(slot([[1, 200]])));
});

test("a Sampler's zones count, through the snapshot's fingerprint", () => {
  assert.notEqual(key(slot([], "a1b2")), key(slot([], "c3d4")));
});

test("Compare takes the saved settings when it first sees an instance, or another under its id", () => {
  const rec = record(slot([]));
  assert.ok(needsCapture(undefined, "builtin:tre", "/p/Song.marbble"));
  assert.ok(!needsCapture(rec, "builtin:tre", "/p/Song.marbble"));
  assert.ok(needsCapture(rec, "builtin:sei", "/p/Song.marbble"), "another plug-in");
  assert.ok(needsCapture(rec, "builtin:tre", "/q/Other.marbble"), "another project, where ids start again");
});

test("unchanged settings leave Compare dim, and a press does nothing", () => {
  const saved = slot([[1, 200]]);
  assert.deepEqual(compareView(record(saved), key(saved)), { changed: false, comparing: false });
  assert.deepEqual(compareStep(record(saved), key(saved)), { kind: "none" });
  assert.deepEqual(compareView(undefined, key(saved)), { changed: false, comparing: false }, "before it has any");
});

test("an edit lights Compare, and a press plays the saved settings", () => {
  const saved = slot([[1, 200]]);
  const now = slot([[1, 300]]);
  assert.deepEqual(compareView(record(saved), key(now)), { changed: true, comparing: false });
  assert.deepEqual(compareStep(record(saved), key(now)), { kind: "show-saved", saved });
});

test("while the saved settings play, Compare is pressed and goes back to the edited ones", () => {
  const saved = slot([[1, 200]]);
  const edited = slot([[1, 300]]);
  const rec = record(saved, edited);
  assert.deepEqual(compareView(rec, key(saved)), { changed: false, comparing: true });
  assert.deepEqual(compareStep(rec, key(saved)), { kind: "show-edited", edited });
});

test("editing while comparing makes those the edited settings, as in Logic", () => {
  const saved = slot([[1, 200]]);
  const rec = record(saved, slot([[1, 300]]));
  const touched = slot([[1, 250]]);
  assert.deepEqual(compareView(rec, key(touched)), { changed: true, comparing: false });
  assert.deepEqual(compareStep(rec, key(touched)), { kind: "show-saved", saved }, "the next press keeps them, not the old stash");
});
