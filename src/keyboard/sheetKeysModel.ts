// Which keys reach the app while sheets are open: the pure part of sheetKeys.ts (which has the
// rule in full). tests/keyboard/sheetKeys.test.ts covers it.

/**
 * How a sheet treats the app's key commands. A modal sheet (Strip Silence, Group Settings, the
 * name sheet, Settings, Bounce…) holds every one back; a non-modal one (Controller Assignments,
 * a window in Logic) lets the transport and the Control Surfaces commands through, so Learn
 * Assignment works while it is open, and holds back the rest, editing included.
 */
export type SheetMode = "modal" | "non-modal";

/** The key commands a non-modal sheet lets through (keymap.ts ids). */
export const NON_MODAL_COMMANDS: ReadonlySet<string> = new Set([
  "play",
  "record",
  "cycle",
  "metronome",
  "count-in",
  "rewind",
  "forward",
  "catch-playhead",
  "show-playhead",
  "learn-assignment",
  "controller-assignments",
]);

/** What a key pressed outside every open sheet does: reaches the app ("pass"), is dropped
 *  ("block"), closes the top sheet (Esc) or presses its default button (Return). */
export type OutsideKey = "pass" | "block" | "close" | "default";

/** Whether key command `cmd` is held back while sheets of these modes are open. */
export function heldBack(modes: SheetMode[], cmd: string | null): boolean {
  if (!modes.length) return false;
  if (modes.includes("modal")) return true;
  return !cmd || !NON_MODAL_COMMANDS.has(cmd);
}

/**
 * A key press whose target is outside the open sheets (`modes`, the top one last): `key` is
 * KeyboardEvent.key, `cmd` the key command it runs, if any. Tab still moves the focus.
 */
export function outsideKey(modes: SheetMode[], key: string, cmd: string | null): OutsideKey {
  if (!modes.length || key === "Tab") return "pass";
  if (key === "Escape") return "close";
  if (key === "Enter") return "default";
  return heldBack(modes, cmd) ? "block" : "pass";
}

/**
 * Whether Return pressed in `el`, inside a sheet, has its default action stopped: in a one-line
 * field, where it has none to keep. A field's Return often closes the sheet (the name sheet's
 * OK), and the focus then goes back to what opened it; the key's default would go there too and
 * press that button, opening the sheet again. A textarea keeps its new line and a button its
 * press.
 */
export const stopsReturn = (el: unknown): boolean => (el as { tagName?: unknown } | null)?.tagName === "INPUT";
