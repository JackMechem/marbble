// Unit tests for src/plugins/panels/settings/settingsMenuModel.ts: the preset menu's Copy and
// Paste Settings, and a VST3 slot menu's Compare and Copy and Paste Settings.
import { test } from "node:test";
import assert from "node:assert/strict";
import { settingsFit, settingsMenuItems, vstSettingsItems } from "../../../../src/plugins/panels/settings/settingsMenuModel.ts";
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

test("a VST3's settings fit the same class wherever its bundle is", () => {
  const here = "/usr/lib/vst3/lsp-plugins.vst3|ABCDEF01";
  assert.ok(settingsFit(copied("/Library/Audio/Plug-Ins/VST3/lsp-plugins.vst3|abcdef01"), here));
  assert.ok(!settingsFit(copied("/usr/lib/vst3/lsp-plugins.vst3|ABCDEF02"), here), "another plug-in in the same bundle");
  assert.ok(!settingsFit(copied("builtin:tre"), here));
});

test("a VST3 slot's Compare is greyed out until an edit, and ticked while the saved settings play", () => {
  const calls: string[] = [];
  const act = { compare: () => calls.push("compare"), copy: () => calls.push("copy"), paste: () => calls.push("paste") };
  const items = (states?: { edited: number[]; comparing: number[] }) => vstSettingsItems(7, "/x.vst3|AB", null, states, act);
  const labels = items().map((i) => i.label);
  assert.deepEqual(labels, ["Compare", "Copy Settings", "Paste Settings"]);
  assert.deepEqual([items()[0].disabled, items()[0].checked], [true, false], "no snapshot yet");
  assert.deepEqual([items({ edited: [7], comparing: [] })[0].disabled, items({ edited: [7], comparing: [] })[0].checked], [false, false]);
  assert.deepEqual([items({ edited: [], comparing: [7] })[0].disabled, items({ edited: [], comparing: [7] })[0].checked], [false, true]);
  assert.equal(items({ edited: [8], comparing: [9] })[0].disabled, true, "other instances'");
  items({ edited: [7], comparing: [] })[0].onClick!();
  assert.deepEqual(calls, ["compare"]);
});
