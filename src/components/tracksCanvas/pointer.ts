// The Tracks area's canvas under the pointer: what a press hits, dragging regions (move, copy, trim,
// fade, loop), the marquee, take swipes, the context menus and the hover cursor. Built on each render,
// with window listeners only while a drag is on.
import { act, actMade, type Id, type Region } from "../../api";
import { getState, setState } from "../../store";
import { waveCache } from "../../waveform";
import { PEAK_BUCKET, snapPos } from "../../time";
import { tempoMap } from "../../tempo";
import { indexAt, visibleSteps } from "../../layout";
import { viewport } from "../../view";
import { openMenu } from "../Menu";
import { laneMenu, linkedRegions, regionMenu, renameRegion } from "../../ops";
import { isLooped, regionEnd } from "../../loops";
import { automationHover, automationMenu, automationMouseDown } from "../../automation";
import { refuseFrozen } from "../../freeze";
import { getSettings } from "../../prefs";
import { draggedFadeOut } from "../../fades";
import { isMidiFolder } from "../../comps";
import { FOLDER_BADGE_W, isFolder, isOpen, swipeComp, takeAtRow, takeFolderMenu, takeShift, toggleFolder } from "../../takes";
import { toggleOverview } from "../Overview";
import { regionBar } from "../../regionBar";
import { moveAutomationAnswer } from "../../moveAutomation";
import { movedStart, placedAt } from "../../musicalMove";
import { lanesPast, splitDrop } from "../../newTrackDrag";
import { onGainHandle, startGainDrag } from "../../regionGainDrag";
import { activeTool, notePointer, pressOnMarquee, toolHover, toolMouseDown, type ToolView } from "../../trackTools";
import { regionCrossfades, setCrossfadeDrag } from "../../regionDraw";
import { grabKind as regionGrabKind } from "../../regionGrab";
import { dragNewLanes, dragTarget, type TracksRefs } from "./drag";

/** Logic's loop tool pointer: a circular arrow. */
const LOOP_CURSOR =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='18' height='18' viewBox='0 0 18 18'%3E%3Cg fill='none' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M14 9a5 5 0 1 1-1.5-3.6' stroke='%23000' stroke-width='3.4'/%3E%3Cpath d='M14 9a5 5 0 1 1-1.5-3.6' stroke='%23fff' stroke-width='1.6'/%3E%3Cpath d='M13.5 2.5v3.2h-3.2' stroke='%23000' stroke-width='3.4'/%3E%3Cpath d='M13.5 2.5v3.2h-3.2' stroke='%23fff' stroke-width='1.6'/%3E%3C/g%3E%3C/svg%3E\") 9 9, ew-resize";

type Refs = TracksRefs & {
  scrollRef: React.RefObject<HTMLDivElement | null>;
  requestDraw: () => void;
  /** This render's value: whether presses go to the automation lanes instead of the regions. */
  showAutomation: boolean;
};

