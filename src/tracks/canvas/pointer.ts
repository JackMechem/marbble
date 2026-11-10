// The Tracks area's canvas under the pointer: what a press hits, dragging regions (move, copy, trim,
// fade, loop), the marquee, take swipes, the context menus and the hover cursor. Built on each render,
// with window listeners only while a drag is on.
import { type Id, type Project, type Track } from "../../engine/project";
import { type Region, type Take, type TakeFolder } from "../../engine/regions";
import { act, actMade } from "../../engine/commands";
import { getState, setState } from "../../state/store";
import { waveCache } from "../regions/waveform";
import { PEAK_BUCKET } from "../../lib/time";
import { tempoMap } from "../../lib/tempo";
import { indexAt, visibleSteps } from "../trackLayout";
import { viewport } from "../viewport";
import { openMenu } from "../../ui/Menu";
import { keepRegionsOn } from "../trackCommands";
import { linkedRegions } from "../regionCommands";
import { laneMenu, regionMenu, renameRegion } from "../contextMenus";
import { isLooped, regionEnd } from "../regions/loops";
import { automationHover, automationMenu, automationMouseDown } from "../automation/canvas";
import { refuseFrozen } from "../freeze";
import { getSettings } from "../../settings/appSettings";
import { draggedFadeOut } from "../regions/fades";
import { edgeNear, folderBarHit, isMidiFolder, sectionAt } from "../takes/comps";
import { isFolder, isOpen, moveCompEdge, swipeComp, takeAtRow, takeShift, toggleFolder } from "../takes/takes";
import { takeFolderMenu } from "../takes/takeFolderMenu";
import { toggleOverview } from "../overview/Overview";
import { drawnBar, regionBar } from "../regions/regionBar";
import { moveAutomationAnswer } from "../regions/moveAutomation";
import { movedStart, placedAt } from "../regions/musicalMove";
import { lanesPast, splitDrop } from "../regions/newTrackDrag";
import { onGainHandle, startGainDrag } from "../regions/regionGainDrag";
import { activeTool, notePointer, pressOnMarquee, toolHover, toolMouseDown, type ToolHit, type ToolView } from "../tools/trackTools";
import { regionCrossfades, setCrossfadeDrag } from "../regions/regionDraw";
import { grabKind as regionGrabKind, type GrabKind, type RegionPress } from "../regions/regionGrab";
import { editorPaneFor } from "../regions/regionEditor";
import { trimAll, trimDelta, trimLimits, type TrimEdge } from "../regions/regionTrim";
import { snapToTracksGrid } from "../gridSnap";
import { dragWith, pastDragThreshold } from "../../ui/pointerDrag";
import { dragNewLanes, dragTarget, type Drag, type TracksRefs } from "./drag";

/** Logic's loop tool pointer: a circular arrow. */
const LOOP_CURSOR =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='18' height='18' viewBox='0 0 18 18'%3E%3Cg fill='none' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M14 9a5 5 0 1 1-1.5-3.6' stroke='%23000' stroke-width='3.4'/%3E%3Cpath d='M14 9a5 5 0 1 1-1.5-3.6' stroke='%23fff' stroke-width='1.6'/%3E%3Cpath d='M13.5 2.5v3.2h-3.2' stroke='%23000' stroke-width='3.4'/%3E%3Cpath d='M13.5 2.5v3.2h-3.2' stroke='%23fff' stroke-width='1.6'/%3E%3C/g%3E%3C/svg%3E\") 9 9, ew-resize";

/** How near (px) a comp section's edge a press in a take row grabs the edge instead of swiping. */
const EDGE_GRAB_PX = 4;
/** However short a region, this much of it (px) can be grabbed. */
const MIN_GRAB_W_PX = 2;

type FolderRegion = Region & { takes: TakeFolder };

/** What the pointer is over: content x and y, the track's index and its lane, the region there
 *  (or a loop repetition of it), the time under the pointer and, in an open take folder's rows
 *  below the lane, the folder and the take row. */
