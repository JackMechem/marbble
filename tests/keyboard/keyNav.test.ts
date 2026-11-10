// Unit tests for src/keyboard/keyNav.ts: the keys a focused slot, menu, list or the Tracks area's
// regions take, and where they move.
import { test } from "node:test";
import assert from "node:assert/strict";
import { adjacentRegion, laneKey, leavesKeyToFocus, listKey, menuEdge, menuKey, menuStep, slotKey, stepIndex, typeAhead, typeAheadText, typesAhead, typingLeavesKey, TYPE_AHEAD_ATTR, TYPE_AHEAD_MS, type NavItem } from "../../src/keyboard/keyNav.ts";
import { keyboardFocus, KEYBOARD_FOCUS_ATTR } from "../../src/keyboard/focusRing.ts";

const key = (k: string, mods: Partial<{ altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }> = {}) => ({
  key: k,
  altKey: false,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  ...mods,
});

test("a slot opens with Return and Space, empties with Delete and steps with Up and Down", () => {
  assert.equal(slotKey(key("Enter")), "open");
  assert.equal(slotKey(key(" ")), "open");
  assert.equal(slotKey(key("Delete")), "remove");
  assert.equal(slotKey(key("Backspace")), "remove");
  assert.equal(slotKey(key("ArrowUp")), "previous");
  assert.equal(slotKey(key("ArrowDown")), "next");
  assert.equal(slotKey(key("ArrowDown", { altKey: true })), "menu", "Alt+Down opens the pop-up, as on a closed pop-up");
  assert.equal(slotKey(key("ContextMenu")), "context");
  assert.equal(slotKey(key("F10", { shiftKey: true })), "context");
  // Key commands with modifiers, and other keys, stay the app's.
  assert.equal(slotKey(key("Backspace", { metaKey: true })), null, "Delete Track");
  assert.equal(slotKey(key("Enter", { shiftKey: true })), null);
  assert.equal(slotKey(key("ArrowLeft")), null);
  assert.equal(slotKey(key("m")), null);
  assert.equal(slotKey(key("F10")), null);
});

test("Up and Down stop at a strip's first and last slot", () => {
  assert.equal(stepIndex(5, 2, 1), 3);
  assert.equal(stepIndex(5, 2, -1), 1);
  assert.equal(stepIndex(5, 4, 1), 4);
  assert.equal(stepIndex(5, 0, -1), 0);
  assert.equal(stepIndex(0, 0, 1), -1);
});

// "No Plug-in", a line, a header, then items with one disabled.
const ITEMS: NavItem[] = [
  { label: "No Plug-in" },
  { separator: true },
  { label: "Marbble", header: true },
  { label: "Channel EQ" },
  { label: "Compressor", disabled: true },
  { label: "Chorus" },
  { custom: true },
  { label: "Delay" },
];

test("menu arrows pass over lines, headers, disabled rows and pickers, and go round the ends", () => {
  assert.equal(menuStep(ITEMS, -1, 1), 0, "Down with nothing focused: the first");
  assert.equal(menuStep(ITEMS, -1, -1), 7, "Up with nothing focused: the last");
  assert.equal(menuStep(ITEMS, 0, 1), 3);
  assert.equal(menuStep(ITEMS, 3, 1), 5, "Compressor is disabled");
  assert.equal(menuStep(ITEMS, 5, 1), 7, "the picker row is passed over");
  assert.equal(menuStep(ITEMS, 7, 1), 0, "round to the start");
  assert.equal(menuStep(ITEMS, 0, -1), 7, "round to the end");
  assert.equal(menuEdge(ITEMS, "first"), 0);
  assert.equal(menuEdge(ITEMS, "last"), 7);
  assert.equal(menuStep([{ separator: true }, { label: "x", disabled: true }], -1, 1), -1);
  assert.equal(menuStep([], -1, 1), -1);
});

