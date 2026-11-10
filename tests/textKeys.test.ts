import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { deleteBackStart, fieldEdit, fieldMenuAction, isTextField, keepsKeys, MENU_ACCELERATORS, midiCommandWhileTyping, textFieldMenu, typingInField } from "../src/textKeys.ts";

/** A text input with a caret or selection, recording what's done to it. */
function input(value: string, start: number, end = start, type = "text") {
  const log: string[] = [];
  const el = {
    tagName: "INPUT",
    type,
    value,
    selectionStart: start,
    selectionEnd: end,
    select: () => log.push("select"),
    setSelectionRange: (a: number, b: number) => {
      el.selectionStart = a;
      el.selectionEnd = b;
      log.push(`range ${a}-${b}`);
    },
  };
  return { el, log };
}

function doc(active: unknown, focused = true) {
  const log: string[] = [];
  return { log, activeElement: active, hasFocus: () => focused, execCommand: (c: string) => (log.push(c), true) };
}

test("text inputs, textareas and editable content are text fields; checkboxes and sliders aren't", () => {
  for (const type of ["text", "search", "number", ""]) assert.equal(isTextField({ tagName: "INPUT", type }), true, type);
  assert.equal(isTextField({ tagName: "TEXTAREA" }), true);
  assert.equal(isTextField({ tagName: "DIV", isContentEditable: true }), true);
  for (const type of ["checkbox", "range", "color", "button"]) assert.equal(isTextField({ tagName: "INPUT", type }), false, type);
  assert.equal(isTextField({ tagName: "SELECT" }), false);
  assert.equal(isTextField({ tagName: "CANVAS" }), false);
  assert.equal(isTextField(null), false);
});

test("a MIDI-fired key command is held back only while a text field in a focused window is typed in", () => {
  assert.equal(typingInField(doc(input("Bass", 4).el)), true);
  // The window doesn't have the focus: the field isn't being typed in, the controller's command runs.
  assert.equal(typingInField(doc(input("Bass", 4).el, false)), false);
  // A focused slider or checkbox would keep its keys, but takes nothing a controller sends.
  assert.equal(typingInField(doc({ tagName: "INPUT", type: "range" })), false);
  assert.equal(typingInField(doc({ tagName: "BODY" })), false);
});

test("any input, pop-up, textarea or editable content keeps its keys", () => {
  assert.equal(keepsKeys({ tagName: "INPUT", type: "checkbox" }), true);
  assert.equal(keepsKeys({ tagName: "SELECT" }), true);
  assert.equal(keepsKeys({ tagName: "TEXTAREA" }), true);
  assert.equal(keepsKeys({ tagName: "DIV", isContentEditable: true }), true);
  assert.equal(keepsKeys({ tagName: "DIV" }), false);
  assert.equal(keepsKeys({ tagName: "BUTTON" }), false);
  assert.equal(keepsKeys(null), false);
});

test("the accelerators and the clipboard items are a field's edits; the rest aren't", () => {
  assert.equal(fieldEdit("select-all"), "select-all");
  assert.equal(fieldEdit("undo"), "undo");
  assert.equal(fieldEdit("redo"), "redo");
  assert.equal(fieldEdit("copy"), "copy");
  assert.equal(fieldEdit("delete-track"), "delete-back");
  for (const id of ["save", "duplicate", "split", "delete", "new-tracks"]) assert.equal(fieldEdit(id), null, id);
});

test("Select All in a focused name field selects its text, not the regions", () => {
  const { el, log } = input("Bass", 4);
  const d = doc(el);
  assert.equal(textFieldMenu("select-all", d), true);
  assert.deepEqual(log, ["select"]);
  assert.deepEqual(d.log, []);
});

test("Undo, Redo and the clipboard go to the field's own editing", () => {
  const { el } = input("Bass", 2);
  for (const id of ["undo", "redo", "cut", "copy", "paste"]) {
    const d = doc(el);
    assert.equal(textFieldMenu(id, d), true, id);
    assert.deepEqual(d.log, [id]);
  }
});

test("editable content selects all through the document", () => {
  const d = doc({ tagName: "DIV", isContentEditable: true });
  assert.equal(textFieldMenu("select-all", d), true);
  assert.deepEqual(d.log, ["selectAll"]);
});

test("the menu items act as usual without a focused text field, or with the window in the background", () => {
  assert.equal(textFieldMenu("select-all", doc({ tagName: "BODY" })), false);
  assert.equal(textFieldMenu("select-all", doc({ tagName: "INPUT", type: "checkbox" })), false);
  assert.equal(textFieldMenu("select-all", doc(null)), false);
  const { el, log } = input("Bass", 4);
  assert.equal(textFieldMenu("select-all", doc(el, false)), false, "a field left focused behind another window");
  assert.deepEqual(log, []);
  assert.equal(textFieldMenu("save", doc(el)), false);
});

