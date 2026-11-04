// Unit tests for src/pluginMenuModel.ts: the VST3 part of the plug-in slot menus.
import { test } from "node:test";
import assert from "node:assert/strict";
import { vst3Items } from "../src/pluginMenuModel.ts";
import type { PluginDesc } from "../src/api.ts";
import type { MenuItem } from "../src/components/Menu.tsx";

const vst = (name: string, sub: string, vendor = "LSP") => ({ id: `vst:${name}`, name, vendor, subcategories: `Fx|${sub}`, is_instrument: false }) as unknown as PluginDesc;
const shape = (items: MenuItem[]) => items.map((i) => (i.separator ? "-" : i.header ? `# ${i.label}` : i.children ? `${i.label} ›` : i.label));

test("the VST3s come under their own heading, so categories the built-ins share don't read twice", () => {
  const items = vst3Items([vst("Gate", "Dynamics"), vst("Echo", "Delay"), vst("Comp", "Dynamics", "Zam")], false, () => {});
  assert.deepEqual(shape(items), ["# VST3", "Delay ›", "Dynamics ›", "By Manufacturer ›"]);
  assert.deepEqual(shape(items[2].children!), ["Comp", "Gate"]);
  assert.deepEqual(shape(items[3].children!), ["LSP ›", "Zam ›"]);
});

test("no VST3s: a note and no heading", () => {
  assert.deepEqual(shape(vst3Items([], false, () => {})), ["No VST3 effects found"]);
  assert.deepEqual(shape(vst3Items([], true, () => {})), ["No VST3 instruments found"]);
});
