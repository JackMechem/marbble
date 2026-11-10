// Unit tests for src/keyboard/keyFormat.ts: shortcuts read as each platform's menu bar writes them.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { formatKeyFor, setGtkKeyNames, withKey } from "../../src/keyboard/keyFormat.ts";

const pc = (k: string) => formatKeyFor(k, "windows");
const gtk = (k: string) => formatKeyFor(k, "gtk");
const mac = (k: string) => formatKeyFor(k, "mac");

test("Windows spells modifiers out, Ctrl Alt Shift then the key", () => {
  assert.equal(pc("Mod+D"), "Ctrl+D");
  assert.equal(pc("Mod+Shift+Z"), "Ctrl+Shift+Z");
  assert.equal(pc("Alt+K"), "Alt+K");
  assert.equal(pc("Mod+Alt+B"), "Ctrl+Alt+B");
  assert.equal(pc("Ctrl+Alt+B"), "Ctrl+Alt+B");
  assert.equal(pc("Shift+Space"), "Shift+Space");
  assert.equal(pc("Delete"), "Delete");
  assert.equal(pc("Backspace"), "Backspace");
  assert.equal(pc("Mod+Backspace"), "Ctrl+Backspace");
  assert.equal(pc("Mod+ArrowLeft"), "Ctrl+←");
  assert.equal(pc("Mod+="), "Ctrl+=");
  assert.equal(pc("Mod+-"), "Ctrl+-");
  assert.equal(pc("Mod++"), "Ctrl++");
  assert.equal(pc("Escape"), "Esc");
  // Mod and Ctrl are the same key outside macOS: shown once.
  assert.equal(pc("Mod+Ctrl+P"), "Ctrl+P");
});

test("macOS uses Logic's glyphs, ⌃⌥⇧⌘ then the key", () => {
  assert.equal(mac("Mod+D"), "⌘D");
  assert.equal(mac("Mod+Shift+Z"), "⇧⌘Z");
  assert.equal(mac("Alt+K"), "⌥K");
  assert.equal(mac("Ctrl+Alt+B"), "⌃⌥B");
  assert.equal(mac("Ctrl+B"), "⌃B");
  assert.equal(mac("Mod+Ctrl+P"), "⌃⌘P");
  assert.equal(mac("Backspace"), "⌫");
  assert.equal(mac("Mod+Backspace"), "⌘⌫");
  assert.equal(mac("Shift+Space"), "⇧Space");
  assert.equal(mac("Mod+ArrowRight"), "⌘→");
  assert.equal(mac("Mod+-"), "⌘−");
  assert.equal(mac("Alt+'"), "⌥'");
});

// The pairs menu/gtk_keys.rs's tests check against GTK's own gtk_accelerator_get_label: the keys the
// menu bar shows beside its items read exactly as the app writes them.
const GTK_LABELS: [string, string][] = JSON.parse(readFileSync(fileURLToPath(new URL("./gtkKeyLabels.json", import.meta.url)), "utf8"));

test("Linux writes keys as GTK's menus draw them: Shift, Ctrl, Alt, then GTK's key name", () => {
  assert.ok(GTK_LABELS.length > 20);
  for (const [spec, label] of GTK_LABELS) assert.equal(gtk(spec), label, spec);
  // Mod and Ctrl are the same key outside macOS.
  assert.equal(gtk("Mod+Ctrl+P"), "Ctrl+P");
  assert.equal(gtk("Alt"), "Alt");
});

test("a bare modifier names the key, for gestures like Alt-drag", () => {
  assert.equal(pc("Alt"), "Alt");
  assert.equal(pc("Mod"), "Ctrl");
  assert.equal(mac("Alt"), "⌥");
  assert.equal(mac("Mod"), "⌘");
  // A plain letter key is not mistaken for a modifier.
  assert.equal(pc("K"), "K");
  assert.equal(mac("Space"), "Space");
});

test("on Linux the app writes keys with the names its GTK draws, translated", () => {
  // As English GTK writes them (menu/gtk_keys.rs gtk_key_names in an English session).
  setGtkKeyNames({ keys: { Backspace: "Backspace", Enter: "Return" }, mods: { Shift: "Shift+", Ctrl: "Ctrl+", Alt: "Alt+" } });
  assert.equal(gtk("Backspace"), "Backspace");
  assert.equal(gtk("Mod+Backspace"), "Ctrl+Backspace");
  assert.equal(gtk("Mod+Shift+Z"), "Shift+Ctrl+Z");
  // A name GTK didn't send keeps the untranslated one.
  assert.equal(gtk("PageUp"), "Page Up");
  setGtkKeyNames({ keys: { Backspace: "Rücktaste" }, mods: { Shift: "Umschalt+", Ctrl: "Strg+", Alt: "Alt+" } });
  assert.equal(gtk("Mod+Alt+Shift+Backspace"), "Umschalt+Strg+Alt+Rücktaste");
  // Other platforms don't use them.
  assert.equal(pc("Mod+Backspace"), "Ctrl+Backspace");
  setGtkKeyNames(null);
  assert.equal(gtk("Mod+Backspace"), "Ctrl+BackSpace");
});

test("a tooltip shows the key after the name, and nothing for a command without one", () => {
  assert.equal(withKey("Record", "R"), "Record (R)");
  assert.equal(withKey("Input Monitoring", ""), "Input Monitoring");
});