test("type-ahead finds the next item starting with the letters, and the same letter steps on", () => {
  assert.equal(typeAhead(ITEMS, -1, "c"), 3);
  assert.equal(typeAhead(ITEMS, 3, "c"), 5, "c again from Channel EQ: Chorus (Compressor is disabled)");
  assert.equal(typeAhead(ITEMS, 5, "cc"), 3, "and round");
  assert.equal(typeAhead(ITEMS, 3, "ch"), 3, "a longer prefix the focused item matches keeps it");
  assert.equal(typeAhead(ITEMS, 3, "cho"), 5);
  assert.equal(typeAhead(ITEMS, -1, "D"), 7, "any case");
  assert.equal(typeAhead(ITEMS, -1, "no p"), 0, "a space inside a word");
  assert.equal(typeAhead(ITEMS, -1, "x"), -1);
  assert.equal(typeAhead(ITEMS, -1, "m"), -1, "headers can't be chosen");
});

test("menu keys: move, choose, open and close a submenu, type ahead", () => {
  assert.deepEqual(menuKey(ITEMS, -1, key("ArrowDown")), { move: 0 });
  assert.deepEqual(menuKey(ITEMS, 3, key("ArrowUp")), { move: 0 });
  assert.deepEqual(menuKey(ITEMS, 3, key("Home")), { move: 0 });
  assert.deepEqual(menuKey(ITEMS, 3, key("End")), { move: 7 });
  assert.deepEqual(menuKey(ITEMS, 3, key("Enter")), { choose: true });
  assert.deepEqual(menuKey(ITEMS, 3, key(" ")), { choose: true });
  assert.equal(menuKey(ITEMS, -1, key("Enter")), null, "nothing to choose: Return stays the app's");
  assert.deepEqual(menuKey(ITEMS, 3, key("ArrowRight")), { open: true });
  assert.deepEqual(menuKey(ITEMS, 3, key("ArrowLeft")), { back: true });
  assert.deepEqual(menuKey(ITEMS, -1, key("d"), "d"), { move: 7 });
  assert.deepEqual(menuKey(ITEMS, 0, key(" "), "no "), { move: 0 }, "Space inside a word types ahead");
  assert.equal(menuKey(ITEMS, 3, key("z", { metaKey: true }), "z"), null, "Undo goes on to the app");
  assert.equal(menuKey(ITEMS, 3, key("F5")), null);
});

const LANE = [
  { id: 3, start: 9600 },
  { id: 1, start: 0 },
  { id: 2, start: 4800 },
];

test("Left and Right step through a lane's regions in time order", () => {
  assert.equal(adjacentRegion(LANE, [1], 1), 2);
  assert.equal(adjacentRegion(LANE, [2], 1), 3);
  assert.equal(adjacentRegion(LANE, [3], 1), null, "the last region stays");
  assert.equal(adjacentRegion(LANE, [2], -1), 1);
  assert.equal(adjacentRegion(LANE, [1], -1), null);
  assert.equal(adjacentRegion(LANE, [], 1), 1, "none selected on the lane: Right takes the first");
  assert.equal(adjacentRegion(LANE, [99], -1), 3, "another lane's selection: Left takes the last");
  assert.equal(adjacentRegion(LANE, [1, 2], 1), 3, "Right goes on from the latest selected");
  assert.equal(adjacentRegion(LANE, [2, 3], -1), 1, "Left from the earliest");
  assert.equal(adjacentRegion([], [], 1), null);
  // Regions starting together keep one order.
  assert.equal(adjacentRegion([{ id: 5, start: 0 }, { id: 4, start: 0 }], [4], 1), 5);
});

