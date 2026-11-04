// Displaying key specs the way the host platform's menus write them. Pure (no app imports), so
// the tests can run it for every platform.
//
// A key spec is the canonical form of keymap.ts: modifiers then the key, joined with "+".
// "Mod" is ⌘ on macOS and Ctrl elsewhere; "Ctrl" is the Control key on macOS (the same as Mod
// elsewhere). A spec may also be a bare modifier ("Alt") for text like "Alt-drag".
//
// The menu bar draws its own keys, so the app writes keys as the menu bar does, and a key reads
// the same in the menu bar, the in-app menus, tooltips and Settings › Key Commands:
// - macOS: Logic's glyphs in AppKit's order, ⌃⌥⇧⌘ then the key ("⇧⌘Z").
// - Linux: GTK's spelling, Shift, Ctrl, Alt then GTK's key name ("Shift+Ctrl+Z", "Ctrl+Right",
//   "BackSpace"). GTK draws every key in the menu bar this way and can't be told otherwise;
//   tests/gtkKeyLabels.json pins the pairs, checked against GTK itself by menu.rs's tests.
//   GTK translates its names ("Backspace" in English), so at startup the app asks GTK for them
//   (`setGtkKeyNames`, menu.rs `gtk_key_names`) and writes keys exactly as the menu bar does.
// - Windows: Ctrl, Alt, Shift then the key ("Ctrl+Shift+Z"), as its menus write accelerators.

/** A tooltip with a key after the name, "Record (R)"; just the name when there is no key. */
export const withKey = (label: string, key: string): string => (key ? `${label} (${key})` : label);

/** How a platform writes keys. */
export type KeyStyle = "mac" | "gtk" | "windows";

function detectMac(): boolean {
  if (typeof navigator === "undefined") return false;
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = nav.userAgentData?.platform || nav.platform || nav.userAgent || "";
  return /Mac|iPhone|iPad/i.test(platform);
}

/** Whether Marbble runs on macOS, detected once. */
export const IS_MAC = detectMac();

function detectStyle(): KeyStyle {
  if (IS_MAC) return "mac";
  if (typeof navigator === "undefined") return "gtk";
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  return /Win/i.test(nav.userAgentData?.platform || nav.platform || nav.userAgent || "") ? "windows" : "gtk";
}

/** How this platform writes keys, detected once. */
export const KEY_STYLE: KeyStyle = detectStyle();

const MAC_MODS: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Mod: "⌘" };
const PC_MODS: Record<string, string> = { Ctrl: "Ctrl", Alt: "Alt", Shift: "Shift", Mod: "Ctrl" };

const MAC_KEYS: Record<string, string> = {
  Enter: "↩",
  Backspace: "⌫",
  Delete: "⌦",
  Escape: "Esc",
  Tab: "⇥",
  Home: "↖",
  End: "↘",
  PageUp: "⇞",
  PageDown: "⇟",
  "-": "−",
};

const PC_KEYS: Record<string, string> = {
  Escape: "Esc",
  PageUp: "Page Up",
  PageDown: "Page Down",
};

/** GTK's names (gdk_keyval_name, as its accelerator labels show them). */
const GTK_KEYS: Record<string, string> = {
  Enter: "Return",
  Backspace: "BackSpace",
  PageUp: "Page Up",
  PageDown: "Page Down",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
};

/** GTK's names as the running app's GTK writes them (translated), when it has said. */
export interface GtkKeyNames {
  /** A named key's label, by key spec name ("Backspace" → "Backspace" or "BackSpace"). */
  keys: Record<string, string>;
  /** What goes before the key for each modifier ("Shift" → "Shift+"). */
  mods: Record<string, string>;
}

let gtkNames: GtkKeyNames | null = null;

/** Takes GTK's names for keys (menu.rs `gtk_key_names`); null goes back to the untranslated ones. */
export function setGtkKeyNames(names: GtkKeyNames | null) {
  gtkNames = names;
}

const ARROWS: Record<string, string> = { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→" };

/**
 * Formats a key spec for display in a platform's style. The spec should already be normalized
 * (keymap.ts `normalizeKey`), though any modifier order is accepted; outside macOS "Ctrl" and
 * "Mod" are the same key and show once.
 */
export function formatKeyFor(spec: string, style: KeyStyle): string {
  const parts = spec.split("+");
  let key = parts.pop() ?? "";
  // "Mod++": a trailing empty part is the "+" key itself.
  if (key === "" && parts[parts.length - 1] === "") {
    parts.pop();
    key = "+";
  }
  const mods = new Set(parts);
  const mac = style === "mac";
  // A bare modifier names the key on its own ("Alt" for "Alt-drag").
  if (!mods.size && key in MAC_MODS) return mac ? MAC_MODS[key] : PC_MODS[key];
  if (mac) return ["Ctrl", "Alt", "Shift", "Mod"].filter((m) => mods.has(m)).map((m) => MAC_MODS[m]).join("") + (ARROWS[key] ?? MAC_KEYS[key] ?? key);
  const ctrl = mods.has("Ctrl") || mods.has("Mod");
  if (style === "gtk") {
    const on = ["Shift", "Ctrl", "Alt"].filter((m) => (m === "Ctrl" ? ctrl : mods.has(m)));
    return on.map((m) => gtkNames?.mods[m] ?? `${PC_MODS[m]}+`).join("") + (gtkNames?.keys[key] ?? GTK_KEYS[key] ?? key);
  }
  const order = ["Ctrl", "Alt", "Shift"];
  const names = order.filter((m) => (m === "Ctrl" ? ctrl : mods.has(m))).map((m) => PC_MODS[m]);
  const name = ARROWS[key] ?? PC_KEYS[key] ?? key;
  return [...names, name].join("+");
}
