// The Tracks area's frame: its local menu bar (Edit, Functions, View), snap, zoom and height controls,
// around the ruler, headers and canvas. tracks/ holds the Tracks area: the canvas, ruler, headers,
// regions, takes, global tracks, markers, automation, tools and overview, and the track and region
// commands.
import { createStack, deleteTracks, deleteUnusedTracks, duplicateTracks, groupTracks, hideTracks, selectedTracks, toggleShowHidden, unhideAllTracks } from "./trackCommands";
import { convertLoops, copyRegions, cutRegions, deleteRegions, duplicate, joinRegions, pasteRegions, repeatRegions, selectAllRegions, setLocatorsByRegions, splitAtPlayhead, toggleLoop } from "./regionCommands";
import { Icon } from "../ui/icons";
import { getState, setState, useStore, type Snap, type State } from "../state/store";
import { SNAP_LABELS } from "./ruler/rulerMenu";
import { MAX_TRACK_H, MIN_TRACK_H, zoomTracks } from "./trackLayout";
import { RULER_H } from "./viewport";
import { Splitter } from "../app/Splitter";
import { Arrange } from "./canvas/TracksCanvas";
import { openMenuAt, type MenuItem } from "../ui/Menu";
import { Ruler } from "./ruler/Ruler";
import { TrackHeaders } from "./headers/TrackHeaders";
import { importAudioDialog, undoRedo } from "../project/files";
import { isLooped } from "./regions/loops";
import { exportSelectionMidi, importMidiDialog } from "../project/midiFiles";
import { fmt, keyHint, tip } from "../keyboard/keymap";
import { transport } from "../app/controlBar/transport";
import { GlobalTracks, globalHeaderMenu, globalTrackItems, globalTracksHeight } from "./globalTracks/GlobalTracks";
import { Overview, toggleOverview } from "./overview/Overview";
import { OVERVIEW_H } from "./overview/overviewModel";
import { tracksAreaMin } from "../app/paneSizes";
import { act } from "../engine/commands";
import type { DragMode } from "./regions/fades";
import { ToolPopups } from "./tools/ToolPopups";
import { marqueeCopy, marqueeCut, marqueeDelete, marqueePaste, toolMenuItems } from "./tools/trackTools";
import { useBarFit } from "../ui/useBarFit";
import { showUsedAutomation } from "./automation/menus";
import { zeroCrossingsItem } from "./regions/zeroCrossings";
import { arrangeItems } from "./arrangeCommands";
import { EDIT_MENU, layMenu, menuLabel } from "../app/menuBar/menuLabels";
import { packTakeFolder } from "./takes/takes";
import { releaseFieldFocus } from "../keyboard/focusRing";
import { onPress } from "../keyboard/keyNav";
import { stepZoom } from "./zoom";

/** Logic's Drag pop-up; only Overlap and X-Fade so far. */
/** In the order of Logic's Drag pop-up. */
const DRAG_LABELS: Record<DragMode, string> = { overlap: "Overlap", no_overlap: "No Overlap", xfade: "X-Fade", shuffle_r: "Shuffle R", shuffle_l: "Shuffle L" };
const DRAG_TIPS: Record<DragMode, string> = {
  overlap: "Overlap: regions keep their borders; the one moved last plays where they overlap",
  no_overlap: "No Overlap: a region that an edit overlaps is shortened where the next one starts",
  xfade: "X-Fade crossfades audio regions that a move, trim or paste leaves overlapping",
  shuffle_r: "Shuffle R: regions stay butted together, closing gaps towards the right",
  shuffle_l: "Shuffle L: regions stay butted together, closing gaps towards the left",
};

/** A submenu, greyed out when none of its items applies. */
const submenu = (label: string, children: MenuItem[]): MenuItem => ({ label, children, disabled: children.every((c) => c.disabled || c.separator) });

/** The Tracks area's Edit menu, the menu bar's Edit menu item for item (src/app/menuBar/menuLabels.ts
 *  EDIT_MENU); the menu bar follows its states (src/app/menuBar/menuBar.ts). */
