// Checks the menu bar (src-tauri/src/menu.rs) against src/menuLabels.ts and the key commands:
// one name per command, the same Edit menu in the menu bar and the Tracks area, and keys that
// belong either to the menu bar or to the key commands, never both.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { EDIT_MENU, layMenu, MENU_LABELS, SHORT_LABELS, type MenuShape } from "../src/menuLabels.ts";

const read = (path: string) => readFileSync(fileURLToPath(new URL(`../${path}`, import.meta.url)), "utf8");
const menuRs = read("src-tauri/src/menu.rs");
const keymap = read("src/keymap.ts");

interface RsItem {
  id: string;
  label: string;
  check: boolean;
  /** The accelerator: a string, or the name of a constant. */
  accel: string | null;
}
type RsNode = RsItem | "-" | { sub: string; items: RsNode[] };

/** menu.rs `spec()`, read as a tree. */
function rsSpec(): RsNode[] {
  const start = menuRs.indexOf("fn spec() -> Vec<Spec> {");
  const end = menuRs.indexOf("\nenum Handle", start);
  assert.ok(start > 0 && end > start, "menu.rs spec()");
  const body = menuRs.slice(start, end);
  const root: RsNode[] = [];
  const stack: RsNode[][] = [root];
  const token = /\b(item|keyed|check|radio|Check|Item)\("([^"]+)", "([^"]+)"(?:, (?:Some\()?(?:"([^"]+)"|([A-Z_]+)))?|\bSep\b|\bSub\(\s*"([^"]+)"|\]/g;
  for (const m of body.matchAll(token)) {
    const cur = stack[stack.length - 1];
    if (m[0] === "]") stack.pop();
    else if (m[0] === "Sep") cur.push("-");
    else if (m[6] !== undefined) {
      const sub = { sub: m[6], items: [] as RsNode[] };
      cur.push(sub);
      stack.push(sub.items);
    } else cur.push({ id: m[2], label: m[3], check: /^c|^C|^radio/.test(m[1]), accel: m[4] ?? m[5] ?? null });
  }
  return root;
}

const items = (nodes: RsNode[]): RsItem[] => nodes.flatMap((n) => (n === "-" ? [] : "sub" in n ? items(n.items) : [n]));
const shapeOf = (nodes: RsNode[]): MenuShape => nodes.map((n) => (n === "-" ? n : "sub" in n ? { sub: n.sub, items: shapeOf(n.items) } : (n.id as MenuShape[number])));

test("menu.rs is read whole", () => {
  const bar = rsSpec();
  assert.deepEqual(
    bar.map((n) => (typeof n === "object" && "sub" in n ? n.sub : n)),
    ["Marbble", "File", "Edit", "Track", "Record", "Navigate", "View", "Window", "Help"],
  );
  assert.ok(items(bar).length > 120);
  assert.ok(items(bar).some((i) => i.id === "musical-typing" && i.check && i.accel === "CmdOrCtrl+K"));
  assert.ok(items(bar).some((i) => i.id === "bounce-in-place" && i.accel === "BOUNCE_IN_PLACE_KEY"));
  assert.ok(items(bar).some((i) => i.id === "theme:light" && i.check));
});

test("the one-of-several choices are radio items: View › Theme, one per theme, by its name", () => {
  const radios = [...menuRs.matchAll(/\bradio\("([^"]+)", "([^"]+)"/g)].map((m) => [m[1], m[2]]);
  const themes = [...read("src/themes.ts").matchAll(/\{ id: "([a-z]+)", name: "([^"]+)"/g)].map((m) => [`theme:${m[1]}`, m[2]]);
  assert.equal(themes.length, 5);
  assert.deepEqual(radios, themes);
});

test("the menu bar uses the menus' names", () => {
  const byId = new Map(items(rsSpec()).map((i) => [i.id, i.label]));
  for (const [id, label] of Object.entries(MENU_LABELS)) assert.equal(byId.get(id), label, id);
});

test("both Edit menus have the same items in the same order", () => {
  const edit = rsSpec().find((n): n is { sub: string; items: RsNode[] } => typeof n === "object" && "sub" in n && n.sub === "Edit");
  assert.ok(edit);
  assert.deepEqual(shapeOf(edit.items), EDIT_MENU);
});

test("right-click menus drop Regions from a name, and nothing else", () => {
  for (const [id, short] of Object.entries(SHORT_LABELS)) {
    const full: string = MENU_LABELS[id as keyof typeof MENU_LABELS];
    assert.equal(short, full.replace(/ (by )?Regions\b/, ""), id);
  }
});

test("layMenu lays out items, separators and submenus, leaving out missing items", () => {
  const out = layMenu(
    ["undo", "-", { sub: "Repeat", items: ["repeat:1", "repeat:2"] }, "redo"],
    (id) => (id === "redo" ? null : id),
    (label, children) => `${label}[${children.join(",")}]`,
    "|",
  );
  assert.deepEqual(out, ["undo", "|", "Repeat[repeat:1,repeat:2]"]);
});

// ---- keys -------------------------------------------------------------------------------------

/** Key command ids (keymap.ts KEY_COMMANDS). */
const commandIds = new Set([...keymap.slice(keymap.indexOf("export const KEY_COMMANDS"), keymap.indexOf("\n];")).matchAll(/\{ id: "([^"]+)"/g)].map((m) => m[1]));

/** A menu.rs accelerator in keymap.ts's form. */
function fromAccel(a: string): string {
  const parts = a.split("+").map((p) => (p === "CmdOrCtrl" ? "Mod" : p.replace(/^Digit/, "").replace(/^Equal$/, "=").replace(/^Minus$/, "-")));
  const key = parts.pop()!;
  return [...["Mod", "Ctrl", "Alt", "Shift"].filter((m) => parts.includes(m)), key].join("+");
}

test("a key belongs to the menu bar or to a key command, never both", () => {
  assert.ok(commandIds.size > 80);
  const accelerated = items(rsSpec()).filter((i) => i.accel);
  assert.ok(accelerated.length > 20);
  // The window handles key commands itself; an accelerator would run the item a second time
  // or keep the key from text fields. Edit › Delete shows the Delete Regions command's key.
  for (const i of accelerated) assert.ok(!commandIds.has(i.id) && i.id !== "delete", `${i.id} has an accelerator and a key command`);
});

test("Settings › Key Commands lists the menu bar's own keys as they are", () => {
  const listed = [...keymap.matchAll(/\{ label: "([^"]+)", menu: "([^"]+)", key: (?:IS_MAC \? "[^"]+" : "([^"]+)"|"([^"]+)") \}/g)].map((m) => ({
    label: m[1],
    key: m[3] ?? m[4],
  }));
  const accelerated = items(rsSpec()).filter((i) => i.accel && i.accel !== "BOUNCE_IN_PLACE_KEY");
  for (const i of accelerated) {
    const l = listed.find((x) => x.label === i.label);
    assert.ok(l, `${i.label} isn't in MENU_SHORTCUTS`);
    assert.equal(fromAccel(l.key), fromAccel(i.accel!), i.label);
  }
  assert.equal(listed.length, accelerated.length + 1, "MENU_SHORTCUTS lists an item menu.rs doesn't have (Bounce in Place aside)");
});
