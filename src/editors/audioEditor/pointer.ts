// The Audio Track Editor under the pointer: the ruler, moving, trimming and fading regions, dragging out a
// range, the hover cursor and the context menus. Built on each render, with window listeners only while
// a drag is on.
import { type Id } from "../../engine/project";
import { type Region } from "../../engine/regions";
import { act } from "../../engine/commands";
import * as ed from "./state";
import { laneMenu, regionMenu, renameRegion } from "../../tracks/contextMenus";
import { getState, setState } from "../../state/store";
import { PEAK_BUCKET, samplesPerBeat, snapPos } from "../../lib/time";
import { waveCache } from "../../tracks/regions/waveform";
import { regionCrossfades, setCrossfadeDrag } from "../../tracks/regions/regionDraw";
import { draggedFadeOut } from "../../tracks/regions/fades";
import { openMenu, type MenuItem } from "../../ui/Menu";
import { fmt } from "../../keyboard/keymap";
import { moveAutomationAnswer } from "../../tracks/regions/moveAutomation";
import { geometry, grabKind as grabOf, RULER, type Drag, type GrabKind } from "./model";
import type { AudioLive } from "./draw";

export type PointerRefs = {
  scrollRef: React.RefObject<HTMLDivElement | null>;
  dragRef: React.RefObject<Drag | null>;
  hoverRef: React.RefObject<Id | null>;
  live: React.RefObject<AudioLive>;
  requestDraw: () => void;
};

