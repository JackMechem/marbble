// The Piano Roll's keys, and the Edit commands of the native menu bar while it has focus. App.tsx and
// DetachedWindow.tsx reach the latter through pianoRollMenu.
import * as me from "../../midiEdit";
import { commandForEvent } from "../../keymap";
import { closeMenu, menuOpen, openMenuAt } from "../Menu";
import type { Commands } from "./commands";
import type { Core } from "./core";
import type { Menus } from "./menus";
import { setPrefs } from "./prefs";
import { TOOLS } from "../../pianoRollModel";

let menuHandler: ((id: string) => boolean) | null = null;

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
    return () => {
      if (menuHandler === h) menuHandler = null;
    };
}

/** The editor's own keys, while it has focus. */
export function editorKeys(ed: Core, c: Commands, m: Menus) {
  const { grid, musical, setSelection, play, work, toolArmed, toolBtn } = ed;
  const { zoomX, zoomY, zoomToFit, notesNow, anySelected, copy, cut, paste, deleteSelected, selectCommand, apply, quantizeNow, nudgeKey } = c;
  const { toolItems } = m;

  const onKeyDown = (e: React.KeyboardEvent) => {
    // An open menu (the tool menu after T aside) has the keys.
    if (isField(e.target) || (menuOpen() && toolArmed.current <= performance.now())) return;
    const mod = e.ctrlKey || e.metaKey;
    const k = e.key;
    const g = grid();
    let handled = true;
    // After T, a letter picks a tool, as in Logic's tool menu.
    if (toolArmed.current > performance.now() && !mod) {
      const t = TOOLS.find((x) => x.key.toLowerCase() === k.toLowerCase());
      toolArmed.current = 0;
      if (t) {
        setPrefs({ tool: t.id });
        closeMenu();
        e.preventDefault();
        e.stopPropagation();
        return;
      }
    }
    const cmd = commandForEvent(e.nativeEvent);
    if (cmd && selectCommand(cmd)) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    if (mod && !e.altKey) {
      // ⌘A and ⌘D are native menu accelerators; they arrive through pianoRollMenu().
      switch (k.toLowerCase()) {
        case "c":
          copy();
          break;
        case "x":
          cut();
          break;
        case "v":
          paste();
          break;
        case "m":
          apply(me.toggleMute, "Mute Notes");
          break;
        case "arrowright":
          zoomX(1.4);
          break;
        case "arrowleft":
          zoomX(1 / 1.4);
          break;
        case "arrowdown":
          zoomY(1.2);
          break;
        case "arrowup":
          zoomY(1 / 1.2);
          break;
        default:
          handled = false;
      }
    } else if (!mod) {
      switch (k) {
        case "Delete":
        case "Backspace":
          deleteSelected();
          break;
        case "ArrowLeft":
        case "ArrowRight": {
          const notes = notesNow();
          if (e.altKey) {
            // Select the previous / next note in time.
            const sorted = notes.map((n, i) => [n, i] as const).sort((a, b) => a[0].start - b[0].start || a[0].pitch - b[0].pitch);
            const cur = sorted.findIndex(([n]) => n.sel);
            const next = sorted[Math.max(0, Math.min(sorted.length - 1, cur < 0 ? 0 : cur + (k === "ArrowRight" ? 1 : -1)))];
            if (next) {
              setSelection(notes.map((n, i) => (i === next[1] ? { ...n, sel: true } : n.sel ? { ...n, sel: false } : n)));
              play(next[0].pitch, next[0].velocity);
              window.setTimeout(() => play(null), 200);
            }
            break;
          }
          if (!anySelected()) break;
          const step = e.shiftKey ? g.bar : g.grid;
          apply((n) => musical(n, (v) => me.nudge(v, k === "ArrowRight" ? step : -step, 0)), "Nudge Notes", nudgeKey());
          break;
        }
        case "ArrowUp":
        case "ArrowDown": {
          if (!anySelected()) break;
          const d = (k === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 12 : 1);
          apply((n) => me.nudge(n, 0, d), "Transpose Notes", nudgeKey());
          const first = notesNow().find((n) => n.sel);
          if (first) {
            play(first.pitch, first.velocity);
            window.setTimeout(() => play(null), 200);
          }
          break;
        }
        case "q":
        case "Q":
          quantizeNow();
          break;
        case "t":
        case "T":
          toolArmed.current = performance.now() + 2000;
          if (toolBtn.current) openMenuAt(toolBtn.current, toolItems());
          break;
        case "z":
        case "Z":
          zoomToFit();
          break;
        case "Escape":
          setSelection(me.selectNone(notesNow()), work.current.cc.map((c) => (c.sel ? { ...c, sel: false } : c)));
          break;
        default:
          handled = false;
      }
    } else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  return onKeyDown;
}