test("Delete Track's key deletes the word before the caret in a field, not the track", () => {
  const { el, log } = input("Lead Vox 2", 8);
  const d = doc(el);
  assert.equal(textFieldMenu("delete-track", d, false), true);
  assert.deepEqual(log, ["range 5-8"]);
  assert.deepEqual(d.log, ["delete"]);
  // On macOS ⌘⌫ deletes to the start of the line.
  const mac = input("Lead Vox 2", 8);
  textFieldMenu("delete-track", doc(mac.el), true);
  assert.deepEqual(mac.log, ["range 0-8"]);
  // A selection is deleted as it is.
  const sel = input("Lead Vox 2", 2, 6);
  const ds = doc(sel.el);
  textFieldMenu("delete-track", ds, false);
  assert.deepEqual(sel.log, []);
  assert.deepEqual(ds.log, ["delete"]);
});

test("while a field has the focus, other accelerators are dropped; Save and click-only items run", () => {
  for (const id of ["duplicate", "split", "join", "group", "folder-stack", "new-tracks", "bounce", "musical-typing", "ui-larger", "save-as"]) {
    assert.equal(fieldMenuAction(id), "ignore", id);
    const { el, log } = input("Bass", 2);
    const d = doc(el);
    assert.equal(textFieldMenu(id, d), true, `${id} is taken (and dropped)`);
    assert.deepEqual(log, [], id);
    assert.deepEqual(d.log, [], id);
  }
  for (const id of ["save", "quit", "delete", "join-per-track", "metronome", "repeat:2"]) {
    assert.equal(fieldMenuAction(id), "run", id);
    assert.equal(textFieldMenu(id, doc(input("Bass", 2).el)), false, id);
  }
  for (const id of ["select-all", "undo", "redo", "cut", "copy", "paste", "delete-track"]) assert.equal(fieldMenuAction(id), "edit", id);
  // Without a focused field, or with the window behind, every accelerator runs as before.
  assert.equal(textFieldMenu("duplicate", doc({ tagName: "BODY" })), false);
  assert.equal(textFieldMenu("duplicate", doc({ tagName: "INPUT", type: "checkbox" })), false);
  assert.equal(textFieldMenu("duplicate", doc(input("Bass", 2).el, false)), false);
});

test("the accelerator list matches the menu bar's", () => {
  const rs = readFileSync(new URL("../src-tauri/src/menu.rs", import.meta.url), "utf8");
  // menu.rs's own tests may build items of their own.
  const end = rs.indexOf("#[cfg(test)]");
  const spec = end < 0 ? rs : rs.slice(0, end);
  const ids = [...spec.matchAll(/keyed\("([^"]+)"/g), ...spec.matchAll(/(?:Check|Radio)\("([^"]+)", "[^"]*", Some\(/g)].map((m) => m[1]);
  assert.ok(ids.length > 10, "found menu.rs's accelerators");
  assert.deepEqual([...MENU_ACCELERATORS].sort(), [...new Set(ids)].sort());
});

test("the word before the caret: past spaces and punctuation, then the word", () => {
  assert.equal(deleteBackStart("Lead Vox", 8), 5);
  assert.equal(deleteBackStart("Lead Vox ", 9), 5);
  assert.equal(deleteBackStart("Lead Vox", 6), 5);
  assert.equal(deleteBackStart("Lead Vox", 5), 0);
  assert.equal(deleteBackStart("Kick (In)", 9), 6);
  assert.equal(deleteBackStart("Gitarre Über", 12), 8);
  assert.equal(deleteBackStart("", 0), 0);
  assert.equal(deleteBackStart("one\ntwo three", 13, true), 4);
  assert.equal(deleteBackStart("one two", 7, true), 0);
});

// Guards: the windows' menu and key handlers go through the module.
const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), "utf8");

test("the main window's menu handler lets a focused field take its edits first", () => {
  const body = src("App.tsx").split("function handleMenu(id: string) {")[1];
  assert.ok(body, "handleMenu is in App.tsx");
  const first = body.split("\n").find((l) => l.trim() && !l.trim().startsWith("//"));
  assert.equal(first?.trim(), "if (textFieldMenu(id)) return;");
});

test("a panel window's menu items reach a focused field there before the panel or the main window", () => {
  assert.match(src("detach.ts"), /if \(!textFieldMenu\(e\.payload\) && !opts\.menu\(e\.payload\)\) emitTo\("main"/);
});

test("the windows' key command listeners leave keys in fields alone", () => {
  assert.match(src("App.tsx"), /function onKey\(e: KeyboardEvent\) \{\r?\n {2}const tgt = e\.target as HTMLElement;\r?\n {2}if \(keepsKeys\(tgt\)\) return;/);
  assert.match(src("detach.ts"), /if \(keepsKeys\(t\)\) return;/);
});

test("a MIDI-fired command waits while a field in a panel window is typed in, as its key would", () => {
  // A key command's key would go to the field there.
  assert.equal(midiCommandWhileTyping("play", false), "drop");
  assert.equal(midiCommandWhileTyping("next-marker", false), "drop");
  // A menu item's accelerator goes to that window, which edits the field (Undo, Select All) or drops it (Split).
  assert.equal(midiCommandWhileTyping("undo", true), "panel");
  assert.equal(midiCommandWhileTyping("select-all", true), "panel");
  assert.equal(midiCommandWhileTyping("split", true), "panel");
  // Save, and items without a key, run as usual.
  assert.equal(midiCommandWhileTyping("save", true), "run");
  assert.equal(midiCommandWhileTyping("marker-list", true), "run");
});
