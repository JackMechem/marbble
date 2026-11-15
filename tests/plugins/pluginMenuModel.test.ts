// Unit tests for src/plugins/pluginMenuModel.ts: the VST3 part of the plug-in slot menus.
import { test } from "node:test";
import assert from "node:assert/strict";
import { vst3Items } from "../../src/plugins/pluginMenuModel.ts";
import type { PluginDesc } from "../../src/engine/listings.ts";
import type { MenuItem } from "../../src/ui/Menu.tsx";

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

test("the user's categories come first under the VST3 heading, then a line, then the plug-ins' own", () => {
  const comp = vst("Comp", "Dynamics", "Zam");
  const echo = vst("Echo", "Delay");
  const items = vst3Items([comp, echo], false, () => {}, undefined, [{ name: "Favourites", plugins: [echo, comp] }]);
  assert.deepEqual(shape(items), ["# VST3", "Favourites ›", "-", "Delay ›", "Dynamics ›", "By Manufacturer ›"]);
  assert.deepEqual(shape(items[1].children!), ["Comp", "Echo"], "by name, as in the other categories");
});
