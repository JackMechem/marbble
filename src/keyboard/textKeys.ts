// Text fields keep their editing keys. Key commands come through each window's keydown listener,
// which leaves a focused field alone (`keepsKeys`). The native menu's accelerators don't: the menu
// bar takes Select All, Undo, Redo and Delete Track (menu.rs) before the web view sees the key, and
// they arrive as menu items. While a text field has the focus, those items edit the field instead
// (`textFieldMenu`), so Ctrl+A in a track's name selects its text rather than every region, and
// the other accelerators (Ctrl+D, Ctrl+T...) do nothing rather than edit the project, except Save.
// Pure apart from its `document` defaults, so the tests run it on stand-ins.
import { IS_MAC } from "./keyFormat.ts";

/** The element a key goes to, as far as these checks need it. */
export interface FieldLike {
  tagName: string;
  type?: string;
  isContentEditable?: boolean;
  readOnly?: boolean;
}

/** An `<input>` or `<textarea>` with a text selection (a number field has none). */
interface TextControl extends FieldLike {
  value: string;
  selectionStart: number | null;
  selectionEnd: number | null;
  select(): void;
  setSelectionRange(start: number, end: number): void;
}

/** What `textFieldMenu` needs of the document. */
export interface DocLike {
  activeElement: unknown;
  hasFocus(): boolean;
  execCommand(command: string): boolean;
  getSelection?(): { modify?(alter: string, direction: string, granularity: string): void } | null;
}

// Inputs whose value is typed text; checkboxes, sliders, colour wells and buttons aren't.
const TEXT_INPUTS = new Set(["", "text", "search", "url", "tel", "email", "password", "number"]);

/** Whether `el` is a field that takes typed text: a text input, a textarea or editable content. */
export function isTextField(el: unknown): el is FieldLike {
  const f = el as FieldLike | null;
  if (!f || typeof f.tagName !== "string") return false;
  if (f.isContentEditable) return true;
  if (f.tagName === "TEXTAREA") return true;
  return f.tagName === "INPUT" && TEXT_INPUTS.has((f.type ?? "").toLowerCase());
}

/**
 * Whether something is being typed in this window: it has the focus and a text field has it. A
 * key command a MIDI controller fires is held back then, as its key would go to the field (a
 * menu item's goes through `textFieldMenu` instead).
 */
export function typingInField(doc: Pick<DocLike, "activeElement" | "hasFocus"> = document): boolean {
  return doc.hasFocus() && isTextField(doc.activeElement);
}

/** Whether a key pressed in `el` belongs to it, not to the key commands: any input, pop-up or
 *  textarea (a focused checkbox or slider keeps its keys too), and editable content. */
export function keepsKeys(el: unknown): boolean {
  const f = el as FieldLike | null;
  if (!f || typeof f.tagName !== "string") return false;
  return f.tagName === "INPUT" || f.tagName === "SELECT" || f.tagName === "TEXTAREA" || !!f.isContentEditable;
}

/** A text edit a menu item makes in a focused field. */
export type FieldEdit = "select-all" | "undo" | "redo" | "cut" | "copy" | "paste" | "delete-back";

// The menu items that are text edits in a field: the accelerators, and the clipboard items (which
// are key commands, but chosen from the menu bar they'd otherwise cut or paste regions).
// Mod+Backspace is Delete Track's accelerator; in a field it deletes the word before the caret
// (to the line's start on macOS), as it does in any other text field.
const FIELD_EDITS: Record<string, FieldEdit> = {
  "select-all": "select-all",
  undo: "undo",
  redo: "redo",
  cut: "cut",
  copy: "copy",
  paste: "paste",
  "delete-track": "delete-back",
};

/** The edit menu item `id` makes in a focused text field, if any. */
export const fieldEdit = (id: string): FieldEdit | null => FIELD_EDITS[id] ?? null;

const WORD = /[\p{L}\p{N}_]/u;

/**
 * Where Mod+Backspace deletes back to from `at`: the start of the word before it (past any spaces
 * and punctuation first, as GTK and Windows do), or with `toLineStart` the start of the line
 * (macOS's ⌘⌫).
 */
