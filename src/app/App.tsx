// The main window: the control bar, the side panes, the Tracks area with the bottom section under
// it, the status line, and the sheets and floating panels over them. At launch it loads the project
// and settings and wires up the engine's events, the keys (keyCommands.ts) and the menu bar
// (menuCommands.ts). app/ holds this shell: its commands, the control bar, pane sizes, the native
// menu bar, the update check and the detached panel windows.
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { type Snapshot, type Tick } from "../engine/snapshot";
import { type InputLevel } from "../engine/devices";
import { type PluginDesc } from "../engine/listings";
import { act, call } from "../engine/commands";
import { ControlBar } from "./controlBar/ControlBar";
import { NewTrackDialog } from "../tracks/NewTrackDialog";
import { GroupSheet } from "../tracks/GroupSheet";
import { SettingsDialog } from "../settings/Settings";
import { BounceDialog, BounceInPlaceDialog } from "../project/bounce/BounceDialogs";
import { ProjectChooser } from "../project/ProjectChooser";
import { ProjectSettingsSheet } from "../settings/panes/ProjectSettings";
import { RecoverySheet } from "../project/RecoverySheet";
import { installPluginKeys } from "../keyboard/pluginKeys";
import { loadSettings } from "../settings/appSettings";
import { offerRecoveryFor, runStartup } from "../project/startup";
import { restoreAppearance, useAppearancePersistence } from "../theme/appearance";
import { Editor } from "../editors/EditorPane";
import { EventList } from "../editors/eventList/EventList";
import { StepSequencer } from "../editors/stepSequencer/StepSequencer";
import { FxWindows } from "../plugins/panels/FxWindow";
import { FxDock } from "../plugins/panels/FxDock";
import { loadFxList } from "../plugins/fxHost";
import { MusicalTyping } from "../editors/noteInput/MusicalTyping";
import { runStepInputCommand, StepInput } from "../editors/noteInput/StepInput";
import { MenuHost } from "../ui/Menu";
import { loadPaneSizes, paneVars, Splitter } from "./Splitter";
import { Mixer } from "../mixer/Mixer";
import { Inspector } from "../inspector/Inspector";
import { Library } from "../browser/Library";
import { Browser } from "../browser/Browser";
import { TracksArea, tracksAreaMinOf } from "../tracks/TracksArea";
import { bottomMax, bottomPaneSize } from "./paneSizes";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirmDiscard } from "../project/files";
import { getState, pushTick, setState, showToast, useStore } from "../state/store";
import { loadGlobalView, loadHeights, loadMixerView, saveHeights, saveMixerView } from "../tracks/trackLayout";
import { installNativeDrop } from "../project/fileDrop";
import { MarkerList } from "../tracks/markers/MarkerList";
import { installDetach, receiveSnapshot } from "./detach/detach";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ControllerAssignmentsSheet, LearnBanner } from "../controllers/ControllerAssignments";
import { useProjectMenus } from "../project/projectFiles";
import { NameSheetHost } from "../ui/NameSheet";
import { sheetBlocks } from "../keyboard/sheetKeys";
import { AlternativesSheet } from "../project/AlternativesSheet";
import { StripSilenceHost } from "../tracks/regions/StripSilence";
import { DrumReplaceHost } from "../tracks/drumReplace/DrumReplaceSheet";
import { installMenuBar } from "./menuBar/menuBar";
import { checkUpdatesAtLaunch } from "./updates/updates";
import { UpdateNotice } from "./updates/UpdateNotice";
import { runMidiCommand, onKey, KEY_ACTIONS } from "./keyCommands";
import { handleMenu } from "./menuCommands";
import { StatusLine } from "./statusLine/StatusLine";
import { Toast } from "./Toast";

/** Remembers track heights, the Mixer's strip widths and the global tracks shown, per project
 *  folder. */
function useHeightPersistence() {
  const path = useStore((s) => s.snap?.path);
  const base = useStore((s) => s.trackHeight);
  const heights = useStore((s) => s.trackHeights);
  const output = useStore((s) => s.showOutput);
  const mixerView = useStore((s) => s.mixerView);
  const [loaded, setLoaded] = useState<string | null>(null);
  useEffect(() => {
    if (path && path !== loaded) {
      loadHeights(path);
      loadMixerView(path);
      loadGlobalView(path);
      setLoaded(path);
    }
  }, [path, loaded]);
  useEffect(() => {
    if (path && path === loaded) saveHeights(path);
  }, [path, loaded, base, heights, output]);
  useEffect(() => {
    if (path && path === loaded) saveMixerView(path);
  }, [path, loaded, mixerView]);
}

/** At launch: the session's state and the settings, then what Settings › General says to open
 *  (or an offer to recover a command-line project's autosave). */
async function startUp() {
  const snap = await act("get_state");
  await loadSettings();
  restoreAppearance();
  checkUpdatesAtLaunch();
  // Launched without a project: recover lost work, or open the last project or the Project
  // Chooser, as Settings › General says.
  if (snap && !snap.saved && snap.project.tracks.length === 0) await runStartup();
  // A project from the command line: offer its autosave (a dirty one is this session's own).
  else if (snap?.saved && !snap.dirty) await offerRecoveryFor(snap.path);
  await warnIfInputTurnedDown();
}

/** Says so when the system mixer has the audio input muted or turned down, which is easy to miss
 *  and sounds like a recording problem. */
