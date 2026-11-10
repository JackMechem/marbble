// The Tracks area's ruler: cycle strip, bar numbers, Autopunch strip and marker lane.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { act } from "../../engine/commands";
import { getState, setState, useStore } from "../../state/store";
import { coalesced, framePos, onFrame } from "../../state/frameLoop";
import { samplesPerBeat, snapPos } from "../../lib/time";
import { tempoMap } from "../../lib/tempo";
import { pixelRatio } from "../../ui/uiScale";
import { mix, theme } from "../../theme/canvasTheme";
import { onViewport, RULER_H, viewport } from "../viewport";
import { clickMarker, createMarker, drawMarkers, markerAt, markerEdgeAt, markerEnd, markerLaneMenu, markerMenu, renameMarker, selectMarker, setLocatorsToMarker, setMarkerLength, type MarkerDrag } from "../markers/markers";
import { openMenu } from "../../ui/Menu";
import { getSettings } from "../../settings/appSettings";
import { scrollTo } from "../viewport";
import { timeStripWheel } from "../../ui/wheel";
import { keyHint } from "../../keyboard/keymap";
import { setLocatorsByRegions } from "../regionCommands";
import { transport } from "../../app/controlBar/transport";
import { rulerMenu } from "./rulerMenu";
import { canvasFont } from "../../ui/canvasText";
import { BARS_H, CYCLE_H, HEAD_W, onPlayheadHead, playheadStyle } from "./rulerHead";
import { dragWith, pastDragThreshold } from "../../ui/pointerDrag";

const PUNCH_H = 6;
const MARKERS_Y = BARS_H + PUNCH_H;

type Strip = "cycle" | "punch";