export function editMenu(): MenuItem[] {
  const s = getState();
  const sel = s.selectedRegions;
  const marquee = s.marquee;
  const snap = s.snap;
  const regs = snap?.project.tracks.flatMap((t) => t.regions).filter((r) => sel.includes(r.id)) ?? [];
  const L = menuLabel;
  const own: MenuItem[] = [
    { id: "undo", label: snap?.undo ? `Undo ${snap.undo}` : L("undo"), hint: fmt("Mod+Z"), disabled: !snap?.undo, onClick: () => undoRedo("undo") },
    { id: "redo", label: snap?.redo ? `Redo ${snap.redo}` : L("redo"), hint: fmt("Mod+Shift+Z"), disabled: !snap?.redo, onClick: () => undoRedo("redo") },
    // With a marquee, Cut, Copy, Paste and Delete act on the part of the regions inside it.
    { id: "cut", label: L("cut"), hint: keyHint("cut"), disabled: !sel.length && !marquee, onClick: () => marqueeCut() || cutRegions() },
    { id: "copy", label: L("copy"), hint: keyHint("copy"), disabled: !sel.length && !marquee, onClick: () => marqueeCopy() || copyRegions() },
    { id: "paste", label: L("paste"), hint: keyHint("paste"), disabled: !s.clipboard.length, onClick: () => marqueePaste() || pasteRegions() },
    { id: "duplicate", label: L("duplicate"), hint: fmt("Mod+D"), onClick: duplicate },
    ...REPEAT_TIMES.map((n): MenuItem => ({ id: `repeat:${n}`, label: L(`repeat:${n}`), disabled: !sel.length, onClick: () => repeatRegions(n) })),
    { id: "split", label: L("split"), hint: fmt("Mod+T"), onClick: () => splitAtPlayhead() },
    { id: "join", label: L("join"), hint: fmt("Mod+J"), disabled: sel.length < 2, onClick: () => joinRegions() },
    { id: "loop", label: L("loop"), hint: keyHint("loop"), disabled: !sel.length, checked: regs.length > 0 && regs.every(isLooped), onClick: () => toggleLoop() },
    { id: "convert-loops", label: L("convert-loops"), disabled: !regs.some(isLooped), onClick: () => convertLoops() },
    { id: "pack-takes", label: L("pack-takes"), hint: keyHint("pack-takes"), disabled: sel.length < 2, onClick: () => packTakeFolder() },
    { id: "set-locators", label: L("set-locators"), hint: keyHint("set-locators"), disabled: !sel.length, onClick: () => setLocatorsByRegions() },
    { ...zeroCrossingsItem(), id: "zero-crossings" },
    { id: "select-all", label: L("select-all"), hint: fmt("Mod+A"), onClick: selectAllRegions },
    { id: "delete", label: L("delete"), hint: keyHint("delete-regions"), disabled: !sel.length && !marquee, onClick: () => marqueeDelete() || deleteRegions() },
  ];
  const byId = new Map([...own, ...arrangeItems()].map((it) => [it.id, it]));
  return layMenu<MenuItem>(EDIT_MENU, (id) => byId.get(id) ?? null, submenu, { separator: true });
}

const REPEAT_TIMES = [1, 2, 3, 4, 8, 16] as const;

/** The Tracks area's Functions menu: the track commands of the menu bar's Track menu, and
 *  importing and exporting. */