export function tracksPointer({ scrollRef, live, dragRef, swipeRef, marqueeRef, hoverRef, requestDraw, showAutomation }: Refs) {
  const hit = (e: React.MouseEvent) => {
    const el = scrollRef.current!;
    const rect = el.getBoundingClientRect();
    const x = e.clientX - rect.left + el.scrollLeft;
    const y = e.clientY - rect.top + el.scrollTop;
    const { project, pps, layout } = live.current;
    if (!project) return null;
    const ti = y < layout.total ? indexAt(layout, y) : -1;
    const track = ti >= 0 ? project.tracks[ti] : undefined;
    let region: Region | undefined;
    let inLoop = false;
    // Below the lane, in an open take folder's rows: the folder and the take row.
    const laneH = ti >= 0 ? layout.heights[ti] - layout.takeRows[ti] : 0;
    let row: { region: Region & { takes: NonNullable<Region["takes"]> }; take: NonNullable<Region["takes"]>["takes"][number] } | null = null;
    if (track && layout.takeRows[ti] && y - layout.tops[ti] >= laneH) {
      for (const r of track.regions) {
        if (!isFolder(r) || !isOpen(r.id) || x < r.start * pps || x > (r.start + r.length) * pps) continue;
        const take = takeAtRow(r, y - layout.tops[ti], laneH);
        if (take) row = { region: r, take };
      }
    } else if (track) {
      // Topmost (last drawn) region wins; region bodies are drawn over loop repetitions.
      for (let k = track.regions.length - 1; k >= 0 && !region; k--) {
        const r = track.regions[k];
        const rx = r.start * pps;
        if (x >= rx && x <= rx + Math.max(2, r.length * pps)) region = r;
      }
      for (let k = track.regions.length - 1; k >= 0 && !region; k--) {
        const r = track.regions[k];
        if (isLooped(r) && x > (r.start + r.length) * pps && x <= regionEnd(r) * pps) {
          region = r;
          inLoop = true;
        }
      }
    }
    const localY = ti >= 0 ? y - layout.tops[ti] : 0;
    return { x, y, ti, track, region, inLoop, localY, laneH, sample: x / pps, row };
  };

  /** What a press at `h` on a region grabs (see regionGrab.ts). */
  const grabKind = (h: NonNullable<ReturnType<typeof hit>>) => regionGrabKind(h, live.current.pps, regionCrossfades);

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
    if (h.row) return startSwipe(e, h.row.region, h.row.take.id);
    if (!h.region) return startMarquee(e, h);
    const r = h.region;
    // A take folder's triangle opens it; its badge opens the take folder menu.
    if (isFolder(r) && h.localY <= regionBar(h.laneH - 3).bar && !h.inLoop) {
      const bx = Math.max(r.start * pps, scrollRef.current!.scrollLeft);
      if (h.x < bx + 17) {
        toggleFolder(r.id);
        return;
      }
      if (h.x < bx + FOLDER_BADGE_W) {
        setState({ selectedRegions: [r.id], focus: "regions" });
        openMenu(e.clientX, e.clientY, takeFolderMenu(r));
        return;
      }
    }
    let sel = getState().selectedRegions;
    // Shift-clicking a selected region only takes it out of the selection; nothing is dragged.
    const deselect = e.shiftKey && sel.includes(r.id);
    if (e.shiftKey) {
      sel = deselect ? sel.filter((i) => i !== r.id) : [...sel, ...linkedRegions(r.id)];
    } else if (!sel.includes(r.id)) {
      sel = linkedRegions(r.id);
    }
    setState({ selectedRegions: sel, focus: "regions" });
    if (deselect) return;
    // A frozen track's regions can be selected but not moved, trimmed or faded.
    if (h.track.frozen) {
      refuseFrozen([h.track.id]);
      return;
    }

    if (!h.inLoop && onGainHandle(r, h.x, h.localY, h.laneH, pps, scrollRef.current!.scrollLeft, viewport().width)) return startGainDrag(e, project, r, sel, requestDraw);
    const kind = grabKind(h);
    const startX = e.clientX;
    const startY = e.clientY;
    const startContentY = h.y;
    const startScrollTop = scrollRef.current!.scrollTop;
    const startIndex = h.ti;
    let moved = false;
    dragRef.current = { kind, ids: kind === "move" ? sel : [r.id], dx: 0, dTrack: 0, grabbed: r.id };
    const fadeFrom = r.content.type === "audio" ? (kind === "fadeIn" ? r.content.fade_in : r.content.fade_out) : 0;
    // Dragging a crossfade's handle resizes it about its centre.
    const crossfading = kind === "fadeOut" && !!regionCrossfades(r.id)?.out;
    const snap = (v: number) => snapPos(project, v, getState().snapMode, getState().pxPerBeat);
    // An audio region can't be trimmed out past the ends of its file.
    const audio = r.content.type === "audio" ? r.content : null;
    const wave = audio ? waveCache.get(audio.clip) : undefined;
    const minStart = audio ? r.start - audio.offset : 0;
    // A take folder reaches as far as its takes do.
    const maxLength = audio && r.takes ? Math.max(0, ...r.takes.takes.map((t) => t.end)) - audio.offset : audio && wave ? wave.frames - audio.offset : Infinity;

    const move = (ev: MouseEvent) => {
      const dxPx = ev.clientX - startX;
      const dyPx = ev.clientY - startY;
      if (!moved && Math.abs(dxPx) < 3 && Math.abs(dyPx) < 3) return;
      moved = true;
      const d = dragRef.current!;
      d.dx = dxPx / pps;
      // Scrolling with the wheel mid-drag counts, so a long project's empty area can be reached.
      const y = startContentY + dyPx + scrollRef.current!.scrollTop - startScrollTop;
      const past = kind === "move" ? lanesPast(y, live.current.layout.total, getState().trackHeight) : 0;
      d.dTrack = visibleSteps(live.current.layout, startIndex, indexAt(live.current.layout, y)) + past;
      d.copy = kind === "move" && ev.altKey;
      if (kind === "fadeIn" || kind === "fadeOut") {
        const other = r.content.type === "audio" ? (kind === "fadeIn" ? r.content.fade_out : r.content.fade_in) : 0;
        const v = fadeFrom + (kind === "fadeIn" ? d.dx : -d.dx);
        d.fade = crossfading ? draggedFadeOut(fadeFrom, d.dx, true, r.length - other) : Math.round(Math.max(0, Math.min(r.length - other, v)));
        if (crossfading) setCrossfadeDrag({ region: r.id, length: d.fade });
      } else if (kind === "right") {
        d.start = r.start;
        d.length = Math.min(maxLength, Math.max(PEAK_BUCKET, snap(r.start + r.length + d.dx) - r.start));
      } else if (kind === "left") {
        const ns = Math.min(r.start + r.length - PEAK_BUCKET, Math.max(0, minStart, snap(r.start + d.dx)));
        d.start = ns;
        d.length = r.length - (ns - r.start);
      } else if (kind === "loop") {
        // Dragging back onto the region's own end turns the loop off.
        d.loopEnd = Math.max(r.start + r.length, snap(regionEnd(r) + d.dx));
      }
      requestDraw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const d = dragRef.current!;
      dragRef.current = null;
      setCrossfadeDrag(null);
      if (!moved) {
        // Double-clicking a pattern region opens the Step Sequencer.
        if (e.detail === 2 && r.pattern) setState({ bottom: "steps" });
        clickLocate(e, true);
        requestDraw();
        return;
      }
      if (kind === "move") {
        const newStart = Math.max(0, snap(r.start + d.dx));
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
        // Past the last track: new tracks for the lanes that fell off the end, in the one edit.
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
      } else if (d.loopEnd !== undefined) {
        const end = Math.round(d.loopEnd);
        act("set_region_loop", { id: r.id, loopEnd: end > r.start + r.length ? end : null });
      } else if (d.fade !== undefined) {
        act("set_region_fades", { ids: [r.id], fadeIn: kind === "fadeIn" ? d.fade : null, fadeOut: kind === "fadeOut" ? d.fade : null });
      } else if (d.start !== undefined && d.length !== undefined) {
        act("resize_region", { id: r.id, start: Math.round(d.start), length: Math.round(d.length) });
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  /** A click without a drag moves the playhead to it, snapped like a region drag would be
   *  (Settings › Editing › Click Moves Playhead). */
  const clickLocate = (e: React.MouseEvent, onRegion: boolean) => {
    const mode = getSettings().ui.click_locates ?? "anywhere";
    if (mode === "off" || (onRegion && mode === "empty")) return;
    const { project, pps } = live.current;
    if (!project) return;
    const el = scrollRef.current!;
    const x = e.clientX - el.getBoundingClientRect().left + el.scrollLeft;
    act("locate", { pos: Math.round(Math.max(0, snapPos(project, x / pps, getState().snapMode, getState().pxPerBeat))) });
  };

  /** Rubber-band selection, from empty lane space: the regions the marquee touches are selected
   *  (Shift adds them to the selection). A click without a drag deselects, as before. */
  const startMarquee = (e: React.MouseEvent, h: NonNullable<ReturnType<typeof hit>>) => {
    const el = scrollRef.current!;
    const base = e.shiftKey ? getState().selectedRegions : [];
    const startX = e.clientX;
    const startY = e.clientY;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 3 && Math.abs(ev.clientY - startY) < 3) return;
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
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      marqueeRef.current = null;
      if (!moved && !e.shiftKey) setState({ selectedRegions: [] });
      if (!moved) clickLocate(e, false);
      requestDraw();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  /** Quick Swipe Comping: dragging across a take row puts that stretch in the comp; a click
   *  without a drag makes the whole take the one heard. */
  const startSwipe = (e: React.MouseEvent, r: Region & { takes: NonNullable<Region["takes"]> }, take: Id) => {
    const { project, pps } = live.current;
    if (!project) return;
    setState({ selectedRegions: [r.id], focus: "regions" });
    // A MIDI take folder plays whole takes: clicking a take chooses it.
    if (isMidiFolder(r)) return act("select_take", { region: r.id, take });
    const el = scrollRef.current!;
    const left = el.getBoundingClientRect().left - el.scrollLeft;
    const snap = (v: number) => Math.min(r.start + r.length, Math.max(r.start, snapPos(project, v, getState().snapMode, getState().pxPerBeat)));
    const toSample = (cx: number) => (cx - left) / pps;
    const from = snap(toSample(e.clientX));
    const startX = e.clientX;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 3) return;
      moved = true;
      const to = snap(toSample(ev.clientX));
      const shift = takeShift(r);
      swipeRef.current = { region: r.id, take, start: Math.min(from, to) + shift, end: Math.max(from, to) + shift };
      requestDraw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const sw = swipeRef.current;
      swipeRef.current = null;
      if (!moved) act("select_take", { region: r.id, take });
      else if (sw && sw.end > sw.start) swipeComp(r, take, sw.start - takeShift(r), sw.end - takeShift(r));
      else requestDraw();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
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
    if (h.row) return openMenu(e.clientX, e.clientY, takeFolderMenu(h.row.region));
    if (!h.region) {
      const pos = Math.max(0, Math.round(snapPos(project, h.sample, getState().snapMode, getState().pxPerBeat)));
      if (h.track && h.track.id !== getState().snap?.selected) act("select_track", { id: h.track.id });
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
      el.style.cursor = "default";
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
    if (!h.inLoop && onGainHandle(h.region, h.x, h.localY, h.laneH, pps, el.scrollLeft, viewport().width)) {
      el.style.cursor = "ns-resize";
      return;
    }
    const kind = grabKind(h);
    el.style.cursor = kind === "loop" ? LOOP_CURSOR : kind === "left" || kind === "right" ? "ew-resize" : kind === "move" ? "default" : "col-resize";
  };

  return { onMouseDown, onHover, onContextMenu };
}
