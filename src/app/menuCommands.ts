// The main window's menu commands: what each item of the native menu bar does (its ids are in
// src-tauri/src/menu/), also run by the key commands that share an item and by MIDI controllers.
// An editor with the focus takes its own Edit commands first.
import { act } from "../engine/commands";
import { isBouncing, openBounce } from "../project/bounce/bounce";
import { bounceInPlace, createStack, deleteTracks, deleteUnusedTracks, duplicateTracks, groupTracks, hideTracks, selectedTracks, switchAll, toggleShowHidden, unhideAllTracks } from "../tracks/trackCommands";
import { convertLoops, copyRegions, cutRegions, duplicate, joinRegions, newPatternRegion, pasteRegions, repeatRegions, selectAllRegions, setLocatorsByRegions, toggleLoop } from "../tracks/regionCommands";
import { toggleZeroCrossings } from "../tracks/regions/zeroCrossings";
import { eventListMenu } from "../editors/eventList/EventList";
import { toggleOverview } from "../tracks/overview/Overview";
import { importAudioDialog, openProjectDialog, saveProject, undoRedo } from "../project/files";
import { currentTick, getState, setState, showToast } from "../state/store";
import { transport } from "./controlBar/transport";
import { applyTheme } from "../theme/themes";
import { applyUiScale, stepUiScale } from "../ui/uiScale";
import { stepZoom } from "../tracks/zoom";
import { newAux } from "../mixer/routing";
import { editorMenuCommand } from "../editors/audioEditor/state";
import { showUsedAutomation } from "../tracks/automation/menus";
import { pianoRollMenu } from "../editors/pianoRoll/keys";
import { exportSelectionMidi, importMidiDialog } from "../project/midiFiles";
import { freezeTracks, unfreezeTracks } from "../tracks/freeze";
import { marqueeCopy, marqueeCut, marqueePaste } from "../tracks/tools/trackTools";
import { getSettings, updateSettings } from "../settings/appSettings";
import { createMarker, deleteMarker, gotoMarker, markerAtPlayhead, openMarkerList } from "../tracks/markers/markers";
import { packTakeFolder } from "../tracks/takes/takes";
import { toggleDetached } from "./detach/detach";
import { learnLastTouched, openControllerAssignments } from "../controllers/learn";
import { newAlternative, revertToBackup, revertToSaved, saveAsTemplate, switchAlternative } from "../project/projectFiles";
import { deleteTrackAlternative, newTrackAlternative, renameTrackAlternative, trackForAlternatives } from "../tracks/alternatives/trackAlternatives";
import { runArrangeCommand } from "../tracks/arrangeCommands";
import { openDrumReplace } from "../tracks/drumReplace/DrumReplaceSheet";
import { checkForUpdates } from "./updates/updates";
import { textFieldMenu } from "../keyboard/textKeys";
import { KEY_ACTIONS } from "./keyCommands";