test("the Tracks area's keys: Left, Right and Return, nothing with modifiers", () => {
  assert.equal(laneKey(key("ArrowLeft")), "previous");
  assert.equal(laneKey(key("ArrowRight")), "next");
  assert.equal(laneKey(key("Enter")), "open");
  assert.equal(laneKey(key("ArrowLeft", { altKey: true })), null, "Nudge Left");
  assert.equal(laneKey(key("ArrowRight", { metaKey: true })), null, "Zoom In");
  assert.equal(laneKey(key(" ")), null, "Space still plays");
  assert.equal(laneKey(key("ArrowUp")), null, "Up and Down select tracks already");
});

const el = (tagName: string, role: string | null = null) => ({ tagName, getAttribute: (n: string) => (n === "role" ? role : null) });

test("Return and Space press what Tab focused, and stay key commands after a click", () => {
  assert.equal(leavesKeyToFocus(key("Enter"), el("BUTTON"), true), true);
  assert.equal(leavesKeyToFocus(key(" "), el("button"), true), true);
  assert.equal(leavesKeyToFocus(key(" "), el("DIV", "button"), true), true, "an empty slot");
  assert.equal(leavesKeyToFocus(key("Enter"), el("DIV", "menuitemcheckbox"), true), true);
  assert.equal(leavesKeyToFocus(key(" "), el("BUTTON"), false), false, "clicked: Space plays");
  assert.equal(leavesKeyToFocus(key(" "), el("DIV", "group"), true), false, "the Tracks area: Space plays");
  assert.equal(leavesKeyToFocus(key(" "), el("DIV"), true), false);
  assert.equal(leavesKeyToFocus(key(" "), null, true), false);
  assert.equal(leavesKeyToFocus(key("Enter", { metaKey: true }), el("BUTTON"), true), false);
  assert.equal(leavesKeyToFocus(key("m"), el("BUTTON"), true), false, "other keys stay key commands");
});

test("keyboardFocus reads the focus ring's attribute", () => {
  const attrs = new Set<string>();
  const root = { hasAttribute: (n: string) => attrs.has(n) };
  assert.equal(keyboardFocus(root), false);
  attrs.add(KEYBOARD_FOCUS_ATTR);
  assert.equal(keyboardFocus(root), true);
});

test("type-ahead letters build up, and start afresh after a pause", () => {
  const a = typeAheadText({ text: "", at: 0 }, "c", 1000);
  assert.deepEqual(a, { text: "c", at: 1000 });
  const b = typeAheadText(a!, "h", 1200);
  assert.deepEqual(b, { text: "ch", at: 1200 });
  assert.deepEqual(typeAheadText(b!, " ", 1300), { text: "ch ", at: 1300 }, "a space once a word has begun");
  assert.equal(typeAheadText({ text: "", at: 0 }, " ", 1000), null, "but not to begin one");
  assert.deepEqual(typeAheadText(b!, "v", 1200 + TYPE_AHEAD_MS + 1), { text: "v", at: 1901 }, "after a pause");
  assert.equal(typeAheadText(b!, "ArrowDown", 1250), null, "not a letter");
  assert.equal(typeAheadText(b!, " ", 1200 + TYPE_AHEAD_MS + 1), null, "a space after a pause begins nothing");
});

test("a list row moves with Up, Down, Home and End, stopping at the ends", () => {
  assert.deepEqual(listKey(4, 1, key("ArrowDown")), { move: 2 });
  assert.deepEqual(listKey(4, 1, key("ArrowUp")), { move: 0 });
  assert.deepEqual(listKey(4, 3, key("ArrowDown")), { move: 3 }, "the last row stays");
  assert.deepEqual(listKey(4, 0, key("ArrowUp")), { move: 0 }, "the first row stays");
  assert.deepEqual(listKey(4, 2, key("Home")), { move: 0 });
  assert.deepEqual(listKey(4, 0, key("End")), { move: 3 });
  assert.equal(listKey(0, -1, key("ArrowDown")), null, "an empty list");
});

test("Return and Space open a list row, Delete and Backspace remove it", () => {
  assert.deepEqual(listKey(3, 1, key("Enter")), { open: true });
  assert.deepEqual(listKey(3, 1, key(" ")), { open: true });
  assert.deepEqual(listKey(3, 1, key("Delete")), { remove: true });
  assert.deepEqual(listKey(3, 1, key("Backspace")), { remove: true });
});

