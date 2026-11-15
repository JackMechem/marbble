// Unit tests for src/plugins/pluginCustom.ts: the Plug-in Manager's own categories and names.
import { test } from "node:test";
import assert from "node:assert/strict";
import { addCategory, categoryNameProblem, customGroups, deleteCategory, renameCategory, setInCategory, setName, withNames } from "../../src/plugins/pluginCustom.ts";
import type { PluginDesc } from "../../src/engine/listings.ts";

// The id has the bundle's path in it; the class id (cid) is what a rescan or a moved bundle keeps.
const vst = (name: string, cid: string, path = "/usr/lib/vst3/x.vst3") => ({ id: `${path}|${cid}`, cid, name, vendor: "LSP", subcategories: "Fx|Dynamics", is_instrument: false }) as unknown as PluginDesc;

test("renamed plug-ins carry the user's name and keep their own, by class id", () => {
  const list = [vst("Compressor Stereo", "c1"), vst("Gate Stereo", "g1")];
  const named = withNames(list, { c1: "Bus Comp" });
  assert.equal(named[0].name, "Bus Comp");
  assert.equal(named[0].original, "Compressor Stereo");
  assert.equal(named[0].id, list[0].id, "the id loaded is still the plug-in's");
  assert.equal(named[1], list[1], "others are left as they are");
  // The bundle found in another folder by a rescan: same class id, same name.
  assert.equal(withNames([vst("Compressor Stereo", "c1", "/home/me/.vst3/x.vst3")], { c1: "Bus Comp" })[0].name, "Bus Comp");
});

test("the user's categories, in their order, with the plug-ins found; empty ones are left out", () => {
  const list = [vst("A", "a"), vst("B", "b"), vst("C", "c")];
  const cats = [
    { name: "Mix", plugins: ["c", "a", "gone"] },
    { name: "Empty", plugins: ["gone"] },
    { name: "Fav", plugins: ["b"] },
  ];
  assert.deepEqual(
    customGroups(list, cats).map((g) => [g.name, g.plugins.map((p) => p.name)]),
    [
      ["Mix", ["A", "C"]],
      ["Fav", ["B"]],
    ],
  );
  assert.deepEqual(customGroups(list), []);
});

test("category names can't be empty or another's, ignoring case; a rename may keep its own", () => {
  const cats = [{ name: "Favourites", plugins: [] }];
  assert.ok(categoryNameProblem(cats, "  "));
  assert.ok(categoryNameProblem(cats, "favourites"));
  assert.equal(categoryNameProblem(cats, "Mix"), null);
  assert.equal(categoryNameProblem(cats, "FAVOURITES", "Favourites"), null);
});

test("categories are made, renamed, filled, emptied and deleted", () => {
  let cats = addCategory([], " Mix ");
  assert.deepEqual(cats, [{ name: "Mix", plugins: [] }]);
  cats = setInCategory(cats, "Mix", "a", true);
  cats = setInCategory(cats, "Mix", "a", true);
  cats = setInCategory(cats, "Mix", "b", true);
  assert.deepEqual(cats[0].plugins, ["a", "b"], "putting one in twice keeps it once");
  cats = setInCategory(cats, "Mix", "a", false);
  assert.deepEqual(cats[0].plugins, ["b"]);
  cats = renameCategory(addCategory(cats, "Fav"), "Mix", "Mixing");
  assert.deepEqual(
    cats.map((c) => c.name),
    ["Mixing", "Fav"],
  );
  assert.deepEqual(cats[0].plugins, ["b"], "a renamed category keeps its plug-ins");
  assert.deepEqual(
    deleteCategory(cats, "Mixing").map((c) => c.name),
    ["Fav"],
  );
});

test("a custom name is set, and taken away by an empty name or the plug-in's own", () => {
  const names = setName({}, "c1", " Bus Comp ", "Compressor Stereo");
  assert.deepEqual(names, { c1: "Bus Comp" });
  assert.deepEqual(setName(names, "c1", "", "Compressor Stereo"), {});
  assert.deepEqual(setName(names, "c1", "Compressor Stereo", "Compressor Stereo"), {});
  assert.deepEqual(names, { c1: "Bus Comp" }, "the names given aren't changed");
});