/** Runs the menu bar item `id`, after giving a focused text field or editor the chance to. */
export function handleMenu(id: string) {
  // Select All, Undo and the rest edit a focused name or search field, and the other
  // accelerators (Ctrl+D...) are dropped while it has the focus (textKeys.ts).
  if (textFieldMenu(id)) return;
  // With a range selected in the Audio Track editor, Split, Cut, Copy and Delete act on it.
  if (editorMenuCommand(id)) return;
  const s = getState();
  // A render holds the session; commands sent now would stall the window until it finishes.
  if (isBouncing()) return;
  // Edit commands act on the notes while the Piano Roll has focus.
  if (pianoRollMenu(id)) return;
  if (eventListMenu(id)) return;
  if (runArrangeCommand(id)) return;
  switch (id) {
    case "new":
      setState({ dialog: "chooser", chooserTab: "new" });
      break;
    case "undo":
      undoRedo("undo");
      break;
    case "redo":
      undoRedo("redo");
      break;
    case "musical-typing":
      setState({ showTyping: !getState().showTyping });
      break;
    case "step-input":
      setState({ showStepInput: !getState().showStepInput });
      break;
    case "chooser-recent":
      setState({ dialog: "chooser", chooserTab: "recent" });
      break;
    case "open":
      openProjectDialog();
      break;
    case "save":
      saveProject(false);
      break;
    case "save-as":
      saveProject(true);
      break;
    case "save-template":
      saveAsTemplate();
      break;
    case "revert-saved":
      revertToSaved();
      break;
    case "alt-new":
      newAlternative();
      break;
    case "alt-edit":
      setState({ dialog: "alternatives" });
      break;
    case "track-alt-new":
    case "track-alt-duplicate": {
      const t = trackForAlternatives();
      if (t) newTrackAlternative(t, id === "track-alt-duplicate");
      break;
    }
    case "track-alt-rename": {
      const t = trackForAlternatives();
      if (t) renameTrackAlternative(t);
      break;
    }
    case "track-alt-delete": {
      const t = trackForAlternatives();
      if (t) deleteTrackAlternative(t);
      break;
    }
    case "import-audio":
      importAudioDialog();
      break;
    case "bounce":
      openBounce("project");
      break;
    case "export-tracks":
      openBounce("tracks");
      break;
    case "export-selected-tracks": {
      const ids = selectedTracks();
      if (ids.length) openBounce("selected", null, ids);
      else showToast("Select the tracks to export first", 2500);
      break;
    }
    case "bounce-in-place":
      bounceInPlace();
      break;
    case "track-on-off":
      switchAll("off");
      break;
    case "drum-replace": {
      const t = s.snap?.selected;
      if (t != null) openDrumReplace(t);
      break;
    }
    case "freeze-track":
      freezeTracks(selectedTracks());
      break;
    case "unfreeze-track":
      unfreezeTracks(selectedTracks());
      break;
    case "low-latency": {
      const on = !getSettings().prefs.latency.low_latency;
      updateSettings((st) => ({ ...st, prefs: { ...st.prefs, latency: { ...st.prefs.latency, low_latency: on } } }));
      showToast(`Low Latency Mode ${on ? "on" : "off"}`, 1500);
      break;
    }
    case "import-midi":
      importMidiDialog();
      break;
    case "export-midi":
      exportSelectionMidi();
      break;
    case "settings":
    case "settings-audio":
      setState({ dialog: "settings", settingsTab: "audio" });
      break;
    case "settings-appearance":
      setState({ dialog: "settings", settingsTab: "appearance" });
      break;
    case "settings-keys":
      setState({ dialog: "settings", settingsTab: "keys" });
      break;
    case "help-updates":
      checkForUpdates(true);
      break;
    case "project-settings":
      setState({ dialog: "project-settings" });
      break;
    case "new-tracks":
      setState({ dialog: "new-tracks" });
      break;
    case "new-aux":
      newAux();
      break;
    case "delete-track":
      deleteTracks();
      break;
    case "duplicate":
      duplicate();
      break;
    case "duplicate-track":
      duplicateTracks(true);
      break;
    case "same-settings":
      duplicateTracks(false);
      break;
    case "new-pattern-region":
      newPatternRegion();
      break;
    case "delete-unused":
      deleteUnusedTracks();
      break;
    case "folder-stack":
      createStack(false);
      break;
    case "summing-stack":
      createStack(true);
      break;
    case "group":
      groupTracks();
      break;
    case "cut":
      if (!marqueeCut()) cutRegions();
      break;
    case "copy":
      if (!marqueeCopy()) copyRegions();
      break;
    case "paste":
      if (!marqueePaste()) pasteRegions();
      break;
    case "join":
      joinRegions();
      break;
    case "set-locators":
      setLocatorsByRegions();
      break;
    case "zero-crossings":
      toggleZeroCrossings();
      break;
    case "loop":
      toggleLoop();
      break;
    case "convert-loops":
      convertLoops();
      break;
    case "hide-tracks":
      hideTracks();
      break;
    case "unhide-all":
      unhideAllTracks();
      break;
    case "show-hidden":
      toggleShowHidden();
      break;
    case "split":
      act("split_regions", { pos: currentTick()?.pos ?? 0, ids: s.selectedRegions });
      break;
    case "select-all":
      selectAllRegions();
      break;
    case "delete":
      // As the Delete key: selected automation points, a tempo change or a marker go first.
      KEY_ACTIONS["delete-regions"]();
      break;
    case "record":
      act("record");
      break;
    case "discard-recording":
      act("discard_last_recording");
      break;
    case "play":
      act("toggle_play");
      break;
    case "metronome":
      act("set_metronome", { on: !s.snap?.project.metronome });
      break;
    case "count-in":
      act("set_count_in", { on: !s.snap?.project.count_in });
      break;
    case "cycle":
      transport.toggleCycle();
      break;
    case "autopunch":
      transport.togglePunch();
      break;
    case "create-marker":
      createMarker();
      break;
    case "delete-marker": {
      const m = markerAtPlayhead();
      if (m) deleteMarker(m.id);
      break;
    }
    case "previous-marker":
      gotoMarker(-1);
      break;
    case "next-marker":
      gotoMarker(1);
      break;
    case "marker-list":
      openMarkerList();
      break;
    case "learn-assignment":
      learnLastTouched();
      break;
    case "controller-assignments":
      openControllerAssignments();
      break;
    case "pack-takes":
      packTakeFolder();
      break;
    case "mixer":
      setState({ bottom: s.bottom === "mixer" ? "none" : "mixer" });
      break;
    case "inspector":
      setState({ showInspector: !s.showInspector });
      break;
    case "library":
      setState({ showLibrary: !s.showLibrary });
      break;
    case "editor":
      setState({ bottom: s.bottom === "editor" ? "none" : "editor" });
      break;
    case "list-editors":
      setState({ bottom: s.bottom === "list" ? "none" : "list" });
      break;
    case "browser":
      setState({ showBrowser: !s.showBrowser });
      break;
    case "automation":
      setState({ showAutomation: !s.showAutomation });
      break;
    case "used-automation":
      showUsedAutomation();
      break;
    case "output-track":
      setState({ showOutput: !s.showOutput });
      break;
    case "global-tracks":
      setState({ showGlobalTracks: !s.showGlobalTracks });
      break;
    case "overview":
      toggleOverview();
      break;
    case "ui-larger":
      stepUiScale(1);
      break;
    case "ui-smaller":
      stepUiScale(-1);
      break;
    case "ui-actual":
      applyUiScale(1);
      break;
    case "zoom-in":
      setState({ pxPerBeat: stepZoom(s.pxPerBeat, 1) });
      break;
    case "zoom-out":
      setState({ pxPerBeat: stepZoom(s.pxPerBeat, -1) });
      break;
    default:
      // Items built from a list carry what they act on after a prefix.
      if (id.startsWith("theme:")) applyTheme(id.slice("theme:".length));
      else if (id.startsWith("detach:")) toggleDetached(id.slice("detach:".length));
      else if (id.startsWith("alternative:")) switchAlternative(id.slice("alternative:".length));
      else if (id.startsWith("revert:")) revertToBackup(id.slice("revert:".length));
      else if (id.startsWith("repeat:")) repeatRegions(Number(id.slice("repeat:".length)));
  }
}
