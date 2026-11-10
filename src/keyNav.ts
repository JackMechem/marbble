// Keyboard use of the controls that aren't sliders: which keys a focused slot, menu or the Tracks
// area's regions take, and where they move. Pure, so it can be tested; Menu.tsx, slotKeys.ts and
// Arrange.tsx apply it. The keys are taken only while the keyboard moves the focus (Tab turned the
// focus ring on, see focusRing.ts): after a click, Space, Return, Delete and the arrows stay the
// app's key commands, as they always were.

import { keepsKeys } from "./textKeys.ts";

interface Keyish {
  key: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

const plain = (e: Keyish) => !e.altKey && !e.ctrlKey && !e.metaKey;

/** What a key does on a focused slot (an insert, send, instrument, input or output slot). */
export type SlotKey = "open" | "menu" | "context" | "remove" | "previous" | "next";

/**
 * Return and Space do what a click on the slot does (open the plug-in, or the slot's menu);
 * Alt+Down opens the slot's pop-up menu, as on a closed pop-up; the context-menu key and
 * Shift+F10 its right-click menu; Delete and Backspace empty it; Up and Down go to the slot above
 * or below in the strip.
 */
export function slotKey(e: Keyish): SlotKey | null {
  if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey && plain(e))) return "context";
  if (e.key === "ArrowDown" && e.altKey && !e.ctrlKey && !e.metaKey) return "menu";
  if (!plain(e) || e.shiftKey) return null;
  switch (e.key) {
    case "Enter":
    case " ":
      return "open";
    case "Delete":
    case "Backspace":
      return "remove";
    case "ArrowUp":
      return "previous";
    case "ArrowDown":
      return "next";
    default:
      return null;
  }
}

/** The index one step from `i` among `count` (slots in a strip): it stops at the ends. */
export function stepIndex(count: number, i: number, dir: -1 | 1): number {
  if (count <= 0) return -1;
  return Math.min(count - 1, Math.max(0, i + dir));
}

/** A menu row as navigation sees it: separators, headers, disabled rows and pickers are passed over. */
export interface NavItem {
  label?: string;
  disabled?: boolean;
  separator?: boolean;
  header?: boolean;
  /** A row that isn't an item (a swatch grid or a picker of its own). */
  custom?: boolean;
}

const choosable = (it: NavItem) => !it.separator && !it.header && !it.disabled && !it.custom;

/**
 * The item Up (-1) or Down (1) moves to from `from` (-1 when none has the focus): the next one that
 * can be chosen, round from the end to the start as a native menu goes. -1 when none can be.
 */
export function menuStep(items: readonly NavItem[], from: number, dir: -1 | 1): number {
  const n = items.length;
  if (!n) return -1;
  let i = from < 0 ? (dir === 1 ? -1 : n) : from;
  for (let k = 0; k < n; k++) {
    i = (i + dir + n) % n;
    if (choosable(items[i])) return i;
  }
  return -1;
}

/** The first (Home) or last (End) item that can be chosen, or -1. */
export function menuEdge(items: readonly NavItem[], edge: "first" | "last"): number {
  return edge === "first" ? menuStep(items, -1, 1) : menuStep(items, -1, -1);
}

/**
 * Type-ahead: the item whose label starts with `typed` (ignoring case), looking from the one after
 * `from` and round. Typing one letter again steps through the items starting with it.
 */
export function typeAhead(items: readonly NavItem[], from: number, typed: string): number {
  const n = items.length;
  const t = typed.toLowerCase();
  if (!n || !t) return -1;
  // The same letter typed again moves on; a longer prefix may still match the current item.
  const again = t.length > 1 && [...t].every((c) => c === t[0]);
  const want = again ? t[0] : t;
  const start = again || t.length === 1 ? from + 1 : Math.max(0, from);
  for (let k = 0; k < n; k++) {
    const i = (((start + k) % n) + n) % n;
    if (choosable(items[i]) && (items[i].label ?? "").toLowerCase().startsWith(want)) return i;
  }
  return -1;
}