export function functionsMenu(): MenuItem[] {
  const s = getState();
  const L = menuLabel;
  const none = !selectedTracks().length;
  return [
    { id: "new-tracks", label: L("new-tracks"), hint: fmt("Mod+Alt+N"), onClick: () => setState({ dialog: "new-tracks" }) },
    { id: "duplicate-track", label: L("duplicate-track"), disabled: none, onClick: () => duplicateTracks(true) },
    { id: "same-settings", label: L("same-settings"), disabled: none, onClick: () => duplicateTracks(false) },
    { separator: true },
    { id: "folder-stack", label: L("folder-stack"), hint: fmt("Mod+Shift+D"), disabled: none, onClick: () => createStack(false) },
    { id: "summing-stack", label: L("summing-stack"), disabled: none, onClick: () => createStack(true) },
    { id: "group", label: L("group"), hint: fmt("Mod+G"), disabled: none, onClick: () => groupTracks() },
    { separator: true },
    { id: "import-audio", label: L("import-audio"), hint: fmt("Mod+Shift+I"), onClick: importAudioDialog },
    { id: "import-midi", label: L("import-midi"), onClick: importMidiDialog },
    { id: "export-midi", label: L("export-midi"), onClick: exportSelectionMidi },
    { separator: true },
    { id: "output-track", label: L("output-track"), checked: s.showOutput, onClick: () => setState({ showOutput: !getState().showOutput }) },
    { id: "hide-tracks", label: L("hide-tracks"), hint: keyHint("hide-tracks"), disabled: none, onClick: () => hideTracks() },
    { id: "unhide-all", label: L("unhide-all"), disabled: !s.snap?.project.tracks.some((t) => t.hidden), onClick: unhideAllTracks },
    { separator: true },
    { id: "delete-track", label: L("delete-track"), hint: fmt("Mod+Backspace"), disabled: none, onClick: () => deleteTracks() },
    { id: "delete-unused", label: L("delete-unused"), onClick: deleteUnusedTracks },
  ];
}

/** The Tracks area's View menu: the menu bar's View items for the main window's areas, in the
 *  same order, and the track heights. */
export function viewMenu(): MenuItem[] {
  const s = getState();
  const L = menuLabel;
  return [
    { id: "library", label: L("library"), hint: keyHint("library"), checked: s.showLibrary, onClick: () => setState({ showLibrary: !s.showLibrary }) },
    { id: "inspector", label: L("inspector"), hint: keyHint("inspector"), checked: s.showInspector, onClick: () => setState({ showInspector: !s.showInspector }) },
    { id: "mixer", label: L("mixer"), hint: keyHint("mixer"), checked: s.bottom === "mixer", onClick: () => setState({ bottom: s.bottom === "mixer" ? "none" : "mixer" }) },
    { id: "browser", label: L("browser"), hint: keyHint("browser"), checked: s.showBrowser, onClick: () => setState({ showBrowser: !s.showBrowser }) },
    { id: "automation", label: L("automation"), hint: keyHint("automation"), checked: s.showAutomation, onClick: () => setState({ showAutomation: !s.showAutomation }) },
    { id: "used-automation", label: L("used-automation"), hint: keyHint("used-automation"), onClick: showUsedAutomation },
    { id: "global-tracks", label: L("global-tracks"), hint: keyHint("global-tracks"), checked: s.showGlobalTracks, onClick: () => setState({ showGlobalTracks: !s.showGlobalTracks }) },
    // Which of them show, one click from anywhere (as Logic's Global Tracks configuration).
    { label: "Global Tracks", children: globalTrackItems() },
    { id: "overview", label: L("overview"), checked: s.showOverview, onClick: toggleOverview },
    { separator: true },
    { id: "show-hidden", label: L("show-hidden"), hint: keyHint("show-hidden"), checked: s.showHidden, onClick: toggleShowHidden },
    { separator: true },
    { id: "zoom-in", label: L("zoom-in"), hint: keyHint("zoom-in"), onClick: () => setState({ pxPerBeat: stepZoom(s.pxPerBeat, 1) }) },
    { id: "zoom-out", label: L("zoom-out"), hint: keyHint("zoom-out"), onClick: () => setState({ pxPerBeat: stepZoom(s.pxPerBeat, -1) }) },
    { separator: true },
    { label: "Taller Tracks", hint: keyHint("taller-tracks"), onClick: () => zoomTracks(1.2) },
    { label: "Shorter Tracks", hint: keyHint("shorter-tracks"), onClick: () => zoomTracks(1 / 1.2) },
    {
      label: "Track Height",
      children: [
        ["Mini", 24],
        ["Compact", 36],
        ["Small", 52],
        ["Medium", 64],
        ["Large", 96],
        ["Extra Large", 150],
      ].map(([label, h]) => ({
        label: label as string,
        checked: s.trackHeight === h && Object.keys(s.trackHeights).length === 0,
        onClick: () => setState({ trackHeight: h as number, trackHeights: {} }),
      })),
    },
  ];
}

