// The overview strip under the Tracks area, in place of its horizontal scroll bar (like
// Ardour's Summary): the whole project drawn small, the playhead, and a box around the part
// shown in the Tracks area. Drag the box to scroll, drag its edges to zoom, click elsewhere to
// centre the view there. The mapping maths is in overviewModel.ts, the painting in draw.ts.
import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { getState, setState, useStore } from "../../state/store";
import { framePos, onFrame } from "../../state/frameLoop";
import { useLayout } from "../trackLayout";
import { samplesPerBeat } from "../../lib/time";
import { pixelRatio } from "../../ui/uiScale";
import { scrollTo, viewport } from "../viewport";
import { timeWheel } from "../../ui/wheel";
import { openMenu } from "../../ui/Menu";
import { OVERVIEW_BAND, anchoredLeft, centreOn, dragScroll, hitBox, overviewSpan, timeToX, visibleBox, xToTime, zoomFromEdge, type BoxPart, type Rect, type StripMap } from "./overviewModel";
import { type Geometry, drawStatic, drawOverlay } from "./draw";
import type { Project } from "../../engine/project";
import { regionEnd } from "../regions/loops";
import { wheelZoom } from "../zoom";
import { dragWith } from "../../ui/pointerDrag";

// ---- showing and hiding (View › Show Overview), remembered per machine -----------------------

const KEY = "marbble.showOverview";

