// Unit tests for src/plugins/pluginSearch.ts: plug-in search in the Library and the slot menus.
import { test } from "node:test";
import assert from "node:assert/strict";
import { findPlugins, matchesQuery, searchMenuItems, type Findable } from "../../src/plugins/pluginSearch.ts";
import type { MenuItem } from "../../src/ui/Menu.tsx";

const shape = (items: MenuItem[]) => items.map((i) => (i.header ? `# ${i.label}` : i.hint ? `${i.label} (${i.hint})` : i.label));
const fx = (name: string, hint: string, keywords?: string): Findable => ({ id: `builtin:${name}`, name, hint, keywords });

test("a query matches when every word is somewhere in the text, in any order and case", () => {
  assert.ok(matchesQuery("comp", "Compressor", "Dynamics"));
  assert.ok(matchesQuery("LSP comp", "Compressor Stereo", "LSP", "Fx|Dynamics"));
  assert.ok(matchesQuery("dyn  COMP", "Compressor", "Dynamics"));
  assert.ok(!matchesQuery("comp reverb", "Compressor", "Dynamics"));
  assert.ok(matchesQuery("", "anything"), "an empty query matches all");
  assert.ok(matchesQuery("   ", "anything"));
});

test("names that start with the query come first, then a word that does, then the rest", () => {
  const list = [fx("Multiband Compressor", "Dynamics"), fx("Sidechain Comp", "Dynamics"), fx("Compressor", "Dynamics"), fx("Gate", "Dynamics", "complements a compressor")];
  assert.deepEqual(
    findPlugins(list, "comp").map((p) => p.name),
    ["Compressor", "Multiband Compressor", "Sidechain Comp", "Gate"],
  );
});

test("the menu lists matching built-ins and VST3s under their headings, with their hints", () => {
  const builtins = [fx("Compressor", "Dynamics"), fx("Reverb", "Reverb")];
  const vst3 = [{ id: "vst:a", name: "LSP Compressor", hint: "LSP", keywords: "Fx|Dynamics" }];
  const picked: string[] = [];
  const items = searchMenuItems(builtins, vst3, "comp", (id) => picked.push(id), "vst:a");
  assert.deepEqual(shape(items), ["# Marbble", "Compressor (Dynamics)", "# VST3", "LSP Compressor (LSP)"]);
  assert.equal(items[3].checked, true, "the slot's plug-in is ticked");
  items[1].onClick?.();
  assert.deepEqual(picked, ["builtin:Compressor"]);
  assert.deepEqual(shape(searchMenuItems(builtins, vst3, "verb", () => {})), ["# Marbble", "Reverb (Reverb)"], "no heading for a set with no matches");
});

test("CLAP plug-ins come under the heading given, tagged", () => {
  const hosted = [
    { id: "vst:a", name: "Compressor Stereo", hint: "LSP VST3", keywords: "VST3" },
    { id: "clap:a", name: "Compressor Stereo", hint: "LSP CLAP", tag: "CLAP", keywords: "CLAP" },
  ];
  const items = searchMenuItems([], hosted, "compressor", () => {}, undefined, "VST3 & CLAP");
  assert.deepEqual(shape(items), ["# VST3 & CLAP", "Compressor Stereo (LSP VST3)", "Compressor Stereo (LSP CLAP)"]);
  assert.deepEqual(
    items.map((i) => i.tag),
    [undefined, undefined, "CLAP"],
  );
  assert.deepEqual(shape(searchMenuItems([], hosted, "clap", () => {})).slice(1), ["Compressor Stereo (LSP CLAP)"], "the format is searched");
});

test("nothing matching: a note that can't be chosen", () => {
  const items = searchMenuItems([fx("Gain", "Utility")], [], "zzz", () => {});
  assert.deepEqual(shape(items), ["No matching plug-ins"]);
  assert.equal(items[0].disabled, true);
});
