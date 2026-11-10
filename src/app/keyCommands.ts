// The main window's key commands: what each one does (keyboard/keymap.ts has the commands and
// their keys), the window's keydown listener, and the commands MIDI controllers fire.
import { emitTo } from "@tauri-apps/api/event";
import { act, api } from "../engine/commands";
import { hideTracks, keepRegionsOn, toggleShowHidden } from "../tracks/trackCommands";
import { copyRegions, cutRegions, deleteRegions, pasteRegions, setLocatorsByRegions, toggleLoop, toggleRegionMute } from "../tracks/regionCommands";
import { commandForEvent, isMenuCommand } from "../keyboard/keymap";
import { runsOnKey } from "../keyboard/keyRepeat";
import { nudgeSamples, type Nudge } from "../settings/appSettings";
import { evenNudge, movedStart, nudgeTarget } from "../tracks/regions/musicalMove";
import { tempoMap } from "../lib/tempo";
import { runStepInputCommand } from "../editors/noteInput/StepInput";
import { getState, setState } from "../state/store";
import { transport } from "./controlBar/transport";
import { currentLayout, stepVisible, zoomTracks } from "../tracks/trackLayout";
import { revealTop, scrollTo, viewport } from "../tracks/viewport";
import { deleteSelectedPoints } from "../tracks/automation/canvas";
import { deleteSelectedTempoEvent } from "../tracks/globalTracks/GlobalTracks";
import { marqueeCopy, marqueeCut, marqueeDelete, marqueePaste, openToolMenu } from "../tracks/tools/trackTools";
import { getSettings, toggleReplace } from "../settings/appSettings";
import { deleteSelectedMarker } from "../tracks/markers/markers";
import { moveSelectedSection, removeSelectedSection, stepTake } from "../tracks/takes/takes";
import { typingPanel } from "./detach/detach";
import { moveAutomationAnswer } from "../tracks/regions/moveAutomation";
import { resetClipIndicators } from "../mixer/stripSettings";
import { sheetBlocks } from "../keyboard/sheetKeys";
import { ARRANGE_COMMANDS } from "../tracks/arrangeCommands";
import { keyboardFocus } from "../keyboard/focusRing";
import { leavesKeyToFocus } from "../keyboard/keyNav";
import { keepsKeys, midiCommandWhileTyping, textFieldMenu, typingInField } from "../keyboard/textKeys";
import { handleMenu } from "./menuCommands";

function selectedTrack() {
  const s = getState().snap;
  return s?.project.tracks.find((t) => t.id === s.selected) ?? null;
}

function moveSelection(d: number) {
  const s = getState().snap;
  if (!s || !s.project.tracks.length) return;
  const i = s.project.tracks.findIndex((t) => t.id === s.selected);
  // Step over tracks that aren't in view (hidden, or inside a collapsed stack).
  const l = currentLayout();
  const from = i < 0 ? stepVisible(l, -1, 1) : i;
  const n = i < 0 ? from : stepVisible(l, from, d);
  if (n < 0 || l.heights[n] === 0) return;
  keepRegionsOn([s.project.tracks[n].id]);
  api.selectTrack(s.project.tracks[n].id);
  // Bring the newly selected track into view, as Logic does.
  const vp = viewport();
  const top = revealTop(l.tops[n], l.heights[n], vp.top, vp.height);
  if (top !== null) scrollTo(vp.left, top);
}

/** What each key command does (see keymap.ts for the commands and their keys). */
export const KEY_ACTIONS: Record<string, () => void> = {
  play: () => act("toggle_play"),
  record: () => act("record"),
  "discard-recording": () => handleMenu("discard-recording"),
  "mark-take-good": () => markTake("good"),
  "mark-take-bad": () => markTake("bad"),
  cycle: () => transport.toggleCycle(),
  metronome: () => handleMenu("metronome"),
  "count-in": () => handleMenu("count-in"),
  "to-start": () => transport.toStart(),
  rewind: () => transport.nudgeBars(-1),
  forward: () => transport.nudgeBars(1),
  "catch-playhead": () => setState({ catchPlayhead: !getState().catchPlayhead }),
  "show-playhead": () => transport.showPlayhead(),
  mixer: () => handleMenu("mixer"),
  inspector: () => handleMenu("inspector"),
  library: () => handleMenu("library"),
  editor: () => handleMenu("editor"),
  "list-editors": () => handleMenu("list-editors"),
  browser: () => handleMenu("browser"),
  "zoom-in": () => handleMenu("zoom-in"),
  "zoom-out": () => handleMenu("zoom-out"),
  // Logic: ⌘↓ zooms in vertically (taller tracks), ⌘↑ zooms out.
  "taller-tracks": () => zoomTracks(1.2),
  "shorter-tracks": () => zoomTracks(1 / 1.2),
  "track-mute": () => {
    const t = selectedTrack();
    if (t) api.updateTrack(t.id, { mute: !t.mute });
  },
  "track-solo": () => {
    const t = selectedTrack();
    if (t) api.updateTrack(t.id, { solo: !t.solo });
  },
  "track-arm": () => {
    const t = selectedTrack();
    if (t) api.updateTrack(t.id, { armed: !t.armed });
  },
  "track-on-off": () => handleMenu("track-on-off"),
  // With a comp section selected in an open take folder, Up and Down move it between takes.
  "select-previous": () => moveSelectedSection(-1) || moveSelection(-1),
  "select-next": () => moveSelectedSection(1) || moveSelection(1),
  // Clipboard keys stay with text fields (onKey skips them), so the menu bar doesn't take them.
  // With a marquee they act on the part of the regions inside it.
  copy: () => marqueeCopy() || copyRegions(),
  cut: () => marqueeCut() || cutRegions(),
  paste: () => marqueePaste() || pasteRegions(),
  "mute-regions": () => toggleRegionMute(),
  "delete-regions": () => {
    // Selected automation points (or a tempo change) go first, as in Logic.
    if (deleteSelectedPoints() || deleteSelectedTempoEvent() || deleteSelectedMarker() || marqueeDelete() || removeSelectedSection()) return;
    if (getState().selectedRegions.length) deleteRegions();
  },
  "set-locators": () => setLocatorsByRegions(),
  "zero-crossings": () => handleMenu("zero-crossings"),
  loop: () => toggleLoop(),
  "show-hidden": () => toggleShowHidden(),
  "hide-tracks": () => hideTracks(),
  "reset-clips": () => resetClipIndicators(null),
  automation: () => handleMenu("automation"),
  "used-automation": () => handleMenu("used-automation"),
  "global-tracks": () => handleMenu("global-tracks"),
  "nudge-left": () => nudgeRegions(-1),
  "nudge-right": () => nudgeRegions(1),
  "nudge-left-sample": () => nudgeRegions(-1, "smp1"),
  "nudge-right-sample": () => nudgeRegions(1, "smp1"),
  "project-settings": () => handleMenu("project-settings"),
  "settings-keys": () => handleMenu("settings-keys"),
  autopunch: () => handleMenu("autopunch"),
  "replace-mode": () => void toggleReplace(),
  "create-marker": () => handleMenu("create-marker"),
  "delete-marker": () => handleMenu("delete-marker"),
  "previous-marker": () => handleMenu("previous-marker"),
  "next-marker": () => handleMenu("next-marker"),
  "marker-list": () => handleMenu("marker-list"),
  "pack-takes": () => handleMenu("pack-takes"),
  "previous-take": () => stepTake(-1),
  "next-take": () => stepTake(1),
  "tool-menu": openToolMenu,
  "learn-assignment": () => handleMenu("learn-assignment"),
  "controller-assignments": () => handleMenu("controller-assignments"),
  "step-input": () => handleMenu("step-input"),
  ...ARRANGE_COMMANDS,
};

