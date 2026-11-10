// The Event List's keys, and the Edit commands of the native menu bar while it has focus. The main
// window's menu commands (app/menuCommands.ts) and DetachedWindow.tsx reach the latter through
// eventListMenu.
import { commandForEvent } from "../../keyboard/keymap";
import type { Commands } from "./commands";
import type { Core } from "./core";
import * as el from "./model";

let menuHandler: ((id: string) => boolean) | null = null;
let menuRoot: Core["rootRef"] | null = null;

/** Whether the Event List has keyboard focus, so the Edit menu acts on it (menuBar.ts). */
export const eventListHasFocus = () => !!menuRoot?.current?.contains(document.activeElement);

/** Edit menu commands (Select All, Delete) for the Event List while it has focus. */
export function eventListMenu(id: string): boolean {
  return menuHandler?.(id) ?? false;
}

/** While the Event List has focus, the native menu's Edit commands act on it. Call from an effect
 *  that runs on every render, so the handler sees that render's commands. */
export function installMenuHandler(rootRef: Core["rootRef"], c: Commands) {
  const { selectCommand, selectAll, deleteEvents } = c;
  const h = (id: string) => {
    if (!rootRef.current?.contains(document.activeElement)) return false;
    if (selectCommand(id)) return true;
    const f: Record<string, () => void> = { "select-all": selectAll, delete: deleteEvents };
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

/** The list's own keys, while it has focus (not while a value is being typed). */
export function listKeys(core: Core, c: Commands) {
  const { work, live } = core;
  const { selectCommand, deleteEvents, selectNone, stepSelection, startEdit } = c;

  /** Runs the key's action; false when the list doesn't use it. */
  const handle = (e: React.KeyboardEvent): boolean => {
    const cmd = commandForEvent(e.nativeEvent);
    if (cmd && selectCommand(cmd)) return true;
    if (e.ctrlKey || e.metaKey || e.altKey) return false;
    switch (e.key) {
      case "Delete":
      case "Backspace":
        deleteEvents();
        return true;
      case "Escape":
        selectNone();
        return true;
      case "ArrowUp":
      case "ArrowDown":
        stepSelection(e.key === "ArrowUp" ? -1 : 1, e.shiftKey);
        return true;
      case "Enter": {
        // Types into the first selected event's position.
        const i = live.current.rows.findIndex((r) => el.isSelected(work.current.lists, r));
        if (i >= 0) startEdit(i, "pos");
        return true;
      }
    }
    return false;
  };

  return (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).tagName === "INPUT") return;
    if (handle(e)) {
      e.preventDefault();
      e.stopPropagation();
    }
  };
}