interface Hit extends ToolHit, RegionPress {
  row: { region: FolderRegion; take: Take } | null;
}

/** The region at content x on a track's lane: the topmost (last drawn) region body, else a loop
 *  repetition, since bodies are drawn over repetitions. */
function regionAt(track: Track, x: number, pps: number): { region?: Region; inLoop: boolean } {
  for (let k = track.regions.length - 1; k >= 0; k--) {
    const r = track.regions[k];
    const rx = r.start * pps;
    if (x >= rx && x <= rx + Math.max(MIN_GRAB_W_PX, r.length * pps)) return { region: r, inLoop: false };
  }
  for (let k = track.regions.length - 1; k >= 0; k--) {
    const r = track.regions[k];
    if (isLooped(r) && x > (r.start + r.length) * pps && x <= regionEnd(r) * pps) return { region: r, inLoop: true };
  }
  return { inLoop: false };
}

/** In the take rows under a track's lane, the open folder at content x and its take row at
 *  `localY`. */
function takeRowAt(track: Track, x: number, localY: number, laneH: number, pps: number): Hit["row"] {
  let row: Hit["row"] = null;
  for (const r of track.regions) {
    if (!isFolder(r) || !isOpen(r.id) || x < r.start * pps || x > (r.start + r.length) * pps) continue;
    const take = takeAtRow(r, localY, laneH);
    if (take) row = { region: r, take };
  }
  return row;
}

/** The regions a trim of `grabbed`'s edge changes: it and the other selected regions, except on
 *  frozen tracks, each with where it is and how far it can go. */
function trimmedRegions(project: Project, grabbed: Region, sel: Id[]) {
  return project.tracks.flatMap((t) =>
    t.frozen
      ? []
      : t.regions
          .filter((x) => x.id === grabbed.id || sel.includes(x.id))
          .map((x) => ({ id: x.id, span: { start: x.start, length: x.length }, lim: trimLimits(x, x.content.type === "audio" ? waveCache.get(x.content.clip)?.frames : undefined) })),
  );
}

type Refs = TracksRefs & {
  scrollRef: React.RefObject<HTMLDivElement | null>;
  requestDraw: () => void;
  /** This render's value: whether presses go to the automation lanes instead of the regions. */
  showAutomation: boolean;
};