/** Mark Take as Good / Bad: the take being recorded or just recorded, else the take heard in
 *  each selected take folder. */
function markTake(rating: "good" | "bad") {
  const s = getState();
  const folders = (s.snap?.project.tracks ?? []).flatMap((t) => t.regions.filter((r) => r.takes && s.selectedRegions.includes(r.id)).map((r) => r.id));
  act("mark_take", { rating, regions: folders });
}

/** Nudge Left / Right: by the Nudge Value, or by `step` (the by-sample key commands). */
function nudgeRegions(dir: number, step: Nudge = getSettings().ui.nudge) {
  const s = getState();
  if (!s.snap || !s.selectedRegions.length) return;
  // A bar or beat as long as it is where the selection starts.
  const at = Math.min(...s.snap.project.tracks.flatMap((t) => t.regions.filter((r) => s.selectedRegions.includes(r.id)).map((r) => r.start)));
  const ids = s.selectedRegions;
  // By milliseconds or samples they all move the same number of samples, so the first stops
  // at the project start rather than the others moving without it.
  const delta = evenNudge(Number.isFinite(at) ? at : 0, dir * nudgeSamples(s.snap.project, Number.isFinite(at) ? at : 0, step));
  if (!delta) return;
  // By a bar, beat or note value the first region moves through the tempo map and the others
  // keep their bars and beats.
  const map = tempoMap(s.snap.project);
  const to = Number.isFinite(at) ? nudgeTarget(map, at, step, dir) : null;
  const moves = s.snap.project.tracks.flatMap((t) =>
    t.regions.filter((r) => ids.includes(r.id)).map((r) => ({ id: r.id, start: to === null ? Math.max(0, r.start + delta) : movedStart(map, r.start, at, to, true) })),
  );
  const musical = to === null ? {} : { from: at, to };
  moveAutomationAnswer(s.snap.project, moves).then((automation) => act("nudge_regions", { ids, delta, automation, ...musical }));
}

export function onKey(e: KeyboardEvent) {
  const tgt = e.target as HTMLElement;
  if (keepsKeys(tgt)) return;
  if (getState().dialog) return;
  // Return and Space press what Tab focused (keyNav.ts) instead of going to the beginning and playing.
  if (leavesKeyToFocus(e, tgt, keyboardFocus())) return;
  // Shortcuts the native menu owns never reach here; everything else comes from the keymap.
  const id = commandForEvent(e);
  const run = id ? KEY_ACTIONS[id] : undefined;
  if (!run) return;
  e.preventDefault();
  if (id && runsOnKey(id, e.repeat)) run();
}

/**
 * A key command or menu item a MIDI controller fired (Controller Assignments; the engine passes
 * them on in the tick). It runs as its key would here, held back by the same things: an open
 * dialog, a sheet that holds the key back, a text field being typed in here or in a detached
 * panel's window (where a menu item's accelerator edits the field or is dropped). Step Input's
 * commands go to it while it is open, as a key passed on from a panel's window does.
 */
export function runMidiCommand(id: string) {
  const menu = isMenuCommand(id);
  if (menu ? textFieldMenu(id) : typingInField()) return;
  const panel = typingPanel();
  if (panel) {
    const where = midiCommandWhileTyping(id, menu);
    if (where === "panel") emitTo(panel, "menu", id).catch(() => {});
    if (where !== "run") return;
  }
  if (getState().dialog || sheetBlocks(id)) return;
  if (menu) handleMenu(id);
  else if (!runStepInputCommand(id, false)) KEY_ACTIONS[id]?.();
}
