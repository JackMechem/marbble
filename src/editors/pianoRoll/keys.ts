// The Piano Roll's keys, and the Edit commands of the native menu bar while it has focus. The main
// window's menu commands (app/menuCommands.ts) and DetachedWindow.tsx reach the latter through
// pianoRollMenu.
import * as me from "../midiEdit";
import { commandForEvent } from "../../keyboard/keymap";
import { closeMenu, menuOpen, openMenuAt } from "../../ui/Menu";
import type { Commands } from "./commands";
import type { Core } from "./core";
import type { Menus } from "./menus";
import { setPrefs } from "./prefs";
import { TOOLS } from "./model";

let menuHandler: ((id: string) => boolean) | null = null;
let menuRoot: Core["rootRef"] | null = null;

/** Whether the Piano Roll has keyboard focus, so the Edit menu acts on it (menuBar.ts). */
export const pianoRollHasFocus = () => !!menuRoot?.current?.contains(document.activeElement);

/**
 * Lets the Piano Roll take Edit menu commands (Select All, Copy, Duplicate…) while it has
 * keyboard focus. Returns true when it handled `id`.
 */
export function pianoRollMenu(id: string): boolean {
  return menuHandler?.(id) ?? false;
}

const isField = (t: EventTarget | null) => {
  const el = t as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "SELECT" || el.tagName === "TEXTAREA");
};

/** While the Piano Roll has focus, the native menu's Edit commands act on it. Call from an effect
 *  that runs on every render, so the handler sees that render's commands. */
export function installMenuHandler(rootRef: Core["rootRef"], c: Commands) {
  const { selectCommand, selectAll, copy, cut, paste, duplicate, deleteSelected } = c;
  const h = (id: string) => {
    if (!rootRef.current?.contains(document.activeElement)) return false;
    if (selectCommand(id)) return true;
    const f: Record<string, () => void> = { "select-all": selectAll, copy, cut, paste, duplicate, delete: deleteSelected };
    if (!f[id]) return false;
    f[id]();
    return true;
  };
  menuHandler = h;
  menuRoot = rootRef;
  return () => {
    if (menuHandler === h) {
      menuHandler = null;
      menuRoot = null;
    }
  };
}

/** The editor's own keys, while it has focus. */
export function editorKeys(ed: Core, c: Commands, m: Menus) {
  const { grid, musical, setSelection, play, work, toolArmed, toolBtn } = ed;
  const { zoomX, zoomY, zoomToFit, notesNow, anySelected, copy, cut, paste, deleteSelected, selectCommand, apply, quantizeNow, nudgeKey } = c;
  const { toolItems } = m;

  /** Plays `n` for a moment, so a key that selects or transposes notes is heard. */
  const audition = (n: me.PNote) => {
    play(n.pitch, n.velocity);
    window.setTimeout(() => play(null), 200);
  };

  /** After T, a letter picks a tool, as in Logic's tool menu. True when it did. */
  const pickArmedTool = (k: string, mod: boolean): boolean => {
    if (toolArmed.current <= performance.now() || mod) return false;
    const t = TOOLS.find((x) => x.key.toLowerCase() === k.toLowerCase());
    toolArmed.current = 0;
    if (!t) return false;
    setPrefs({ tool: t.id });
    closeMenu();
    return true;
  };

  /** ⌥← / ⌥→: selects the previous / next note in time, and plays it. */
  const selectNeighbour = (forward: boolean) => {
    const notes = notesNow();
    const sorted = notes.map((n, i) => [n, i] as const).sort((a, b) => a[0].start - b[0].start || a[0].pitch - b[0].pitch);
    const cur = sorted.findIndex(([n]) => n.sel);
    const next = sorted[Math.max(0, Math.min(sorted.length - 1, cur < 0 ? 0 : cur + (forward ? 1 : -1)))];
    if (!next) return;
    setSelection(notes.map((n, i) => (i === next[1] ? { ...n, sel: true } : n.sel ? { ...n, sel: false } : n)));
    audition(next[0]);
  };

  /** ← / →: moves the selected notes by the grid, or by a bar with ⇧. */
  const nudgeSelected = (forward: boolean, byBar: boolean) => {
    if (!anySelected()) return;
    const g = grid();
    const step = byBar ? g.bar : g.grid;
    apply((n) => musical(n, (v) => me.nudge(v, forward ? step : -step, 0)), "Nudge Notes", nudgeKey());
  };

  /** ↑ / ↓: transposes the selected notes a semitone, or an octave with ⇧, and plays the first. */
  const transposeSelected = (up: boolean, octave: boolean) => {
    if (!anySelected()) return;
    const d = (up ? 1 : -1) * (octave ? 12 : 1);
    apply((n) => me.nudge(n, 0, d), "Transpose Notes", nudgeKey());
    const first = notesNow().find((n) => n.sel);
    if (first) audition(first);
  };

  /** A key without ⌘ or Ctrl. True when it is one of the editor's. */
  const plainKey = (e: React.KeyboardEvent): boolean => {
    const k = e.key;
    switch (k) {
      case "Delete":
      case "Backspace":
        deleteSelected();
        return true;
      case "ArrowLeft":
      case "ArrowRight":
        if (e.altKey) selectNeighbour(k === "ArrowRight");
        else nudgeSelected(k === "ArrowRight", e.shiftKey);
        return true;
      case "ArrowUp":
      case "ArrowDown":
        transposeSelected(k === "ArrowUp", e.shiftKey);
        return true;
      case "q":
      case "Q":
        quantizeNow();
        return true;
      case "t":
      case "T":
        toolArmed.current = performance.now() + 2000;
        if (toolBtn.current) openMenuAt(toolBtn.current, toolItems());
        return true;
      case "z":
      case "Z":
        zoomToFit();
        return true;
      case "Escape":
        setSelection(me.selectNone(notesNow()), work.current.cc.map((c) => (c.sel ? { ...c, sel: false } : c)));
        return true;
      default:
        return false;
    }
  };

  // ⌘A and ⌘D are native menu accelerators; they arrive through pianoRollMenu().
  const commandKeys = new Map<string, () => void>([
    ["c", copy],
    ["x", cut],
    ["v", paste],
    ["m", () => apply(me.toggleMute, "Mute Notes")],
    ["arrowright", () => zoomX(1.4)],
    ["arrowleft", () => zoomX(1 / 1.4)],
    ["arrowdown", () => zoomY(1.2)],
    ["arrowup", () => zoomY(1 / 1.2)],
  ]);

  /** A key with ⌘ or Ctrl (and not ⌥). True when it is one of the editor's. */
  const commandKey = (k: string): boolean => {
    const run = commandKeys.get(k.toLowerCase());
    run?.();
    return !!run;
  };

  /** A key command of the keymap's that selects notes (Select All Following…). True when it was. */
  const selectionKey = (e: React.KeyboardEvent): boolean => {
    const cmd = commandForEvent(e.nativeEvent);
    return !!cmd && selectCommand(cmd);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    // An open menu (the tool menu after T aside) has the keys.
    if (isField(e.target) || (menuOpen() && toolArmed.current <= performance.now())) return;
    const mod = e.ctrlKey || e.metaKey;
    const handled = pickArmedTool(e.key, mod) || selectionKey(e) || (mod ? !e.altKey && commandKey(e.key) : plainKey(e));
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  return onKeyDown;
}
