// Unit tests for src/oldName.ts (moving `fermata.` localStorage keys to `marbble.`). Run with
// `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { migrateStorage } from "../src/oldName.ts";

function fakeStorage(init: Record<string, string>): Storage {
  const m = new Map(Object.entries(init));
  return {
    get length() {
      return m.size;
    },
    key: (i: number) => [...m.keys()][i] ?? null,
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, v),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  };
}

test("old keys move to the new name, and a value saved under the new name wins", () => {
  const s = fakeStorage({ "fermata.theme": "dark", "fermata.uiScale": "1.25", "marbble.uiScale": "1", other: "x" });
  migrateStorage(s);
  assert.equal(s.getItem("marbble.theme"), "dark");
  assert.equal(s.getItem("marbble.uiScale"), "1");
  assert.equal(s.getItem("fermata.theme"), null);
  assert.equal(s.getItem("fermata.uiScale"), null);
  assert.equal(s.getItem("other"), "x");
  migrateStorage(s);
  assert.equal(s.length, 3);
});