test("keys with modifiers, and Shift with the arrows, stay the app's", () => {
  assert.equal(listKey(3, 1, key("Backspace", { metaKey: true })), null, "Delete Track");
  assert.equal(listKey(3, 1, key("ArrowDown", { altKey: true })), null);
  assert.equal(listKey(3, 1, key("ArrowDown", { shiftKey: true })), null);
  assert.equal(listKey(3, 1, key("Enter", { ctrlKey: true })), null);
  assert.equal(listKey(3, 1, key("ArrowLeft")), null);
  assert.equal(listKey(3, 1, key("m")), null, "no type-ahead without labels");
});

test("letters type ahead through a list's names", () => {
  const names = ["Bass", "Chorus", "Compressor", "Delay"];
  assert.deepEqual(listKey(4, 0, key("c"), "c", names), { move: 1 });
  assert.deepEqual(listKey(4, 1, key("c"), "cc", names), { move: 2 }, "the same letter again steps on");
  assert.deepEqual(listKey(4, 1, key("o"), "co", names), { move: 2 });
  assert.deepEqual(listKey(4, 0, key("D", { shiftKey: true }), "D", names), { move: 3 }, "a capital");
  assert.deepEqual(listKey(4, 0, key("x"), "x", names), { move: -1 }, "no match: taken, but nothing moves");
  assert.deepEqual(listKey(4, 0, key(" "), "", names), { open: true }, "Space opens before a word has begun");
  assert.deepEqual(listKey(2, 0, key(" "), "my ", ["My Song", "My Song 2"]), { move: 0 }, "and types ahead after");
});

// Stand-ins for what has the focus: a row marked for type-ahead ("menu" or "list"), or a plain element.
const focused = (tagName: string, typeAhead?: "menu" | "list") => ({
  tagName,
  closest: (selector: string) => (typeAhead && selector === `[${TYPE_AHEAD_ATTR}]` ? { getAttribute: (name: string) => (name === TYPE_AHEAD_ATTR ? typeAhead : null) } : null),
});

test("Musical Typing leaves letters to a focused list or menu that types ahead, as to a text field", () => {
  const menuRow = focused("DIV", "menu");
  const listRow = focused("DIV", "list");
  // "a" plays C and "z" is octave down while Musical Typing is open; in a menu they type ahead.
  assert.equal(typingLeavesKey(key("a"), menuRow, false), true, "a menu row, opened with the mouse or the keyboard");
  assert.equal(typingLeavesKey(key("z"), menuRow, true), true);
  assert.equal(typingLeavesKey(key("a"), listRow, true), true, "a list row the keyboard moved to");
  // A list takes keys only while the keyboard moves the focus (listKeys.ts); after a click, letters play.
  assert.equal(typingLeavesKey(key("a"), listRow, false), false);
  assert.equal(typingLeavesKey(key("a"), focused("DIV"), true), false, "anything else: the letter plays");
  assert.equal(typingLeavesKey(key("a"), null, false), false);
  // Text fields keep their keys, and a modifier makes it a key command, as before.
  assert.equal(typingLeavesKey(key("a"), focused("INPUT"), false), true);
  assert.equal(typingLeavesKey(key("a"), focused("TEXTAREA"), false), true);
  assert.equal(typingLeavesKey(key("s", { shiftKey: true }), focused("DIV"), false), true);
  // Keys that aren't letters stay Musical Typing's in a menu too (Tab is its sustain pedal).
  assert.equal(typesAhead(key("Tab"), menuRow, true), false);
  assert.equal(typesAhead(key("a", { ctrlKey: true }), menuRow, true), false);
  assert.equal(typesAhead(key("a"), { tagName: "DIV" }, true), false, "an element without closest()");
});
