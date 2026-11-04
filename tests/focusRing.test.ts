import { test } from "node:test";
import assert from "node:assert/strict";
import { installFocusRing, KEYBOARD_FOCUS_ATTR, releaseFieldFocus } from "../src/focusRing.ts";

function fakeRoot() {
  const attrs = new Set<string>();
  return { attrs, setAttribute: (n: string) => attrs.add(n), removeAttribute: (n: string) => attrs.delete(n) };
}

const keydown = (key: string) => Object.assign(new Event("keydown"), { key });

test("Tab turns the ring on, a mouse press turns it off, other keys leave it", () => {
  const win = new EventTarget();
  const root = fakeRoot();
  const stop = installFocusRing(win, root);
  win.dispatchEvent(keydown(" "));
  assert.equal(root.attrs.has(KEYBOARD_FOCUS_ATTR), false, "a shortcut after a click doesn't ring");
  win.dispatchEvent(keydown("Tab"));
  assert.equal(root.attrs.has(KEYBOARD_FOCUS_ATTR), true);
  win.dispatchEvent(keydown("ArrowUp"));
  assert.equal(root.attrs.has(KEYBOARD_FOCUS_ATTR), true, "arrows on a focused slider keep it");
  win.dispatchEvent(new Event("mousedown"));
  assert.equal(root.attrs.has(KEYBOARD_FOCUS_ATTR), false);
  stop();
  win.dispatchEvent(keydown("Tab"));
  assert.equal(root.attrs.has(KEYBOARD_FOCUS_ATTR), false, "stopped");
});

function fakeField(tagName: string, inside: unknown[] = []) {
  const f = { tagName, blurred: false, contains: (x: unknown) => x === f || inside.includes(x), blur: () => (f.blurred = true) };
  return f;
}

test("a press in the Tracks area takes the keyboard from a text field (the Library's search)", () => {
  const search = fakeField("INPUT");
  assert.equal(releaseFieldFocus({}, search), true);
  assert.equal(search.blurred, true);
  for (const tag of ["TEXTAREA", "SELECT"]) assert.equal(releaseFieldFocus({}, fakeField(tag)), true);
});

test("a press inside the field, or with no field focused, leaves focus alone", () => {
  const rename = fakeField("INPUT");
  assert.equal(releaseFieldFocus(rename, rename), false);
  assert.equal(rename.blurred, false);
  const button = fakeField("BUTTON");
  assert.equal(releaseFieldFocus({}, button), false);
  assert.equal(button.blurred, false);
  assert.equal(releaseFieldFocus({}, null), false);
});