export function deleteBackStart(text: string, at: number, toLineStart = false): number {
  if (toLineStart) return text.lastIndexOf("\n", at - 1) + 1;
  let i = at;
  while (i > 0 && !WORD.test(text[i - 1])) i--;
  while (i > 0 && WORD.test(text[i - 1])) i--;
  return i;
}

const hasSelection = (el: FieldLike): el is TextControl => typeof (el as TextControl).selectionStart === "number";

/** Makes `edit` in the field `el`. Goes through `execCommand` where it can, so the field's own
 *  undo and its input events (React's onChange) follow. */
function applyFieldEdit(el: FieldLike, edit: FieldEdit, doc: DocLike, mac = IS_MAC) {
  if (edit === "select-all") {
    if (typeof (el as TextControl).select === "function") (el as TextControl).select();
    else doc.execCommand("selectAll");
    return;
  }
  if (edit !== "delete-back") {
    doc.execCommand(edit);
    return;
  }
  if (el.readOnly) return;
  if (hasSelection(el)) {
    const { selectionStart: start, selectionEnd: end } = el;
    if (start === end) el.setSelectionRange(deleteBackStart(el.value, start!, mac), end!);
  } else if (el.isContentEditable) {
    doc.getSelection?.()?.modify?.("extend", "backward", mac ? "lineboundary" : "word");
  }
  doc.execCommand("delete");
}

/** The menu items that have an accelerator: menu.rs's `keyed` items and Musical Typing's check
 *  item. A guard test keeps the list in step with menu.rs. */
export const MENU_ACCELERATORS: readonly string[] = [
  "settings",
  "quit",
  "new",
  "chooser-recent",
  "open",
  "save",
  "save-as",
  "import-audio",
  "bounce",
  "export-selected-tracks",
  "export-tracks",
  "undo",
  "redo",
  "duplicate",
  "split",
  "join",
  "select-all",
  "new-tracks",
  "bounce-in-place",
  "folder-stack",
  "group",
  "delete-track",
  "musical-typing",
  "ui-larger",
  "ui-smaller",
  "ui-actual",
];

// Accelerators that still run while a field is being typed in: saving loses nothing typed. (Quit
// never reaches the web view: menu.rs runs it.)
const RUN_IN_FIELD = new Set(["save", "quit"]);

/**
 * What native menu item `id` does while a text field has the focus: "edit" the field (Select
 * All, Undo...), be "ignored" (an accelerator such as Duplicate's Ctrl+D, most likely pressed
 * while typing), or "run" as usual (Save, and items with no accelerator, which only a click
 * chooses).
 */
export function fieldMenuAction(id: string): "edit" | "ignore" | "run" {
  if (fieldEdit(id)) return "edit";
  return MENU_ACCELERATORS.includes(id) && !RUN_IN_FIELD.has(id) ? "ignore" : "run";
}

/**
 * What a key command or menu item a MIDI controller fired does while a text field in a detached
 * panel's window is being typed in: a key command is held back ("drop"), as its key would go to
 * the field; a menu item with an accelerator goes to that window ("panel"), where it edits the
 * field or is dropped (`textFieldMenu` there), as its key would; any other item runs ("run").
 */
export function midiCommandWhileTyping(id: string, menu: boolean): "drop" | "panel" | "run" {
  if (!menu) return "drop";
  return fieldMenuAction(id) === "run" ? "run" : "panel";
}

/**
 * Takes native menu item `id` while a text field has the focus: runs it on the field when it's
 * one of the field's edits, drops it when it's another accelerator (`fieldMenuAction`), and says
 * whether it took it. Call it before anything else handles a menu item. Only while this window
 * has the focus: a field left focused in a window behind keeps it, and an item forwarded from a
 * panel window isn't for it.
 *
 * The menu bar doesn't say whether an item came from its key or a click, so an item with an
 * accelerator chosen with the mouse while a field has the focus is dropped too; pressing Return
 * or clicking away from the field first lets it run.
 */
export function textFieldMenu(id: string, doc: DocLike = document, mac = IS_MAC): boolean {
  const action = fieldMenuAction(id);
  const el = doc.activeElement;
  if (action === "run" || !doc.hasFocus() || !isTextField(el)) return false;
  if (action === "edit") applyFieldEdit(el, fieldEdit(id)!, doc, mac);
  return true;
}
