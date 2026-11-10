// Keeps the native menu bar (src-tauri/src/menu/) in step with the app: the key commands' keys
// beside their items (from Settings › Key Commands, so a reassigned key shows there too), ticks,
// greyed-out items and the Undo/Redo names. The states come from the Tracks area's own menus,
// which have the same items by id (TracksArea.tsx), and, for the items only the menu bar has,
// from the state their commands use. Only what changed is sent.
import { call } from "../../engine/commands";
import { getState, subscribeState } from "../../state/store";
import { getSettings, onSettingsChange } from "../../settings/appSettings";
import { formatKey, KEY_COMMANDS, keysFor } from "../../keyboard/keymap";
import { editMenu, functionsMenu, viewMenu } from "../../tracks/TracksArea";
import { changedStates, keyStates, mergeStates, statesOf, type ItemState } from "./menuBarModel";
import { EDIT_MENU, type MenuShape } from "./menuLabels";
import { selectedTracks } from "../../tracks/trackCommands";
import { canFreeze } from "../../tracks/freeze";
import { canHaveAlternatives } from "../../tracks/alternatives/trackAlternatives";
import { THEMES } from "../../theme/themes";
import { pianoRollHasFocus } from "../../editors/pianoRoll/keys";
import { eventListHasFocus } from "../../editors/eventList/EventList";
import { editorTakesEdit } from "../../editors/audioEditor/state";

/** The Edit menu's item ids. */
const editIds = (shape: MenuShape): string[] => shape.flatMap((s) => (s === "-" ? [] : typeof s === "object" ? editIds(s.items) : [s]));
const EDIT_IDS = editIds(EDIT_MENU).filter((id) => id !== "undo" && id !== "redo");

/** The items only the menu bar has: whether they apply, and their ticks. */
function ownStates(): Map<string, ItemState> {
  const s = getState();
  const p = s.snap?.project;
  const ids = selectedTracks();
  const sel = p?.tracks.filter((t) => ids.includes(t.id)) ?? [];
  const cur = p?.tracks.find((t) => t.id === s.snap?.selected);
  const alt = !!cur && canHaveAlternatives(cur) && !cur.frozen;
  const st: ItemState[] = [
    { id: "export-selected-tracks", enabled: ids.length > 0 },
    { id: "revert-saved", enabled: !!s.snap?.saved && !!s.snap.dirty },
    { id: "new-pattern-region", enabled: cur?.kind === "instrument" },
    { id: "bounce-in-place", enabled: ids.length > 0 || s.selectedRegions.length > 0 },
    { id: "freeze-track", enabled: sel.some((t) => canFreeze(t) && !t.frozen && t.regions.length > 0) },
    { id: "unfreeze-track", enabled: sel.some((t) => !!t.frozen) },
    { id: "track-on-off", enabled: sel.length > 0, checked: sel.length > 0 && sel.every((t) => !t.off) },
    { id: "drum-replace", enabled: cur?.kind === "audio" },
    { id: "track-alt-new", enabled: alt },
    { id: "track-alt-duplicate", enabled: alt },
    { id: "track-alt-rename", enabled: alt },
    { id: "track-alt-delete", enabled: alt && !!cur?.alternatives?.length },
    { id: "cycle", checked: !!p?.cycle.enabled },
    { id: "autopunch", checked: !!p?.punch.enabled },
    { id: "metronome", checked: !!p?.metronome },
    { id: "count-in", checked: !!p?.count_in },
    { id: "low-latency", checked: getSettings().prefs.latency.low_latency },
    { id: "editor", checked: s.bottom === "editor" },
    { id: "list-editors", checked: s.bottom === "list" },
    { id: "detach:mixer", checked: s.detached.includes("mixer") },
    { id: "detach:editor", checked: s.detached.includes("editor") },
    { id: "detach:list", checked: s.detached.includes("list") },
    { id: "musical-typing", checked: s.showTyping },
    { id: "step-input", checked: s.showStepInput },
    ...THEMES.map((t) => ({ id: `theme:${t.id}`, checked: s.themeId === t.id })),
  ];
  // The editors take the Edit menu while they have focus (or may, in their own window), and
  // decide for themselves what applies there.
  if (pianoRollHasFocus() || eventListHasFocus() || editorTakesEdit() || s.detached.includes("editor") || s.detached.includes("list")) {
    st.push(...EDIT_IDS.map((id) => ({ id, enabled: true })));
  }
  return new Map(st.map((x) => [x.id, x]));
}

/** Every item's state as it should be now. */
function currentStates(): Map<string, ItemState> {
  const keys = keyStates(
    KEY_COMMANDS.map((c) => c.id),
    (id) => keysFor(id),
    formatKey,
  );
  return mergeStates(keys, statesOf([...editMenu(), ...functionsMenu(), ...viewMenu()]), ownStates());
}

const sent = new Map<string, ItemState>();
let queued = false;

function sync() {
  queued = false;
  if (!getState().snap) return;
  const changed = changedStates(currentStates(), sent);
  if (changed.length) call("set_menu_state", { items: changed });
}

/** Brings the menu bar up to date soon: after the change in hand, but before a key pressed next
 *  could find an item still greyed out. */
function schedule() {
  if (queued) return;
  queued = true;
  window.setTimeout(sync, 0);
}

/** Follows the app's state, the settings (keys, Low Latency Mode) and the keyboard focus (the
 *  editors take the Edit menu). Main window only: the bar is the same in every window. */
export function installMenuBar(): () => void {
  const offs = [subscribeState(schedule), onSettingsChange(schedule)];
  window.addEventListener("focusin", schedule);
  window.addEventListener("focusout", schedule);
  schedule();
  return () => {
    offs.forEach((f) => f());
    window.removeEventListener("focusin", schedule);
    window.removeEventListener("focusout", schedule);
  };
}