/** The Tracks canvas's mouse handlers, built on each render from its refs. */
export function tracksPointer({ scrollRef, live, dragRef, swipeRef, marqueeRef, hoverRef, requestDraw, showAutomation }: Refs) {
  /** What the pointer is over, or null while no project is open. */
  const hit = (e: React.MouseEvent): Hit | null => {
    const el = scrollRef.current!;
    const rect = el.getBoundingClientRect();
    const x = e.clientX - rect.left + el.scrollLeft;
    const y = e.clientY - rect.top + el.scrollTop;
    const { project, pps, layout } = live.current;
    if (!project) return null;
    const ti = y < layout.total ? indexAt(layout, y) : -1;
    const track = ti >= 0 ? project.tracks[ti] : undefined;
    const laneH = ti >= 0 ? layout.heights[ti] - layout.takeRows[ti] : 0;
    const localY = ti >= 0 ? y - layout.tops[ti] : 0;
    const inTakeRows = !!track && layout.takeRows[ti] > 0 && localY >= laneH;
    const row = track && inTakeRows ? takeRowAt(track, x, localY, laneH, pps) : null;
    const { region, inLoop } = track && !inTakeRows ? regionAt(track, x, pps) : { region: undefined, inLoop: false };
    return { x, y, ti, track, region, inLoop, localY, laneH, sample: x / pps, row };
  };

  /** In an open folder's take row, the comp section edge under the pointer (take time), if any. */
  const rowEdge = (h: Hit) => {
    if (!h.row || isMidiFolder(h.row.region)) return null;
    const pps = live.current.pps;
    return edgeNear(h.row.region.takes, h.row.take.id, h.sample + takeShift(h.row.region), EDGE_GRAB_PX / pps);
  };

  /** What a press at `h` on a region grabs (see regionGrab.ts). */
  const grabKind = (h: Hit) => regionGrabKind(h, live.current.pps, regionCrossfades);

  /** On a take folder's name bar, whether `h` is on its triangle or its badge (see comps.ts).
   *  A short folder is its name bar alone; below where the bar would end, its trims win. */
  const folderHit = (h: Hit) => {
    const r = h.region;
    if (!r || !isFolder(r) || h.inLoop) return null;
    const g = regionBar(h.laneH - 3);
    if (g.barOnly && h.localY > g.bar + 1 && grabKind(h) !== "move") return null;
    const bx = Math.max(r.start * live.current.pps, scrollRef.current!.scrollLeft);
    return folderBarHit(h.x - bx, h.localY, drawnBar(g, h.laneH - 3));
  };

  /** What the tools (trackTools.ts) need to handle the pointer. */
  const toolView = (): ToolView => ({
    ...live.current,
    project: live.current.project!,
    el: scrollRef.current!,
    redraw: requestDraw,
    // The Fade tool shows the fade it will set as a fade handle drag would.
    previewFade: (p) => {
      dragRef.current = p ? { kind: p.kind, ids: [p.id], grabbed: p.id, dx: 0, dTrack: 0, fade: p.length } : null;
    },
    clickLocate,
  });

  /** What the automation view needs to handle the pointer. */
  const autoInput = () => ({ ...live.current, project: live.current.project!, el: scrollRef.current!, redraw: requestDraw });

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    // Regions can't be edited while automation is shown, as in Logic.
    if (showAutomation && live.current.project) return automationMouseDown(e, autoInput());
    const h = hit(e);
    if (!h) return;
    // The Tracks-area tools; the Pointer carries on below.
    const tool = activeTool(e);
    if (pressOnMarquee(tool, h) || toolMouseDown(tool, e, h, toolView())) return;
    if (!h.track) return startMarquee(e, h);
    const { project, pps } = live.current;
    if (!project) return;
    if (h.track.id !== getState().snap?.selected) act("select_track", { id: h.track.id });
    if (!h.row && getState().compSection) setState({ compSection: null });
    const edge = rowEdge(h);
    if (h.row && edge !== null) return startEdgeDrag(e, h.row.region, edge);
    if (h.row) return startSwipe(e, h.row.region, h.row.take.id);
    if (!h.region) return startMarquee(e, h);
    const r = h.region;
    // A take folder's triangle opens it; its badge opens the take folder menu.
    const onBar = folderHit(h);
    if (onBar === "triangle") return toggleFolder(r.id);
    if (onBar === "badge" && isFolder(r)) {
      setState({ selectedRegions: [r.id], focus: "regions" });
      openMenu(e.clientX, e.clientY, takeFolderMenu(r));
      return;
    }
    const sel = selectOnPress(e, r);
    if (!sel) return;
    // A frozen track's regions can be selected but not moved, trimmed or faded.
    if (h.track.frozen) {
      if (e.detail === 2) return openEditor(r);
      refuseFrozen([h.track.id]);
      return;
    }
    if (!h.inLoop && onGainHandle(r, h.x, h.localY, h.laneH, pps, scrollRef.current!.scrollLeft, viewport().width)) return startGainDrag(e, project, r, sel, requestDraw);
    startRegionDrag(e, h, project, r, sel);
  };

  /** Selects as a press on region `r` does: Shift adds it and its linked regions, or takes it out
   *  when it's selected; a plain press on an unselected region selects it (and those linked to
   *  it) alone. Returns the selection, or null when Shift took `r` out, which drags nothing. */
  const selectOnPress = (e: React.MouseEvent, r: Region): Id[] | null => {
    let sel = getState().selectedRegions;
    const deselect = e.shiftKey && sel.includes(r.id);
    if (e.shiftKey) {
      sel = deselect ? sel.filter((i) => i !== r.id) : [...sel, ...linkedRegions(r.id)];
    } else if (!sel.includes(r.id)) {
      sel = linkedRegions(r.id);
    }
    setState({ selectedRegions: sel, focus: "regions" });
    return deselect ? null : sel;
  };

  /** Dragging region `r` by what the press grabbed (grabKind): moving or copying the selection,
   *  trimming an edge, a fade or the loop. The drag shows in `dragRef` and is sent on release;
   *  a click without a drag opens the editor (double-click) or moves the playhead. */
  const startRegionDrag = (e: React.MouseEvent, h: Hit, project: Project, r: Region, sel: Id[]) => {
    const { pps } = live.current;
    const kind = grabKind(h);
    const startScrollTop = scrollRef.current!.scrollTop;
    // A trim moves the same edge of every selected region, as in Logic (frozen tracks' stay).
    const trimEdge: TrimEdge | null = kind === "left" || kind === "right" ? kind : null;
    const trimming = trimEdge ? trimmedRegions(project, r, sel) : [];
    dragRef.current = { kind, ids: kind === "move" ? sel : trimEdge ? trimming.map((x) => x.id) : [r.id], dx: 0, dTrack: 0, grabbed: r.id };
    const fadeFrom = r.content.type === "audio" ? (kind === "fadeIn" ? r.content.fade_in : r.content.fade_out) : 0;
    // Dragging a crossfade's handle resizes it about its centre.
    const crossfading = kind === "fadeOut" && !!regionCrossfades(r.id)?.out;
    const snap = (v: number) => snapToTracksGrid(project, v);
    let moved = false;

    const move = (ev: MouseEvent) => {
      const dxPx = ev.clientX - e.clientX;
      const dyPx = ev.clientY - e.clientY;
      if (!moved && !pastDragThreshold(dxPx, dyPx)) return;
      moved = true;
      const d = dragRef.current!;
      d.dx = dxPx / pps;
      // Scrolling with the wheel mid-drag counts, so a long project's empty area can be reached.
      const y = h.y + dyPx + scrollRef.current!.scrollTop - startScrollTop;
      const past = kind === "move" ? lanesPast(y, live.current.layout.total, getState().trackHeight) : 0;
      d.dTrack = visibleSteps(live.current.layout, h.ti, indexAt(live.current.layout, y)) + past;
      d.copy = kind === "move" && ev.altKey;
      if (kind === "fadeIn" || kind === "fadeOut") {
        const other = r.content.type === "audio" ? (kind === "fadeIn" ? r.content.fade_out : r.content.fade_in) : 0;
        const v = fadeFrom + (kind === "fadeIn" ? d.dx : -d.dx);
        d.fade = crossfading ? draggedFadeOut(fadeFrom, d.dx, true, r.length - other) : Math.round(Math.max(0, Math.min(r.length - other, v)));
        if (crossfading) setCrossfadeDrag({ region: r.id, length: d.fade });
      } else if (trimEdge) {
        // The dragged region's edge snaps; the others' move by as much, each within its own
        // limits (an audio region's file, a take folder's takes).
        d.spans = trimAll(trimming, trimEdge, trimDelta(r, trimEdge, d.dx, snap), PEAK_BUCKET);
        const own = d.spans.get(r.id) ?? r;
        d.start = own.start;
        d.length = own.length;
      } else if (kind === "loop") {
        // Dragging back onto the region's own end turns the loop off.
        d.loopEnd = Math.max(r.start + r.length, snap(regionEnd(r) + d.dx));
      }
      requestDraw();
    };
    const up = () => {
      const d = dragRef.current!;
      dragRef.current = null;
      setCrossfadeDrag(null);
      if (moved) return dropRegionDrag(project, r, kind, d);
      if (e.detail === 2) openEditor(r);
      clickLocate(e, true);
      requestDraw();
    };
    dragWith(move, up);
  };

  /** Sends the edit a region drag made, on release. */
  const dropRegionDrag = (project: Project, r: Region, kind: GrabKind, d: Drag) => {
    if (kind === "move") {
      dropMove(project, r, d);
    } else if (d.loopEnd !== undefined) {
      const end = Math.round(d.loopEnd);
      act("set_region_loop", { id: r.id, loopEnd: end > r.start + r.length ? end : null });
    } else if (d.fade !== undefined) {
      act("set_region_fades", { ids: [r.id], fadeIn: kind === "fadeIn" ? d.fade : null, fadeOut: kind === "fadeOut" ? d.fade : null });
    } else if (d.spans?.size) {
      // One undo step for them all.
      const resizes = [...d.spans].map(([id, x]) => [id, Math.round(x.start), Math.round(x.length)]);
      if (resizes.length === 1) act("resize_region", { id: resizes[0][0], start: resizes[0][1], length: resizes[0][2] });
      else act("resize_regions", { resizes });
    }
  };

  /** Moves the dragged regions (`r` the one grabbed) where the drag left them, or with Option
   *  copies them there; regions dragged past the last track go onto new tracks, in the same edit. */
  const dropMove = (project: Project, r: Region, d: Drag) => {
    const newStart = Math.max(0, snapToTracksGrid(project, r.start + d.dx));
    // With snapping on, the others keep their bars and beats across tempo changes.
    const musical = getState().snapMode !== "off";
    const tracks = project.tracks;
    const layout = live.current.layout;
    const moves = d.ids.flatMap((id) => {
      const ti = tracks.findIndex((t) => t.regions.some((x) => x.id === id));
      if (ti < 0) return [];
      const reg = tracks[ti].regions.find((x) => x.id === id)!;
      const target = tracks[dragTarget(layout, ti, d.dTrack, tracks)];
      return [{ reg, id, source: ti, start: movedStart(tempoMap(project), reg.start, r.start, newStart, musical), track: target.id }];
    });
    const { onTracks, onNew } = splitDrop(moves, dragNewLanes(layout, d, tracks));
    if (d.copy) {
      // Option-drag: copies at the drop position in one undo step; the copies get selected.
      const items = onTracks.map((m) => [m.track, placedAt(m.reg, m.start)]);
      const lanes = onNew.map((l) => [tracks[l.source].id, l.moves.map((m) => placedAt(m.reg, m.start))]);
      (lanes.length ? actMade("copy_regions_to_new_tracks", { items, lanes }) : actMade("paste_regions", { items, offset: 0, label: "Copy Regions" })).then((made) => made && setState({ selectedRegions: made }));
    } else {
      const plain = onTracks.map(({ id, start, track }) => ({ id, start, track }));
      const lanes = onNew.map((l) => [tracks[l.source].id, l.moves.map(({ id, start }) => ({ id, start }))]);
      // Regions going onto a new track leave their automation behind, so only the others ask.
      moveAutomationAnswer(project, plain).then((automation) => {
        if (lanes.length) actMade("move_regions_to_new_tracks", { moves: plain, lanes, automation });
        else act("move_regions", { moves: plain, automation });
      });
    }
  };

  /** A double-click on a region opens it in its editor: the Piano Roll, the Audio Track editor
   *  or the Step Sequencer (regionEditor.ts). */
  const openEditor = (r: Region) => setState({ bottom: editorPaneFor(r, getState().bottom) });

  /** A click without a drag moves the playhead to it, snapped like a region drag would be
   *  (Settings › Editing › Click Moves Playhead). */
  const clickLocate = (e: React.MouseEvent, onRegion: boolean) => {
    const mode = getSettings().ui.click_locates ?? "empty";
    if (mode === "off" || (onRegion && mode === "empty")) return;
    const { project, pps } = live.current;
    if (!project) return;
    const el = scrollRef.current!;
    const x = e.clientX - el.getBoundingClientRect().left + el.scrollLeft;
    act("locate", { pos: Math.round(Math.max(0, snapToTracksGrid(project, x / pps))) });
  };

  /** Rubber-band selection, from empty lane space: the regions the marquee touches are selected
   *  (Shift adds them to the selection). A click without a drag deselects, as before. */
  const startMarquee = (e: React.MouseEvent, h: Hit) => {
    const el = scrollRef.current!;
    const base = e.shiftKey ? getState().selectedRegions : [];
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && !pastDragThreshold(ev.clientX - e.clientX, ev.clientY - e.clientY)) return;
      moved = true;
      const rect = el.getBoundingClientRect();
      const x = ev.clientX - rect.left + el.scrollLeft;
      const y = ev.clientY - rect.top + el.scrollTop;
      const box = { left: Math.min(h.x, x), right: Math.max(h.x, x), top: Math.min(h.y, y), bottom: Math.max(h.y, y) };
      marqueeRef.current = box;
      const { project, pps, layout } = live.current;
      if (!project) return;
      const hits = new Set(base);
      project.tracks.forEach((t, i) => {
        // The lane itself: hidden and collapsed tracks have none, take rows don't count.
        const top = layout.tops[i];
        const lane = layout.heights[i] - layout.takeRows[i];
        if (lane <= 0 || top > box.bottom || top + lane < box.top) return;
        for (const r of t.regions) if (r.start * pps <= box.right && regionEnd(r) * pps >= box.left) hits.add(r.id);
      });
      setState({ selectedRegions: [...hits], focus: "regions" });
      requestDraw();
    };
    const up = () => {
      marqueeRef.current = null;
      if (!moved && !e.shiftKey) setState({ selectedRegions: [] });
      if (!moved) clickLocate(e, false);
      requestDraw();
    };
    dragWith(move, up);
  };

  /** Dragging a comp section's edge in a take row: a border between two sections moves both, an
   *  outer edge its own section (snapped, inside the folder). */
  const startEdgeDrag = (e: React.MouseEvent, r: FolderRegion, edge: number) => {
    const { project, pps } = live.current;
    if (!project) return;
    setState({ selectedRegions: [r.id], focus: "regions" });
    const el = scrollRef.current!;
    const left = el.getBoundingClientRect().left - el.scrollLeft;
    const snap = (v: number) => Math.min(r.start + r.length, Math.max(r.start, snapToTracksGrid(project, v)));
    const shift = takeShift(r);
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && !pastDragThreshold(ev.clientX - e.clientX)) return;
      moved = true;
      swipeRef.current = { region: r.id, edge, to: snap((ev.clientX - left) / pps) + shift };
      requestDraw();
    };
    const up = () => {
      const sw = swipeRef.current;
      swipeRef.current = null;
      if (moved && sw && "edge" in sw) moveCompEdge(r, edge - shift, sw.to - shift);
      else requestDraw();
    };
    dragWith(move, up);
  };

  /** Quick Swipe Comping: dragging across a take row puts that stretch in the comp and selects
   *  it; a click without a drag makes the whole take the one heard, or, on a part of the take
   *  already in the comp, selects that comp section (for the arrow keys and the row's menu). */
  const startSwipe = (e: React.MouseEvent, r: FolderRegion, take: Id) => {
    const { project, pps } = live.current;
    if (!project) return;
    setState({ selectedRegions: [r.id], focus: "regions" });
    // A MIDI take folder plays whole takes: clicking a take chooses it.
    if (isMidiFolder(r)) return act("select_take", { region: r.id, take });
    const el = scrollRef.current!;
    const left = el.getBoundingClientRect().left - el.scrollLeft;
    const snap = (v: number) => Math.min(r.start + r.length, Math.max(r.start, snapToTracksGrid(project, v)));
    const toSample = (cx: number) => (cx - left) / pps;
    const from = snap(toSample(e.clientX));
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && !pastDragThreshold(ev.clientX - e.clientX)) return;
      moved = true;
      const to = snap(toSample(ev.clientX));
      const shift = takeShift(r);
      swipeRef.current = { region: r.id, take, start: Math.min(from, to) + shift, end: Math.max(from, to) + shift };
      requestDraw();
    };
    const up = () => {
      const sw = swipeRef.current;
      swipeRef.current = null;
      const shift = takeShift(r);
      if (!moved) {
        const at = Math.round(toSample(e.clientX));
        if (sectionAt(r.takes, at + shift)?.take === take) return setState({ compSection: { region: r.id, at } });
        setState({ compSection: null });
        act("select_take", { region: r.id, take });
      } else if (sw && "take" in sw && sw.end > sw.start) {
        // The stretch the take has audio for is the new section.
        const t = r.takes.takes.find((k) => k.id === take);
        const a = Math.max(sw.start, t?.start ?? sw.start);
        const b = Math.min(sw.end, t?.end ?? sw.end);
        setState({ compSection: a < b ? { region: r.id, at: Math.floor((a + b) / 2) - shift } : null });
        swipeComp(r, take, sw.start - shift, sw.end - shift);
      } else requestDraw();
    };
    dragWith(move, up);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    // A right-click on the horizontal scroll bar (shown while the overview is hidden) offers
    // the overview back.
    const el = scrollRef.current!;
    if (!getState().showOverview && e.clientY >= el.getBoundingClientRect().top + el.clientHeight) {
      return openMenu(e.clientX, e.clientY, [{ label: "Show Overview", onClick: toggleOverview }]);
    }
    const { project } = live.current;
    if (showAutomation && project) return openMenu(e.clientX, e.clientY, automationMenu(e, autoInput()));
    const h = hit(e);
    if (!h || !project) return;
    if (h.row) return openMenu(e.clientX, e.clientY, takeFolderMenu(h.row.region, { take: h.row.take.id, at: Math.round(h.sample) }));
    if (!h.region) {
      const pos = Math.max(0, Math.round(snapToTracksGrid(project, h.sample)));
      if (h.track && h.track.id !== getState().snap?.selected) {
        keepRegionsOn([h.track.id]);
        act("select_track", { id: h.track.id });
      }
      openMenu(e.clientX, e.clientY, laneMenu(h.track ?? null, pos));
      return;
    }
    const r = h.region;
    if (!getState().selectedRegions.includes(r.id)) setState({ selectedRegions: linkedRegions(r.id), focus: "regions" });
    openMenu(
      e.clientX,
      e.clientY,
      regionMenu(r, () => renameRegion(r)),
    );
  };

  const onHover = (e: React.MouseEvent) => {
    notePointer({ x: e.clientX, y: e.clientY });
    if (dragRef.current) return;
    if (showAutomation && live.current.project) return automationHover(e, autoInput());
    const h = hit(e);
    const el = scrollRef.current!;
    if (live.current.project && toolHover(activeTool(e), h, { project: live.current.project, el, redraw: requestDraw })) return;
    if (!h?.region) {
      el.style.cursor = h && rowEdge(h) !== null ? "ew-resize" : "default";
      if (hoverRef.current !== null) {
        hoverRef.current = null;
        requestDraw();
      }
      return;
    }
    if (hoverRef.current !== h.region.id) {
      hoverRef.current = h.region.id;
      requestDraw();
    }
    const { pps } = live.current;
    if (folderHit(h)) {
      el.style.cursor = "pointer";
      return;
    }
    if (!h.inLoop && onGainHandle(h.region, h.x, h.localY, h.laneH, pps, el.scrollLeft, viewport().width)) {
      el.style.cursor = "ns-resize";
      return;
    }
    const kind = grabKind(h);
    el.style.cursor = kind === "loop" ? LOOP_CURSOR : kind === "left" || kind === "right" ? "ew-resize" : kind === "move" ? "default" : "col-resize";
  };

  return { onMouseDown, onHover, onContextMenu };
}