/** What a key does in an open menu with an item focused (or none, at -1). */
export type MenuKey = { move: number } | { choose: true } | { open: true } | { back: true } | null;

/**
 * Up and Down move (round the ends), Home and End go to the ends, Return and Space choose, Right
 * opens a submenu, Left closes one; a letter types ahead (`typed` is the letters typed so far,
 * this one included, and a space only once a word has begun). Keys with Ctrl, Cmd or Alt are left
 * to the app.
 */
export function menuKey(items: readonly NavItem[], from: number, e: Keyish, typed = ""): MenuKey {
  if (!plain(e)) return null;
  switch (e.key) {
    case "ArrowDown":
      return { move: menuStep(items, from, 1) };
    case "ArrowUp":
      return { move: menuStep(items, from, -1) };
    case "Home":
    case "PageUp":
      return { move: menuEdge(items, "first") };
    case "End":
    case "PageDown":
      return { move: menuEdge(items, "last") };
    case "Enter":
    case " ":
      // Space types ahead once a word has begun ("No P…").
      if (e.key === " " && typed) return { move: typeAhead(items, from, typed) };
      return from >= 0 ? { choose: true } : null;
    case "ArrowRight":
      return from >= 0 ? { open: true } : null;
    case "ArrowLeft":
      return { back: true };
  }
  if (e.key.length === 1 && typed) return { move: typeAhead(items, from, typed) };
  return null;
}

/**
 * What a key does in the focused Tracks area: Left and Right select the region before or after on
 * the selected track's lane, Return opens the selected region in its editor (a double-click's
 * job). Up and Down are the app's Select Previous and Next Track already, and Space still plays.
 */
export function laneKey(e: Keyish): "previous" | "next" | "open" | null {
  if (!plain(e) || e.shiftKey) return null;
  return e.key === "ArrowLeft" ? "previous" : e.key === "ArrowRight" ? "next" : e.key === "Enter" ? "open" : null;
}

/** A region on a lane, as the arrows step through them. */
interface Placed {
  id: number;
  start: number;
}

/**
 * The region Left (-1) or Right (1) selects on a lane: the one before or after the selected
 * region of the lane that starts latest (with Left, the earliest), in time order. With none of the
 * lane's regions selected, Right takes the first and Left the last. Null at the ends.
 */
export function adjacentRegion(regions: readonly Placed[], selected: readonly number[], dir: -1 | 1): number | null {
  const sorted = [...regions].sort((a, b) => a.start - b.start || a.id - b.id);
  if (!sorted.length) return null;
  const at = sorted.map((r, i) => (selected.includes(r.id) ? i : -1)).filter((i) => i >= 0);
  if (!at.length) return (dir === 1 ? sorted[0] : sorted[sorted.length - 1]).id;
  const i = dir === 1 ? Math.max(...at) + 1 : Math.min(...at) - 1;
  return sorted[i]?.id ?? null;
}

interface Focusable {
  tagName: string;
  getAttribute(name: string): string | null;
}

const ACTIVATED_ROLES = ["button", "menuitem", "menuitemcheckbox", "menuitemradio", "tab", "checkbox", "switch", "option", "link"];

/**
 * Whether the main window's key commands leave a key to the focused element: Return and Space
 * press the button (or slot, tab, link) the keyboard focused, instead of going to the beginning and
 * playing. After a click, `keyboard` is false and they stay key commands.
 */
export function leavesKeyToFocus(e: Keyish, el: Focusable | null, keyboard: boolean): boolean {
  if (!keyboard || !el || !plain(e) || (e.key !== "Enter" && e.key !== " ")) return false;
  const tag = el.tagName.toUpperCase();
  if (tag === "BUTTON" || tag === "A" || tag === "SUMMARY") return true;
  return ACTIVATED_ROLES.includes(el.getAttribute("role") ?? "");
}

/**
 * For a button that acts as the mouse goes down (a pop-up, a local menu, a track switch): the
 * same handler on a click without a pointer behind it (`detail` 0), which is Return or Space on
 * the focused button. Spread it in place of `onMouseDown`.
 */