export function savedShowOverview(): boolean {
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

export function toggleOverview() {
  const on = !getState().showOverview;
  setState({ showOverview: on });
  try {
    localStorage.setItem(KEY, on ? "1" : "0");
  } catch {
    // Storage can be unavailable; the choice still applies for this session.
  }
}

/**
 * The strip replaces the Tracks area's horizontal scroll bar, which CSS hides while it is
 * shown (`.with-overview`). WebKit only re-reads scroll bar styles when the scroller's layout
 * changes, so give it a nudge whenever the strip comes or goes.
 */
function refreshScrollbars() {
  const el = document.querySelector(".arrange-scroll") as HTMLElement | null;
  if (!el) return;
  const { scrollLeft, scrollTop } = el;
  el.style.overflow = "hidden";
  void el.offsetWidth;
  el.style.overflow = "";
  el.scrollLeft = scrollLeft;
  el.scrollTop = scrollTop;
}

/** The end of the project's content: its last region (loops included), marker or cycle range.
 *  `regions` is the end of the last region alone, drawn as the project end. */
function contentEnds(p: Project): { regions: number; all: number } {
  let regions = 0;
  for (const t of p.tracks) for (const r of t.regions) regions = Math.max(regions, regionEnd(r));
  let all = regions;
  for (const m of p.markers) all = Math.max(all, m.pos + (m.length ?? 0));
  if (p.cycle.end > p.cycle.start) all = Math.max(all, p.cycle.end);
  return { regions, all };
}

interface Drag {
  part: BoxPart;
  /** Pointer and scroll position where the drag started (or where a click outside centred it). */
  x: number;
  y: number;
  left: number;
  top: number;
  /** The mapping is frozen while dragging so the strip doesn't rescale under the pointer. */
  map: StripMap;
  /** For edge drags: the other edge's time, and the pointer's distance from the dragged edge. */
  fixed: number;
  offset: number;
}

export function Overview() {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const project = useStore((s) => s.snap?.project);
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const layout = useLayout();
  // Redraw on a theme or interface size change.
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);

  const ends = useMemo(() => (project ? contentEnds(project) : { regions: 0, all: 0 }), [project]);
  const spb = project ? samplesPerBeat(project) : 1;
  const pps = pxPerBeat / spb;
  // Some room past the end: two bars, or 6% of a long project.
  const pad = project ? Math.max(ends.all * 0.06, 2 * project.sig_num * spb) : 0;

  const live = useRef({ project, layout, pps, spb, ends, pad });
  live.current = { project, layout, pps, spb, ends, pad };
  const size = useRef({ W: 0, H: 0 });
  const dragRef = useRef<Drag | null>(null);
  const layerRef = useRef<HTMLCanvasElement | null>(null);
  // The region layer is redrawn when a render (new project, layout, zoom or theme) or a change
  // of strip size, scale or span calls for it; the box and playhead are cheap, every frame
  // something moved.
  const staticKey = useRef("");
  const overlayKey = useRef("");
  const dirty = useRef(true);

  useLayoutEffect(() => {
    dirty.current = true;
  });

  /** The mapping as things stand (frozen during a drag). */
  const geometry = (): Geometry | null => {
    const { project, layout, pps, ends, pad } = live.current;
    const { W, H } = size.current;
    if (!project || W <= 0 || H <= 0 || pps <= 0) return null;
    const vp = viewport();
    const span = overviewSpan(ends.all, (vp.left + vp.width) / pps, pad);
    const map = dragRef.current?.map ?? { span, width: W, total: layout.total, rowsTop: OVERVIEW_BAND, rowsH: H - OVERVIEW_BAND - 1 };
    return { W, H, dpr: pixelRatio(), map };
  };

  const draw = () => {
    const cv = canvasRef.current;
    const g = geometry();
    const { project, layout, pps, ends } = live.current;
    if (!cv || !g || !project) return;
    const DW = Math.round(g.W * g.dpr);
    const DH = Math.round(g.H * g.dpr);
    if (cv.width !== DW || cv.height !== DH) {
      cv.width = DW;
      cv.height = DH;
      cv.style.width = `${g.W}px`;
      cv.style.height = `${g.H}px`;
    }
    const layer = (layerRef.current ??= document.createElement("canvas"));
    const sk = `${DW}x${DH}|${g.map.span}|${g.map.total}`;
    if (dirty.current || sk !== staticKey.current) {
      dirty.current = false;
      staticKey.current = sk;
      overlayKey.current = "";
      layer.width = DW;
      layer.height = DH;
      drawStatic(layer.getContext("2d")!, g, project, layout, ends.regions);
    }
    const b = visibleBox(viewport(), pps, g.map);
    const box: [number, number, number, number] = [
      Math.round(b.x * g.dpr),
      Math.round(b.y * g.dpr),
      Math.round((b.x + b.w) * g.dpr),
      Math.round((b.y + b.h) * g.dpr),
    ];
    const phx = Math.round(timeToX(framePos(), g.map.span, g.map.width) * g.dpr);
    const ok = `${sk}|${box.join(",")}|${phx}`;
    if (ok === overlayKey.current) return;
    overlayKey.current = ok;
    drawOverlay(cv.getContext("2d")!, layer, box, phx, g.dpr);
  };
  const drawRef = useRef(draw);
  drawRef.current = draw;

  // One check per display frame covers the playhead, scrolling, zoom and resizing; it draws
  // only when something on the strip moved.
  useEffect(() => onFrame(() => drawRef.current()), []);

  useEffect(() => {
    const el = wrapRef.current!;
    const ro = new ResizeObserver(() => {
      size.current = { W: el.clientWidth, H: el.clientHeight };
    });
    ro.observe(el);
    refreshScrollbars();
    // Wheel: scrolls through time (Shift: the tracks); Ctrl/Cmd zooms around the time under
    // the pointer, or the middle of the view when the pointer is outside it.
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const g = geometry();
      const { project, pps, spb } = live.current;
      if (!g || !project) return;
      const vp = viewport();
      if (e.ctrlKey || e.metaKey) {
        const x = e.clientX - el.getBoundingClientRect().left;
        const t = xToTime(x, g.map.span, g.map.width);
        const inView = t * pps >= vp.left && t * pps <= vp.left + vp.width;
        const anchor = inView ? t : (vp.left + vp.width / 2) / pps;
        const cx = anchor * pps - vp.left;
        const next = wheelZoom(getState().pxPerBeat, e.deltaY);
        setState({ pxPerBeat: next });
        requestAnimationFrame(() => scrollTo(anchoredLeft(anchor, cx, next / spb)));
        return;
      }
      const { dx, dy } = timeWheel(e, vp.width);
      scrollTo(Math.max(0, vp.left + dx), Math.max(0, vp.top + dy));
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      ro.disconnect();
      el.removeEventListener("wheel", wheel);
      refreshScrollbars();
    };
  }, []);

  const local = (e: { clientX: number; clientY: number }) => {
    const r = wrapRef.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const boxNow = (g: Geometry): Rect => visibleBox(viewport(), live.current.pps, g.map);

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const g = geometry();
    if (!g) return;
    e.preventDefault();
    const { x, y } = local(e);
    const vp = viewport();
    const { pps } = live.current;
    const b = boxNow(g);
    const part = hitBox(b, x, y);
    let left = vp.left;
    let top = vp.top;
    // A click outside the box centres the view there (as in Ardour), and the press carries
    // on as a drag of the box.
    if (part === "outside") {
      ({ left, top } = centreOn(x, y, vp, pps, g.map));
      scrollTo(left, top);
    }
    const fixed = part === "left" ? (vp.left + vp.width) / pps : vp.left / pps;
    const offset = part === "left" ? x - b.x : part === "right" ? x - (b.x + b.w) : 0;
    dragRef.current = { part, x, y, left, top, map: g.map, fixed, offset };
    const wrap = wrapRef.current!;
    wrap.style.cursor = part === "left" || part === "right" ? "ew-resize" : "grabbing";

    const move = (ev: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const p = local(ev);
      const { pps, spb } = live.current;
      if (d.part === "left" || d.part === "right") {
        const t = Math.max(0, xToTime(p.x - d.offset, d.map.span, d.map.width));
        const z = zoomFromEdge(d.part, d.fixed, t, viewport().width, spb);
        setState({ pxPerBeat: z.pxPerBeat });
        // Scroll once the Tracks area has re-rendered at the new width.
        requestAnimationFrame(() => scrollTo((z.start * z.pxPerBeat) / spb));
        return;
      }
      const { dLeft, dTop } = dragScroll(p.x - d.x, p.y - d.y, pps, d.map);
      scrollTo(Math.max(0, d.left + dLeft), Math.max(0, d.top + dTop));
    };
    const up = () => {
      dragRef.current = null;
      wrap.style.cursor = "";
    };
    dragWith(move, up);
  };

  // The pointer shows what a press would do.
  const onHover = (e: React.MouseEvent) => {
    if (dragRef.current) return;
    const g = geometry();
    if (!g) return;
    const { x, y } = local(e);
    const part = hitBox(boxNow(g), x, y);
    wrapRef.current!.style.cursor = part === "left" || part === "right" ? "ew-resize" : part === "inside" ? "grab" : "";
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    openMenu(e.clientX, e.clientY, [{ label: "Show Overview", checked: true, onClick: toggleOverview }]);
  };

  return (
    <div className="overview" ref={wrapRef} onMouseDown={onMouseDown} onMouseMove={onHover} onContextMenu={onContextMenu}>
      <canvas ref={canvasRef} />
    </div>
  );
}