export function Ruler() {
  const ref = useRef<HTMLCanvasElement>(null);
  const project = useStore((s) => s.snap?.project);
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);
  // The selected marker is drawn highlighted.
  useStore((s) => s.selectedMarker);
  const live = useRef({ project, pxPerBeat });
  live.current = { project, pxPerBeat };
  const preview = useRef<{ strip: Strip; start: number; end: number } | null>(null);
  const movingMarker = useRef<MarkerDrag | null>(null);

  const draw = () => {
    const cv = ref.current;
    const { project, pxPerBeat } = live.current;
    if (!cv || !project) return;
    const vp = viewport();
    const dpr = pixelRatio();
    const W = vp.width;
    if (cv.width !== Math.round(W * dpr)) {
      cv.width = Math.round(W * dpr);
      cv.height = Math.round(RULER_H * dpr);
      cv.style.width = `${W}px`;
      cv.style.height = `${RULER_H}px`;
    }
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = theme.ruler;
    ctx.fillRect(0, 0, W, RULER_H);
    ctx.fillStyle = theme.rulerCycleZone;
    ctx.fillRect(0, 0, W, CYCLE_H);
    ctx.fillRect(0, BARS_H, W, RULER_H - BARS_H);
    ctx.fillStyle = theme.rulerEdge;
    ctx.fillRect(0, RULER_H - 1, W, 1);
    ctx.fillRect(0, MARKERS_Y, W, 1);

    const pps = pxPerBeat / samplesPerBeat(project);
    const map = tempoMap(project);

    // Cycle and Autopunch strips, dimmed while they're off.
    for (const strip of ["cycle", "punch"] as Strip[]) {
      const range = strip === "cycle" ? project.cycle : project.punch;
      const dragging = preview.current?.strip === strip;
      const c = dragging ? preview.current : range.end > range.start ? range : null;
      if (!c) continue;
      const x0 = c.start * pps - vp.left;
      const x1 = c.end * pps - vp.left;
      const on = dragging || range.enabled;
      if (strip === "punch") ctx.fillStyle = on ? theme.red : mix(theme.red, theme.rulerCycleZone, 0.65);
      else ctx.fillStyle = on ? theme.cycleOn : theme.cycleOff;
      ctx.beginPath();
      if (strip === "cycle") ctx.roundRect(x0, 1.5, x1 - x0, CYCLE_H - 3, 2);
      else ctx.roundRect(x0, BARS_H + 1, x1 - x0, PUNCH_H - 2, 1.5);
      ctx.fill();
    }

    ctx.font = canvasFont("sm", 500);
    ctx.textBaseline = "alphabetic";
    // Bars through the tempo map, so their numbers follow tempo and signature changes.
    for (const b of map.bars(Math.max(0, vp.left / pps), (vp.left + W) / pps)) {
      const x = Math.round(b.start * pps - vp.left) + 0.5;
      const barPx = (b.end - b.start) * pps;
      // Skip bar labels when they would collide.
      const every = barPx >= 34 ? 1 : barPx >= 16 ? 2 : barPx >= 8 ? 4 : 8;
      if ((b.bar - 1) % every === 0) {
        ctx.fillStyle = theme.rulerTick;
        ctx.fillRect(x, CYCLE_H + 1, 1, BARS_H - CYCLE_H - 2);
        ctx.fillStyle = theme.rulerText;
        ctx.fillText(String(b.bar), x + 3, BARS_H - 6);
      } else {
        ctx.fillStyle = theme.rulerMinor;
        ctx.fillRect(x, BARS_H - 8, 1, 7);
      }
      if (barPx / b.num >= 10) {
        ctx.fillStyle = theme.rulerMinor;
        for (let k = 1; k < b.num; k++) {
          ctx.fillRect(Math.round(map.sampleAt(b.startQ + (k * 4) / b.den) * pps - vp.left + 0.5), BARS_H - 5, 1, 4);
        }
      }
    }
    drawMarkers(ctx, project, pps, vp.left, W, MARKERS_Y + 1, movingMarker.current);

  };

  const drawRef = useRef(draw);
  drawRef.current = draw;
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));
  useLayoutEffect(() => draw());
  // The playhead handle is a DOM element moved every frame; the canvas only repaints when the
  // view, zoom or cycle changes.
  const handleRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Where the playhead's element sits in the window, for putting its line on device pixels;
    // it moves only when the Tracks area's size or the track list's width does.
    let origin = 0;
    const measure = () => (origin = handleRef.current?.parentElement?.getBoundingClientRect().left ?? 0);
    measure();
    const a = onViewport(() => {
      measure();
      requestDraw();
    });
    let last = "";
    const b = onFrame((now) => {
      const { project, pxPerBeat } = live.current;
      const el = handleRef.current;
      if (!project || !el) return;
      const x = (framePos(now) * pxPerBeat) / samplesPerBeat(project) - viewport().left;
      // On whole device pixels, as the global tracks and the Tracks area draw it.
      const st = playheadStyle(x, pixelRatio(), origin);
      const tf = st.transform + st.width;
      if (tf !== last) {
        last = tf;
        el.style.transform = st.transform;
        el.style.width = st.width;
        el.style.visibility = x < -HEAD_W || x > viewport().width + HEAD_W ? "hidden" : "visible";
      }
    });
    // A click anywhere else drops the marker selection, so Delete goes back to regions (the
    // Marker List keeps it).
    const down = (e: MouseEvent) => {
      const el = e.target as Element | null;
      if (getState().selectedMarker !== null && el !== ref.current && !el?.closest?.(".marker-list")) selectMarker(null);
    };
    window.addEventListener("mousedown", down, true);
    return () => {
      a();
      b();
      window.removeEventListener("mousedown", down, true);
    };
  }, []);

  const onMouseDown = (e: React.MouseEvent) => {
    const { project, pxPerBeat } = live.current;
    if (!project || e.button !== 0) return;
    const rect = ref.current!.getBoundingClientRect();
    const pps = pxPerBeat / samplesPerBeat(project);
    const toSample = (cx: number) => (cx - rect.left + viewport().left) / pps;
    const y = e.clientY - rect.top;
    // The ruler always snaps, to beats at least, even with Snap off: locators and markers set
    // between beats are hardly ever wanted.
    const snap = (v: number) => Math.max(0, snapPos(project, v, getState().snapMode === "off" ? "beat" : getState().snapMode, pxPerBeat));
    const at = toSample(e.clientX);

    // The playhead's head, grabbed: it follows the pointer from where it was, without jumping
    // to the press.
    const head = framePos() * pps;
    if (onPlayheadHead(at * pps - head, y)) return headDown(e, head / pps, toSample, snap);
    if (y >= MARKERS_Y) return markerDown(e, at, toSample, snap);
    if (y >= BARS_H) return stripDown(e, "punch", at, toSample, snap);
    if (y > CYCLE_H) {
      // Lower half: set and scrub the playhead.
      const locate = (s: number) => act("locate", { pos: Math.round(Math.max(0, snap(s))) });
      locate(at);
      const move = (ev: MouseEvent) => locate(toSample(ev.clientX));
      dragWith(move);
      return;
    }

    stripDown(e, "cycle", at, toSample, snap);
  };

  /** Dragging the playhead's head: it moves by as much as the pointer does, snapped; a click
   *  without a drag leaves it where it is. */
  const headDown = (e: React.MouseEvent, from: number, toSample: (cx: number) => number, snap: (v: number) => number) => {
    e.preventDefault();
    const cv = ref.current!;
    cv.style.cursor = "grabbing";
    const x0 = e.clientX;
    const at0 = toSample(x0);
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - x0) < 2) return;
      moved = true;
      act("locate", { pos: Math.round(Math.max(0, snap(from + toSample(ev.clientX) - at0))) });
    };
    const up = () => {
      cv.style.cursor = "grab";
    };
    dragWith(move, up);
  };

  // Over the head, the pointer says it can be grabbed.
  const onMouseMove = (e: React.MouseEvent) => {
    const { project, pxPerBeat } = live.current;
    const cv = ref.current;
    if (!project || !cv || e.buttons) return;
    const rect = cv.getBoundingClientRect();
    const dx = e.clientX - rect.left + viewport().left - (framePos() * pxPerBeat) / samplesPerBeat(project);
    const cursor = onPlayheadHead(dx, e.clientY - rect.top) ? "grab" : "";
    if (cv.style.cursor !== cursor) cv.style.cursor = cursor;
  };

  /** The cycle strip (top) and the Autopunch strip (below the bar numbers): drag to set or
   *  move the range or its edges, click to switch it on or off, as in Logic. */
  const stripDown = (e: React.MouseEvent, strip: Strip, at: number, toSample: (cx: number) => number, snap: (v: number) => number) => {
    const { project, pxPerBeat } = live.current;
    if (!project) return;
    const pps = pxPerBeat / samplesPerBeat(project);
    const c = strip === "cycle" ? project.cycle : project.punch;
    const commit = (enabled: boolean, start: number, end: number) => act(strip === "cycle" ? "set_cycle" : "set_punch", { enabled, start, end });
    const has = c.end > c.start;
    const edgePx = 6;
    const sx = c.start * pps;
    const ex = c.end * pps;
    const ax = at * pps;
    let mode: "new" | "move" | "start" | "end" = "new";
    if (has && Math.abs(ax - sx) < edgePx) mode = "start";
    else if (has && Math.abs(ax - ex) < edgePx) mode = "end";
    else if (has && ax > sx && ax < ex) mode = "move";
    const startX = e.clientX;
    let moved = false;
    const map = tempoMap(project);

    const move = (ev: MouseEvent) => {
      if (!moved && !pastDragThreshold(ev.clientX - startX)) return;
      moved = true;
      const cur = toSample(ev.clientX);
      let s = c.start;
      let en = c.end;
      if (mode === "new") {
        s = snap(Math.min(at, cur));
        en = snap(Math.max(at, cur));
        if (en <= s) en = s + map.samplesPerBarAt(s);
      } else if (mode === "move") {
        const d = snap(c.start + (cur - at)) - c.start;
        s = c.start + d;
        en = c.end + d;
      } else if (mode === "start") {
        s = Math.min(snap(cur), c.end - map.samplesPerBeatAt(c.end));
      } else {
        en = Math.max(snap(cur), c.start + map.samplesPerBeatAt(c.start));
      }
      preview.current = { strip, start: Math.max(0, s), end: Math.max(0, en) };
      draw();
    };
    const up = () => {
      const p = preview.current;
      preview.current = null;
      if (moved && p) {
        commit(true, Math.round(p.start), Math.round(p.end));
      } else if (mode === "move" || mode === "start" || mode === "end") {
        // A click on the strip toggles Cycle (or Autopunch), like Logic.
        commit(!c.enabled, c.start, c.end);
      } else {
        draw();
      }
    };
    dragWith(move, up);
  };

  /** The marker lane: click a marker to select it and go to it (with Cycle on, the cycle takes
   *  its length), drag it to move it (snapped) or up into the cycle strip to set the locators
   *  by it, drag the end of its bar to change its length, double-click it to rename it;
   *  double-click the empty lane to create one there. */
  const markerDown = (e: React.MouseEvent, at: number, toSample: (cx: number) => number, snap: (v: number) => number) => {
    const { project, pxPerBeat } = live.current;
    if (!project) return;
    const pps = pxPerBeat / samplesPerBeat(project);
    const edge = markerEdgeAt(at * pps);
    if (edge) return markerLengthDown(e, edge, toSample, snap);
    const m = markerAt(at * pps);
    if (!m) {
      selectMarker(null);
      if (e.detail === 2) createMarker(snap(at));
      return;
    }
    if (e.detail === 2) {
      renameMarker(m);
      return;
    }
    selectMarker(m.id);
    const top = ref.current!.getBoundingClientRect().top;
    const startX = e.clientX;
    let moved = false;
    let toCycle = false;
    const move = (ev: MouseEvent) => {
      if (!moved && !pastDragThreshold(ev.clientX - startX, ev.clientY - e.clientY)) return;
      moved = true;
      // Over the cycle strip, the marker sets the locators instead of moving.
      toCycle = ev.clientY - top < CYCLE_H;
      if (toCycle) {
        movingMarker.current = null;
        const end = markerEnd(project, m) ?? m.pos + tempoMap(project).samplesPerBarAt(m.pos) * 4;
        preview.current = { strip: "cycle", start: m.pos, end };
      } else {
        preview.current = null;
        movingMarker.current = { id: m.id, pos: Math.round(snap(m.pos + toSample(ev.clientX) - at)) };
      }
      draw();
    };
    const up = () => {
      const mv = movingMarker.current;
      movingMarker.current = null;
      preview.current = null;
      if (moved && toCycle) setLocatorsToMarker(m);
      else if (moved && mv && mv.pos !== m.pos) act("move_marker", { id: m.id, pos: mv.pos });
      else if (!moved) clickMarker(m);
      else draw();
    };
    dragWith(move, up);
  };

  /** Dragging the end of a marker's bar: its own length, snapped, at least a beat. */
  const markerLengthDown = (e: React.MouseEvent, m: { id: number; pos: number; length?: number | null }, toSample: (cx: number) => number, snap: (v: number) => number) => {
    const { project } = live.current;
    if (!project) return;
    e.preventDefault();
    selectMarker(m.id);
    const end0 = m.pos + (m.length ?? 0);
    const x0 = toSample(e.clientX);
    const min = tempoMap(project).samplesPerBeatAt(m.pos);
    const move = (ev: MouseEvent) => {
      const end = Math.max(m.pos + min, snap(end0 + toSample(ev.clientX) - x0));
      movingMarker.current = { id: m.id, pos: m.pos, length: Math.round(end - m.pos) };
      draw();
    };
    const up = () => {
      const mv = movingMarker.current;
      movingMarker.current = null;
      const marker = project.markers.find((x) => x.id === m.id);
      if (mv?.length && marker && mv.length !== m.length) setMarkerLength(marker, mv.length);
      else draw();
    };
    dragWith(move, up);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    const { project, pxPerBeat } = live.current;
    const rect = ref.current!.getBoundingClientRect();
    if (!project) return;
    e.preventDefault();
    const pps = pxPerBeat / samplesPerBeat(project);
    const at = (e.clientX - rect.left + viewport().left) / pps;
    // Snapped as a click in the ruler is: to beats at least.
    const snapped = Math.round(Math.max(0, snapPos(project, at, getState().snapMode === "off" ? "beat" : getState().snapMode, pxPerBeat)));
    if (e.clientY - rect.top < MARKERS_Y) {
      const s = getState();
      const menu = rulerMenu(
        {
          cycle: project.cycle.enabled,
          autopunch: project.punch.enabled,
          regions: s.selectedRegions.length > 0,
          snap: s.snapMode,
          hints: { cycle: keyHint("cycle"), autopunch: keyHint("autopunch"), setLocators: keyHint("set-locators") },
        },
        {
          locate: () => act("locate", { pos: snapped }),
          createMarker: () => createMarker(snapped),
          toggleCycle: () => transport.toggleCycle(),
          toggleAutopunch: () => transport.togglePunch(),
          setLocatorsByRegions: () => setLocatorsByRegions(),
          setSnap: (snapMode) => setState({ snapMode }),
        },
      );
      return openMenu(e.clientX, e.clientY, menu);
    }
    const m = markerAt(at * pps);
    if (m) selectMarker(m.id);
    openMenu(e.clientX, e.clientY, m ? markerMenu(m) : markerLaneMenu(snapped));
  };

  // The wheel scrolls through time here, as over the global tracks.
  const onWheel = (e: React.WheelEvent) => {
    const dx = timeStripWheel(e, getSettings().ui.wheel_scrolls ?? "vertical", viewport().width);
    if (dx) scrollTo(Math.max(0, viewport().left + dx));
  };

  return (
    <div className="ruler-wrap">
      <canvas ref={ref} className="ruler" onMouseDown={onMouseDown} onMouseMove={onMouseMove} onContextMenu={onContextMenu} onWheel={onWheel} />
      <div className="ruler-playhead" ref={handleRef} style={{ ["--ph" as string]: theme.playhead }} />
    </div>
  );
}