export function editorPointer({ scrollRef, dragRef, hoverRef, live, requestDraw }: PointerRefs) {
  const snapEd = (f: number, free = false) => {
    const { project, pps } = live.current;
    if (!project) return Math.max(0, Math.round(f));
    const mode = getState().snapMode;
    const pxPerBeat = samplesPerBeat(project) * pps;
    // Smart snap stops at sixteenths; zoomed in further, the waveform is edited freely.
    if (free || mode === "off" || (mode === "smart" && pxPerBeat / 4 > 80)) return Math.max(0, Math.round(f));
    return Math.max(0, Math.round(snapPos(project, f, mode, pxPerBeat)));
  };

  /** Presses on the scroll bar are left to it. */
  const onScrollbar = (e: { clientY: number }) => {
    const el = scrollRef.current!;
    return e.clientY - el.getBoundingClientRect().top >= el.clientHeight;
  };

  const frameAt = (clientX: number) => {
    const el = scrollRef.current!;
    return (clientX - el.getBoundingClientRect().left + el.scrollLeft) / live.current.pps;
  };

  const hit = (e: { clientX: number; clientY: number }) => {
    const el = scrollRef.current!;
    const rect = el.getBoundingClientRect();
    const { pps, track } = live.current;
    const x = e.clientX - rect.left + el.scrollLeft;
    const y = e.clientY - rect.top;
    const g = geometry(el.clientHeight);
    let region: Region | undefined;
    if (y >= g.top && y <= g.top + g.h) {
      // Topmost (last drawn) region wins.
      for (let k = track.regions.length - 1; k >= 0; k--) {
        const r = track.regions[k];
        const rx = r.start * pps;
        if (x >= rx && x <= rx + Math.max(2, r.length * pps)) {
          region = r;
          break;
        }
      }
    }
    return { x, y, g, frame: x / pps, region, inRuler: y < RULER };
  };

  /** What a press grabs (see editors/audioEditor/model.ts). */
  const grabKind = (h: ReturnType<typeof hit>): GrabKind => grabOf(h, live.current.pps, regionCrossfades);

  const drag = (onMove: (ev: MouseEvent) => void, onUp: (ev: MouseEvent) => void) => {
    const up = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", up);
      onUp(ev);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", up);
  };

  const autoScroll = (clientX: number) => {
    const el = scrollRef.current!;
    const r = el.getBoundingClientRect();
    if (clientX > r.right - 16) el.scrollLeft += Math.min(40, clientX - (r.right - 16));
    else if (clientX < r.left + 16) el.scrollLeft -= Math.min(40, r.left + 16 - clientX);
  };

  const selectRegion = (r: Region, shift: boolean) => {
    const cur = getState().selectedRegions;
    const next = shift ? (cur.includes(r.id) ? cur.filter((i) => i !== r.id) : [...cur, r.id]) : [r.id];
    setState({ selectedRegions: next, focus: "regions" });
  };

  const onMouseDown = (e: React.MouseEvent) => {
    ed.setEditorFocus(true);
    if (e.button !== 0 || onScrollbar(e)) return;
    const { project, track } = live.current;
    if (!project) return;
    if (track.id !== getState().snap?.selected) act("select_track", { id: track.id });
    const h = hit(e);

    if (h.inRuler) {
      // Click or scrub the ruler to locate.
      const locate = (cx: number, free: boolean) => act("locate", { pos: snapEd(frameAt(cx), free) });
      locate(e.clientX, e.altKey);
      drag(
        (ev) => locate(ev.clientX, ev.altKey),
        () => {},
      );
      return;
    }

    if (h.region && e.detail === 2) {
      // Double-click: select the whole region.
      ed.setEd({ sel: { track: track.id, start: h.region.start, end: h.region.start + h.region.length } });
      setState({ selectedRegions: [h.region.id], focus: "regions" });
      return;
    }

    const kind = grabKind(h);
    if (kind === "select") {
      beginSelect(e, h.region);
      return;
    }
    const r = h.region!;
    if (!getState().selectedRegions.includes(r.id) || e.shiftKey) selectRegion(r, e.shiftKey);
    const startX = e.clientX;
    const { pps } = live.current;
    let moved = false;
    dragRef.current = { kind, id: r.id };
    const audio = r.content.type === "audio" ? r.content : null;
    const wave = audio ? waveCache.get(audio.clip) : undefined;
    // Same limits as the arrange: trims stay within the file, fades can't overlap.
    const minStart = audio ? r.start - audio.offset : 0;
    const maxLength = audio && wave ? wave.frames - audio.offset : Infinity;
    const fadeFrom = audio ? (kind === "fadeIn" ? audio.fade_in : audio.fade_out) : 0;
    const crossfading = kind === "fadeOut" && !!regionCrossfades(r.id)?.out;
    drag(
      (ev) => {
        if (!moved && Math.abs(ev.clientX - startX) < 3) return;
        moved = true;
        const d = dragRef.current!;
        const dx = (ev.clientX - startX) / pps;
        if (kind === "move") d.start = snapEd(r.start + dx, ev.altKey);
        else if (kind === "fadeIn" || kind === "fadeOut") {
          const other = audio ? (kind === "fadeIn" ? audio.fade_out : audio.fade_in) : 0;
          d.fade = crossfading ? draggedFadeOut(fadeFrom, dx, true, r.length - other) : Math.round(Math.max(0, Math.min(r.length - other, fadeFrom + (kind === "fadeIn" ? dx : -dx))));
          if (crossfading) setCrossfadeDrag({ region: r.id, length: d.fade });
        } else if (kind === "right") {
          d.start = r.start;
          d.length = Math.min(maxLength, Math.max(PEAK_BUCKET, snapEd(r.start + r.length + dx, ev.altKey) - r.start));
        } else {
          const ns = Math.min(r.start + r.length - PEAK_BUCKET, Math.max(0, minStart, snapEd(r.start + dx, ev.altKey)));
          d.start = ns;
          d.length = r.length - (ns - r.start);
        }
        requestDraw();
      },
      () => {
        const d = dragRef.current!;
        dragRef.current = null;
        setCrossfadeDrag(null);
        requestDraw();
        if (!moved) return;
        if (kind === "move" && d.start !== undefined && d.start !== r.start) {
          const start = d.start;
          const project = getState().snap?.project;
          if (project) moveAutomationAnswer(project, [{ id: r.id, start }]).then((automation) => act("move_region", { id: r.id, start, track: null, automation }));
        }
        else if (d.fade !== undefined) act("set_region_fades", { ids: [r.id], fadeIn: kind === "fadeIn" ? d.fade : null, fadeOut: kind === "fadeOut" ? d.fade : null });
        else if (d.start !== undefined && d.length !== undefined) act("resize_region", { id: r.id, start: Math.round(d.start), length: Math.round(d.length) });
      },
    );
  };

  /** Drag across the waveform to select a range; Shift extends the current one. A click
   *  without dragging selects the region under it instead. */
  const beginSelect = (e: React.MouseEvent, under: Region | undefined) => {
    const { track } = live.current;
    const cur = ed.rangeSel();
    const at = frameAt(e.clientX);
    let anchor = snapEd(at, e.altKey);
    const extending = e.shiftKey && !!cur;
    if (extending) {
      anchor = Math.abs(at - cur.start) > Math.abs(at - cur.end) ? cur.start : cur.end;
      const s = snapEd(at, e.altKey);
      ed.setEd({ sel: { track: track.id, start: Math.min(anchor, s), end: Math.max(anchor, s) } });
    }
    const startX = e.clientX;
    let moved = extending;
    drag(
      (ev) => {
        if (!moved && Math.abs(ev.clientX - startX) < 3) return;
        moved = true;
        autoScroll(ev.clientX);
        const s = snapEd(frameAt(ev.clientX), ev.altKey);
        ed.setEd({ sel: s === anchor ? null : { track: track.id, start: Math.min(anchor, s), end: Math.max(anchor, s) } });
      },
      () => {
        if (moved) return;
        ed.clearSel();
        if (under) selectRegion(under, e.shiftKey);
        else setState({ selectedRegions: [] });
      },
    );
  };

  const onHover = (e: React.MouseEvent) => {
    if (dragRef.current) return;
    const el = scrollRef.current!;
    const h = hit(e);
    const id = h.region?.id ?? null;
    if (hoverRef.current !== id) {
      hoverRef.current = id;
      requestDraw();
    }
    const kind = h.inRuler ? null : grabKind(h);
    el.style.cursor = kind === null || kind === "move" ? "default" : kind === "left" || kind === "right" ? "ew-resize" : kind === "select" ? "text" : "col-resize";
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    ed.setEditorFocus(true);
    if (onScrollbar(e)) return;
    const { track } = live.current;
    const h = hit(e);
    const s = ed.rangeSel();
    if (s && h.frame >= s.start && h.frame <= s.end) {
      openMenu(e.clientX, e.clientY, ed.selectionMenu());
      return;
    }
    const r = h.region;
    if (!r) {
      openMenu(e.clientX, e.clientY, laneMenu(track, snapEd(h.frame)));
      return;
    }
    if (!getState().selectedRegions.includes(r.id)) setState({ selectedRegions: [r.id], focus: "regions" });
    const extra: MenuItem[] = [
      { label: "Select Whole Region", hint: "Double-click", onClick: () => ed.setEd({ sel: { track: track.id, start: r.start, end: r.start + r.length } }) },
      // Z zooms to the focused region (or the selection); this one may be another region, so
      // no key is shown. Z still zooms back.
      { label: "Zoom to Region", onClick: () => ed.zoomToSpan(r.start, r.start + r.length) },
      { label: "Split at Transients", hint: fmt("Mod+Shift+T"), disabled: r.content.type !== "audio", onClick: ed.splitAtTransients },
      { separator: true },
    ];
    openMenu(
      e.clientX,
      e.clientY,
      [
        ...extra,
        ...regionMenu(r, () => renameRegion(r)),
      ],
    );
  };

  return { onMouseDown, onHover, onContextMenu };
}