/** The least height the Tracks area keeps beside the bottom section: its fixed rows (ruler,
 *  global tracks, overview) and a little of the track list. */
export const tracksAreaMinOf = (s: State) => tracksAreaMin(RULER_H + globalTracksHeight(s) + (s.showOverview ? OVERVIEW_H + 1 : 0));

/** What the local bar gives way with, in this order, when it's short of room (barFit.ts). The
 *  menus and the Left-click Tool never do; what leaves the bar is in the View menu. */
const BAR_STEPS = ["captions", "tight", "height", "scroll", "drag", "catch", "zoom", "cmd-tool", "snap"] as const;

export function TracksArea() {
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const catchPlayhead = useStore((s) => s.catchPlayhead);
  const snapMode = useStore((s) => s.snapMode);
  const trackHeight = useStore((s) => s.trackHeight);
  const headerW = useStore((s) => s.paneSizes.headers);
  const showHidden = useStore((s) => s.showHidden);
  const globalH = useStore(globalTracksHeight);
  const showOverview = useStore((s) => s.showOverview);
  const minHeight = useStore(tracksAreaMinOf);
  const dragMode = useStore((s) => s.snap?.project.drag_mode) ?? "overlap";
  // Menus read the rest of the state when they open.
  const s = { ...getState(), pxPerBeat, catchPlayhead, snapMode, trackHeight };
  const zoom = Math.log(s.pxPerBeat / 2) / Math.log(200);
  const fit = useBarFit(BAR_STEPS, `${snapMode}|${dragMode}`);
  const gone = fit.has;
  const snapItems = (): MenuItem[] => (Object.keys(SNAP_LABELS) as Snap[]).map((k) => ({ label: SNAP_LABELS[k], checked: s.snapMode === k, onClick: () => setState({ snapMode: k }) }));
  const dragItems = (): MenuItem[] => (Object.keys(DRAG_LABELS) as DragMode[]).map((k) => ({ label: DRAG_LABELS[k], checked: dragMode === k, onClick: () => act("set_drag_mode", { mode: k }) }));
  // The View menu with the controls that left the bar and aren't in it already (zoom and track
  // height are).
  const view = (): MenuItem[] => {
    const moved: MenuItem[] = [
      ...(gone("catch") ? [{ label: "Catch Playhead", hint: keyHint("catch-playhead"), checked: s.catchPlayhead, onClick: () => setState({ catchPlayhead: !s.catchPlayhead }) }] : []),
      ...(gone("scroll") ? [{ label: "Scroll to Playhead", hint: keyHint("show-playhead"), onClick: () => transport.showPlayhead() }] : []),
      ...(gone("cmd-tool") ? [{ label: `${fmt("Mod")}-click Tool`, children: toolMenuItems("cmd") }] : []),
      ...(gone("snap") ? [{ label: "Snap", children: snapItems() }] : []),
      ...(gone("drag") ? [{ label: "Drag", children: dragItems() }] : []),
    ];
    return moved.length ? [...viewMenu(), { separator: true }, ...moved] : viewMenu();
  };

  return (
    <div className="tracks-area" style={{ minHeight }}>
      <div ref={fit.ref} className={"local-bar fit-bar tracks-bar" + fit.className}>
        <div className="local-menus">
          <button className="local-menu" {...onPress((e) => openMenuAt(e.currentTarget, editMenu()))}>
            Edit
          </button>
          <button className="local-menu" {...onPress((e) => openMenuAt(e.currentTarget, functionsMenu()))}>
            Functions
          </button>
          <button className="local-menu" {...onPress((e) => openMenuAt(e.currentTarget, view()))}>
            View
          </button>
        </div>
        <div className="local-right">
          {!gone("catch") && (
            <button
              className={"lb-btn catch" + (s.catchPlayhead ? " on" : "")}
              aria-pressed={s.catchPlayhead}
              title={tip("Catch Playhead", "catch-playhead")}
              aria-label={tip("Catch Playhead", "catch-playhead")}
              onClick={() => setState({ catchPlayhead: !s.catchPlayhead })}
            >
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
                <path d="M3 2v10M3 7h8M8 4l3 3-3 3" />
              </svg>
            </button>
          )}
          {!gone("scroll") && (
            <button className="lb-btn" title={"Scroll to Playhead" + (keyHint("show-playhead") ? ` (${keyHint("show-playhead")})` : "")} aria-label={"Scroll to Playhead" + (keyHint("show-playhead") ? ` (${keyHint("show-playhead")})` : "")} onClick={() => transport.showPlayhead()}>
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
                <path d="M7 1v12M2 4h3M2 10h3M9 4h3M9 10h3" />
              </svg>
            </button>
          )}
          <ToolPopups cmd={!gone("cmd-tool")} />
          {!gone("snap") && (
            <>
              <span className="lb-label">Snap:</span>
              <button className="popup" {...onPress((e) => openMenuAt(e.currentTarget, snapItems()))}>
                {SNAP_LABELS[s.snapMode]} <Icon.chevron />
              </button>
            </>
          )}
          {!gone("drag") && (
            <>
              <span className="lb-label">Drag:</span>
              <button className="popup" title={DRAG_TIPS[dragMode]} {...onPress((e) => openMenuAt(e.currentTarget, dragItems()))}>
                {DRAG_LABELS[dragMode]} <Icon.chevron />
              </button>
            </>
          )}
          {!gone("height") && (
            <>
              <span className="lb-label" title="Track height">
                Height
              </span>
              <input
                className="zoom vzoom"
                type="range"
                min={MIN_TRACK_H}
                max={MAX_TRACK_H}
                step={1}
                value={s.trackHeight}
                title={`Track height (${fmt("Alt")}-scroll, ${keyHint("shorter-tracks")} / ${keyHint("taller-tracks")})`}
                onChange={(e) => setState({ trackHeight: parseInt(e.target.value), trackHeights: {} })}
              />
            </>
          )}
          {!gone("zoom") && (
            <>
              <span className="lb-label" title="Horizontal zoom">
                Zoom
              </span>
              <input
                className="zoom"
                type="range"
                min={0}
                max={1}
                step={0.001}
                value={zoom}
                title={`Horizontal zoom (${keyHint("zoom-out")} / ${keyHint("zoom-in")})`}
                onChange={(e) => setState({ pxPerBeat: 2 * Math.pow(200, parseFloat(e.target.value)) })}
              />
            </>
          )}
        </div>
      </div>
      <div
        className={"tracks-body" + (showOverview ? " with-overview" : "")}
        // A press anywhere here, global tracks and ruler included, ends typing in a text field.
        onMouseDownCapture={(e) => releaseFieldFocus(e.target)}
        style={{
          gridTemplateColumns: `${headerW}px 1fr`,
          gridTemplateRows: (globalH ? `${RULER_H}px ${globalH}px 1fr` : `${RULER_H}px 1fr`) + (showOverview ? ` ${OVERVIEW_H + 1}px` : ""),
        }}
      >
        <div className="headers-top">
          <button className="add-track" title={`New Tracks (${fmt("Mod+Alt+N")})`} aria-label={`New Tracks (${fmt("Mod+Alt+N")})`} onClick={() => setState({ dialog: "new-tracks" })}>
            <Icon.plus />
          </button>
          <button className={"hide-mode" + (showHidden ? " on" : "")} aria-pressed={showHidden} title={tip("Show Hidden Tracks", "show-hidden")} aria-label={tip("Show Hidden Tracks", "show-hidden")} onClick={toggleShowHidden}>
            H
          </button>
          <span className="global-label" onContextMenu={globalHeaderMenu}>
            Marker
          </span>
        </div>
        <Ruler />
        {globalH > 0 && <GlobalTracks />}
        <div className="headers-cell">
          <TrackHeaders />
          <Splitter pane="headers" dir="x" sign={1} />
        </div>
        <Arrange />
        {showOverview && <div className="overview-corner" />}
        {showOverview && <Overview />}
      </div>
    </div>
  );
}