async function warnIfInputTurnedDown() {
  const l = await call<InputLevel | null>("input_level");
  if (!l?.attenuating) return;
  const level = l.muted ? "muted" : `at ${Math.round(l.volume * 100)}%`;
  const gain = l.muted ? "silent" : `${Math.round(l.gain_db)} dB`;
  showToast(`${l.device} input is ${level} in the system mixer (${gain}). Fix it in Settings › Audio.`, 10000);
}

/** The main window's listeners: the engine's ticks and snapshots, the menu bar, the keys, file
 *  drops, the detached panel windows and closing the window. Returns what removes them. */
function installListeners(): () => void {
  const offs = [
    listen<Tick>("tick", (e) => {
      pushTick(e.payload);
      e.payload.commands?.forEach(runMidiCommand);
    }),
    listen<Snapshot>("project", (e) => receiveSnapshot(e.payload)),
    // On this window only: menu items for a focused panel window go there (windows.rs), and a
    // plain listen() would hear those too.
    getCurrentWebviewWindow().listen<string>("menu", (e) => handleMenu(e.payload)),
    // Keys a plug-in's editor window didn't use (pluginKeys.ts).
    installPluginKeys(),
    getCurrentWindow().onCloseRequested(async (e) => {
      if (!(await confirmDiscard("Quit"))) e.preventDefault();
    }),
  ];
  window.addEventListener("keydown", onKey);
  const offDrop = installNativeDrop();
  const offMenuBar = installMenuBar();
  // Detached panel windows: their key commands run here, as if pressed in this window.
  const offDetach = installDetach({
    key: (id, repeat) => {
      if (!getState().dialog && !sheetBlocks(id) && !runStepInputCommand(id, repeat)) KEY_ACTIONS[id]?.();
    },
  });
  const noContextMenu = (e: MouseEvent) => e.preventDefault();
  window.addEventListener("contextmenu", noContextMenu);
  return () => {
    offs.forEach((p) => p.then((f) => f()));
    window.removeEventListener("keydown", onKey);
    window.removeEventListener("contextmenu", noContextMenu);
    offDrop();
    offMenuBar();
    offDetach();
  };
}

export default function App() {
  // Only the layout-level flags: panes re-render themselves on the state they use.
  const s = {
    snap: useStore((s) => s.snap !== null),
    showLibrary: useStore((s) => s.showLibrary),
    showInspector: useStore((s) => s.showInspector),
    showBrowser: useStore((s) => s.showBrowser),
    bottom: useStore((s) => s.bottom),
    dialog: useStore((s) => s.dialog),
  };
  useHeightPersistence();
  useAppearancePersistence();
  useProjectMenus();
  const showTyping = useStore((s) => s.showTyping);
  const showStepInput = useStore((s) => s.showStepInput);
  const groupSheet = useStore((s) => s.groupSheet);
  const showMarkerList = useStore((s) => s.showMarkerList);
  const showControllerAssignments = useStore((s) => s.showControllerAssignments);
  useEffect(loadPaneSizes, []);
  // The bottom section's room: the centre column less what the Tracks area keeps.
  const centerRef = useRef<HTMLDivElement>(null);
  const bottomRoom = () => (centerRef.current ? bottomMax(centerRef.current.clientHeight, tracksAreaMinOf(getState())) : Infinity);
  const bottomSize = bottomPaneSize(s.bottom);
  useEffect(() => {
    void startUp();
    call<PluginDesc[]>("list_plugins").then((p) => p && setState({ plugins: p }));
    loadFxList();
    return installListeners();
  }, []);

  if (!s.snap) return <div className="loading">Starting audio engine…</div>;

  return (
    <div className="app">
      <ControlBar />
      <div className="main" ref={paneVars}>
        {s.showLibrary && <Library />}
        {s.showLibrary && <Splitter pane="library" dir="x" sign={1} />}
        {s.showInspector && <Inspector />}
        {s.showInspector && <Splitter pane="inspector" dir="x" sign={1} />}
        <div className="center" ref={centerRef}>
          <TracksArea />
          {bottomSize && <Splitter pane={bottomSize} dir="y" sign={-1} room={bottomRoom} />}
          {s.bottom === "mixer" && <Mixer />}
          {s.bottom === "editor" && <Editor />}
          {s.bottom === "list" && <EventList />}
          {s.bottom === "steps" && <StepSequencer />}
          {s.bottom === "fx" && <FxDock />}
        </div>
        {s.showBrowser && <Splitter pane="browser" dir="x" sign={-1} />}
        {s.showBrowser && <Browser />}
      </div>
      <StatusLine />
      {s.dialog === "new-tracks" && <NewTrackDialog />}
      {s.dialog === "settings" && <SettingsDialog />}
      {s.dialog === "chooser" && <ProjectChooser />}
      {s.dialog === "bounce" && <BounceDialog />}
      {s.dialog === "bounce-in-place" && <BounceInPlaceDialog />}
      {s.dialog === "project-settings" && <ProjectSettingsSheet />}
      {s.dialog === "recovery" && <RecoverySheet />}
      {s.dialog === "alternatives" && <AlternativesSheet />}
      {groupSheet !== null && <GroupSheet />}
      {showMarkerList && <MarkerList />}
      {showControllerAssignments && <ControllerAssignmentsSheet />}
      <LearnBanner />
      <FxWindows />
      {showTyping && <MusicalTyping />}
      {showStepInput && <StepInput />}
      <MenuHost />
      <NameSheetHost />
      <StripSilenceHost />
      <DrumReplaceHost />
      <UpdateNotice />
      <Toast />
    </div>
  );
}
