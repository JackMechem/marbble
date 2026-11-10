// Unit tests for src/plugins/panels/settings/settingsMenuModel.ts: the preset menu's Copy and
// Paste Settings.
import { test } from "node:test";
import assert from "node:assert/strict";
import { settingsFit, settingsMenuItems } from "../../../../src/plugins/panels/settings/settingsMenuModel.ts";
import type { PluginSlot } from "../../../../src/engine/project.ts";

const copied = (plugin_id: string): PluginSlot => ({ uid: 3, plugin_id, name: "Copied", bypass: false, params: [[1, 2]] });

test("settings paste only into the same plug-in", () => {
  assert.ok(settingsFit(copied("builtin:channel-eq"), "builtin:channel-eq"));
  assert.ok(!settingsFit(copied("builtin:linear-phase-eq"), "builtin:channel-eq"), "another EQ's");
  assert.ok(!settingsFit(null, "builtin:channel-eq"), "nothing copied yet");
});

test("Copy is always there; Paste waits for settings that fit", () => {
  const calls: string[] = [];
  const items = (clip: PluginSlot | null) =>
    settingsMenuItems(
      "builtin:tre",
      clip,
      () => calls.push("copy"),
      () => calls.push("paste"),
    );
  const [copy, paste] = items(null);
  assert.deepEqual([copy.label, copy.disabled, paste.label, paste.disabled], ["Copy Settings", undefined, "Paste Settings", true]);
  assert.equal(items(copied("builtin:sei"))[1].disabled, true);
  const fits = items(copied("builtin:tre"));
  assert.equal(fits[1].disabled, false);
  fits[0].onClick!();
  fits[1].onClick!();
  assert.deepEqual(calls, ["copy", "paste"]);
});
