// Unit tests for src/menuBarModel.ts: the native menu bar's state from the in-app menus and the
// key commands, and sending only what changed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { changedStates, keyItemFor, keyStates, mergeStates, statesOf, type ItemState } from "../src/menuBarModel.ts";

test("states come from the in-app items with an id, submenus included", () => {
  const m = statesOf([
    { id: "undo", label: "Undo Move Region" },
    { id: "cut", label: "Cut", disabled: true },
    { separator: true } as never,
    { label: "Repeat", children: [{ id: "repeat:2", label: "2 Times", disabled: true }] },
    { id: "loop", label: "Loop Regions", checked: false },
    { label: "Taller Tracks" },
  ]);
  assert.deepEqual([...m.keys()], ["undo", "cut", "repeat:2", "loop"]);
  assert.deepEqual(m.get("undo"), { id: "undo", enabled: true, text: "Undo Move Region" });
  assert.deepEqual(m.get("cut"), { id: "cut", enabled: false, text: "Cut" });
  assert.deepEqual(m.get("loop"), { id: "loop", enabled: true, checked: false, text: "Loop Regions" });
});

test("each key command's first key shows on the item with its id, Delete Regions on Edit › Delete", () => {
  const keys: Record<string, string[]> = { record: ["R", "Shift+R"], "delete-regions": ["Backspace", "Delete"], "track-on-off": [] };
  const m = keyStates(Object.keys(keys), (id) => keys[id], (k) => `<${k}>`);
  assert.deepEqual(m.get("record"), { id: "record", key: "R", keyText: "<R>" });
  assert.deepEqual(m.get("delete"), { id: "delete", key: "Backspace", keyText: "<Backspace>" });
  // No key: an empty one, which takes a key away that was shown before.
  assert.deepEqual(m.get("track-on-off"), { id: "track-on-off", key: "", keyText: "" });
  assert.equal(keyItemFor("cut"), "cut");
});

test("later states add to and override earlier ones", () => {
  const a = new Map<string, ItemState>([["cut", { id: "cut", key: "Mod+X", enabled: false }]]);
  const b = new Map<string, ItemState>([["cut", { id: "cut", enabled: true }]]);
  assert.deepEqual(mergeStates(a, b).get("cut"), { id: "cut", key: "Mod+X", enabled: true });
});

test("only what changed is sent, starting from the menu as menu.rs builds it", () => {
  const sent = new Map<string, ItemState>();
  const now = (...s: ItemState[]) => new Map(s.map((x) => [x.id, x]));
  // As built: enabled, unticked, usual label, no key. Nothing to send for that.
  assert.deepEqual(changedStates(now({ id: "cut", enabled: true, checked: false, text: "Cut", key: "", keyText: "" }), sent), []);
  // A key, a greyed-out item, a tick and a new label each go once.
  assert.deepEqual(changedStates(now({ id: "cut", enabled: false, key: "Mod+X", keyText: "Ctrl+X" }, { id: "cycle", checked: true }, { id: "undo", text: "Undo Split" }), sent), [
    { id: "cut", enabled: false, key: "Mod+X", keyText: "Ctrl+X" },
    { id: "cycle", checked: true },
    { id: "undo", text: "Undo Split" },
  ]);
  assert.deepEqual(changedStates(now({ id: "cut", enabled: false, key: "Mod+X", keyText: "Ctrl+X" }, { id: "cycle", checked: true }), sent), []);
  // Back to the usual label, and a key reassigned.
  assert.deepEqual(changedStates(now({ id: "undo", text: "Undo" }, { id: "cut", key: "Shift+X", keyText: "Shift+X" }), sent), [
    { id: "undo", text: "Undo" },
    { id: "cut", key: "Shift+X", keyText: "Shift+X" },
  ]);
  // A field left out is left as it was.
  assert.deepEqual(changedStates(now({ id: "cut", enabled: true }), sent), [{ id: "cut", enabled: true }]);
  assert.deepEqual(sent.get("cut"), { id: "cut", enabled: true, checked: false, text: "Cut", key: "Shift+X", keyText: "Shift+X" });
});