export function onPress<E extends { detail: number }>(h: (e: E) => void): { onMouseDown: (e: E) => void; onClick: (e: E) => void } {
  return {
    onMouseDown: h,
    onClick: (e) => {
      if (e.detail === 0) h(e);
    },
  };
}

/** The letters typed so far for type-ahead, and when the last one was. */
export interface Typed {
  text: string;
  at: number;
}

/** A pause this long (ms) starts the type-ahead letters afresh. */
export const TYPE_AHEAD_MS = 700;

/**
 * `key` added to the type-ahead letters typed before it (at time `now`): forgotten after a pause,
 * and a space only once a word has begun. Null when the key isn't one (the letters stay as they
 * were).
 */
export function typeAheadText(prev: Typed, key: string, now: number): Typed | null {
  const text = now - prev.at > TYPE_AHEAD_MS ? "" : prev.text;
  if (key.length !== 1 || (key === " " && !text)) return null;
  return { text: text + key, at: now };
}

/** What a key does on a focused row of a list: move to another row (-1: none matched), open or
 *  choose the row, or remove it. */
export type ListKey = { move: number } | { open: true } | { remove: true } | null;

/**
 * A key on a focused row of a list (the Library, the Project Chooser's recent projects, Project
 * Alternatives, the Marker List), `from` its index among `count`: Up and Down move one row,
 * stopping at the ends, Home and End go to the ends, Return and Space open or choose it (what a
 * click or double-click does there), Delete and Backspace remove it. With `labels`, letters type
 * ahead (`typed` as for menuKey, from typeAheadText). Keys with Ctrl, Cmd or Alt are left to the
 * app, and so are the arrows with Shift.
 */
export function listKey(count: number, from: number, e: Keyish, typed = "", labels?: readonly string[]): ListKey {
  if (!plain(e) || count <= 0) return null;
  if (labels && e.key.length === 1 && typed) {
    return { move: typeAhead(labels.map((label) => ({ label })), from, typed) };
  }
  if (e.shiftKey) return null;
  switch (e.key) {
    case "ArrowDown":
      return { move: stepIndex(count, from, 1) };
    case "ArrowUp":
      return { move: stepIndex(count, from, -1) };
    case "Home":
      return { move: 0 };
    case "End":
      return { move: count - 1 };
    case "Enter":
    case " ":
      return { open: true };
    case "Delete":
    case "Backspace":
      return { remove: true };
  }
  return null;
}

/** The attribute on a row whose letters type ahead: "menu" on a menu's rows (Menu.tsx), which
 *  always take them, "list" on a list's (listKeys.ts), which take them only while the keyboard
 *  moves the focus. */
export const TYPE_AHEAD_ATTR = "data-type-ahead";

interface Rowish {
  closest(selector: string): { getAttribute(name: string): string | null } | null;
}

/**
 * Whether a key pressed with the focus on `el` is a letter for the type-ahead of the menu or
 * list `el` is a row of (`keyboard`: the keyboard is moving the focus, focusRing.ts).
 */
export function typesAhead(e: Keyish, el: unknown, keyboard: boolean): boolean {
  if (!plain(e) || e.key.length !== 1) return false;
  const row = el as Partial<Rowish> | null;
  if (typeof row?.closest !== "function") return false;
  const kind = row.closest(`[${TYPE_AHEAD_ATTR}]`)?.getAttribute(TYPE_AHEAD_ATTR);
  return kind === "menu" || (kind === "list" && keyboard);
}

/**
 * Whether Musical Typing, which takes its letters in the capture phase before anything else
 * sees them, leaves a key pressed in `el` alone: with a modifier (Shift too, so Shift+S stays
 * Select Similar Regions), in a field that keeps its keys (textKeys.ts), and a letter for a
 * focused menu's or list's type-ahead, which would otherwise play a note instead of going to
 * the item.
 */
export function typingLeavesKey(e: Keyish, el: unknown, keyboard: boolean): boolean {
  return e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || keepsKeys(el) || typesAhead(e, el, keyboard);
}
