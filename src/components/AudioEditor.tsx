// The Audio Track editor (bottom pane): the track's regions on the project timeline, drawn
// large, with Logic's Audio File Editor-style range editing done non-destructively. The state
// and the actions live in audioEdit.ts, the layout in audioEditorModel.ts. This component holds
// the refs, zoom, scrolling and the effects; audioTrackEditor/ draws and handles the pointer.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { type Id, type Region, type Track } from "../api";
import * as ed from "../audioEdit";
import { coalesced, framePos, onFrame } from "../frame";
import { currentTick, getState, useStore } from "../store";
import { theme } from "../theme";
import { tempoMap } from "../tempo";
import { openMenuAt } from "./Menu";
import { fmt } from "../keymap";
import { onRevealPlayhead, revealLeft } from "../view";
import { DetachButton } from "./DetachButton";
import { useBarFit } from "./useBarFit";
import { timelineFrames, zoomBounds, type Drag } from "../audioEditorModel";
import { editorDrawer, type AudioLive } from "./audioTrackEditor/draw";
import { GainField } from "./audioTrackEditor/GainField";
import { InfoStrip } from "./audioTrackEditor/InfoStrip";
import { editorPointer } from "./audioTrackEditor/pointer";

/** Scroll position kept while the editor is closed, like its zoom. */
let lastLeft = 0;

/** What the local bar gives way with, in order (barFit.ts); what leaves it is in the View menu.
 *  The region's gain stays. */
const BAR_STEPS = ["captions", "tight", "detach", "wave", "transients", "zoom", "title"] as const;

export function AudioEditor({ track, region }: { track: Track; region: Region | null }) {
  const project = useStore((s) => s.snap?.project);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const themeId = useStore((s) => s.themeId);
  // Subscribed to without using the value: a change re-renders, and so redraws, the canvas
  // (the draw code reads these itself).
  useStore((s) => s.uiScale);
  useStore((s) => s.waveStyle);
  const pps = ed.useEd((s) => s.pps);
  const amp = ed.useEd((s) => s.amp);
  const showTransients = ed.useEd((s) => s.showTransients);
  ed.useEd((s) => s.sensitivity);
  const sel = ed.useEd((s) => (s.sel && s.sel.track === track.id ? s.sel : null));

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scaleRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const playheadRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<Drag | null>(null);
  const hoverRef = useRef<Id | null>(null);
  // Where to scroll once the next zoom has laid out; the saved position on opening.
  const pendingLeft = useRef<number | null>(lastLeft);
  const drawRef = useRef<() => void>(() => {});
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));
  const accent = useMemo(() => getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#4c9eff", [themeId]);

  const focus = region && track.regions.some((r) => r.id === region.id) ? region : null;
  const total = project ? timelineFrames(project) : 1;
  const contentW = Math.ceil(total * pps);
  const live = useRef<AudioLive>({ project, track, selectedRegions, pps, amp, showTransients, accent, total });
  live.current = { project, track, selectedRegions, pps, amp, showTransients, accent, total };

  // ---- zoom and scroll ----------------------------------------------------------------------

  const ppsBounds = () => zoomBounds(scrollRef.current?.clientWidth || 600, live.current.total);

  /** Sets the zoom, keeping `anchor` (a frame) at `anchorPx` in the view. */
  const setZoom = (next: number, anchor?: number, anchorPx?: number) => {
    const el = scrollRef.current;
    if (!el) return;
    const [lo, hi] = ppsBounds();
    const cur = live.current.pps || next;
    const v = Math.min(hi, Math.max(lo, next));
    const px = anchorPx ?? el.clientWidth / 2;
    const at = anchor ?? (el.scrollLeft + px) / cur;
    const left = Math.max(0, at * v - px);
    if (v === live.current.pps) el.scrollLeft = left;
    else {
      pendingLeft.current = left;
      ed.setEd({ pps: v });
    }
  };

  const zoomTo = (a: number, b: number) => {
    const el = scrollRef.current;
    if (!el) return;
    const W = el.clientWidth;
    const [lo, hi] = ppsBounds();
    const v = Math.min(hi, Math.max(lo, (W * 0.94) / Math.max(1, b - a)));
    const left = Math.max(0, a * v - W * 0.03);
    if (v === live.current.pps) el.scrollLeft = left;
    else {
      pendingLeft.current = left;
      ed.setEd({ pps: v });
    }
  };

  useEffect(() => {
    ed.attachView({
      track: track.id,
      region: focus?.id ?? null,
      zoomTo,
      zoomBy: (f) => setZoom(live.current.pps * f),
      saveView: () => ({ pps: live.current.pps, left: scrollRef.current?.scrollLeft ?? 0 }),
      restoreView: (v) => {
        if (v.pps === live.current.pps && scrollRef.current) scrollRef.current.scrollLeft = v.left;
        else {
          pendingLeft.current = v.left;
          ed.setEd({ pps: v.pps });
        }
      },
    });
  });
  useEffect(() => () => ed.attachView(null), []);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !project) return;
    if (live.current.pps <= 0 && el.clientWidth > 0) {
      // First time open: fit the region (or the track's regions, or eight bars).
      const regs = focus ? [focus] : track.regions;
      if (regs.length) zoomTo(Math.min(...regs.map((r) => r.start)), Math.max(...regs.map((r) => r.start + r.length)));
      else zoomTo(0, tempoMap(project).barStart(9));
      return;
    }
    if (pendingLeft.current !== null) {
      el.scrollLeft = pendingLeft.current;
      pendingLeft.current = null;
    }
    requestDraw();
  });

  // Bring a newly selected region into view if it's off screen.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !focus || pps <= 0) return;
    const x0 = focus.start * pps;
    const x1 = (focus.start + focus.length) * pps;
    if (x1 < el.scrollLeft || x0 > el.scrollLeft + el.clientWidth) el.scrollLeft = Math.max(0, x0 - el.clientWidth * 0.05);
  }, [focus?.id]);

  // ---- drawing --------------------------------------------------------------------------------

  drawRef.current = editorDrawer({ canvasRef, scaleRef, scrollRef, playheadRef, dragRef, hoverRef, live, requestDraw, positionPlayhead: () => positionPlayhead() });

  // ---- playhead -------------------------------------------------------------------------------

  const lastPlayhead = useRef("");
  const positionPlayhead = (pos: number = framePos()) => {
    const ph = playheadRef.current;
    const el = scrollRef.current;
    const { pps } = live.current;
    if (!ph || !el || pps <= 0) return;
    const x = pos * pps - el.scrollLeft;
    const visible = x >= -2 && x <= el.clientWidth + 2;
    const key = visible ? `translate3d(${x.toFixed(1)}px,0,0)` : "hidden";
    if (key === lastPlayhead.current) return;
    lastPlayhead.current = key;
    if (visible) ph.style.transform = key;
    ph.style.display = visible ? "block" : "none";
  };

  useEffect(() => {
    const el = scrollRef.current!;
    const ro = new ResizeObserver(() => requestDraw());
    ro.observe(el);
    const wheel = (e: WheelEvent) => {
      const { pps } = live.current;
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        const cx = e.clientX - el.getBoundingClientRect().left;
        setZoom(pps * (e.deltaY < 0 ? 1.2 : 1 / 1.2), (el.scrollLeft + cx) / pps, cx);
      } else if (e.altKey) {
        e.preventDefault();
        ed.setAmp(ed.edState().amp * (e.deltaY < 0 ? 1.25 : 0.8));
      } else if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
        // Nothing scrolls vertically here, so the wheel moves along the timeline.
        e.preventDefault();
        el.scrollLeft += e.deltaY;
      }
    };
    el.addEventListener("wheel", wheel, { passive: false });
    window.addEventListener("keydown", ed.onEditorKey, true);
    window.addEventListener("mousedown", ed.onWindowMouseDown, true);
    const offFrame = onFrame((now) => {
      const { pps } = live.current;
      if (pps <= 0) return;
      const pos = framePos(now);
      positionPlayhead(pos);
      if (currentTick()?.playing && getState().catchPlayhead && !dragRef.current) {
        const x = pos * pps - el.scrollLeft;
        if (x > el.clientWidth - 30 || x < 0) el.scrollLeft = Math.max(0, pos * pps - 60);
      }
    });
    // Go to Beginning and a second Stop bring the playhead into view, as in the Tracks area.
    const offReveal = onRevealPlayhead((pos) => {
      const left = revealLeft(pos * live.current.pps, el.scrollLeft, el.clientWidth);
      if (left !== null) el.scrollLeft = left;
    });
    return () => {
      ro.disconnect();
      el.removeEventListener("wheel", wheel);
      window.removeEventListener("keydown", ed.onEditorKey, true);
      window.removeEventListener("mousedown", ed.onWindowMouseDown, true);
      offFrame();
      offReveal();
    };
  }, []);

  // ---- pointer --------------------------------------------------------------------------------

  const { onMouseDown, onHover, onContextMenu } = editorPointer({ scrollRef, dragRef, hoverRef, live, requestDraw });

  // ---- local bar ------------------------------------------------------------------------------

  const fit = useBarFit(BAR_STEPS, track.name);
  const [lo, hi] = ppsBounds();
  const zoomValue = pps > 0 && hi > lo ? Math.log(pps / lo) / Math.log(hi / lo) : 0;

  return (
    <div className="editor audio-editor">
      <div ref={fit.ref} className={"local-bar editor-bar fit-bar aed-bar" + fit.className}>
        <div className="local-menus">
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, ed.editMenu())}>
            Edit
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, ed.functionsMenu())}>
            Functions
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, ed.viewMenu())}>
            View
          </button>
          {!fit.has("title") && <span className="local-title">Audio Track: {track.name}</span>}
        </div>
        <div className="local-right">
          {!fit.has("transients") && (
            <button className={"lb-btn" + (showTransients ? " on" : "")} aria-pressed={showTransients} title="Show Transients" aria-label="Show Transients" onClick={() => ed.setEd({ showTransients: !showTransients })}>
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
                <path d="M1 7h2l1.2-4 1.6 8L7 5l1 3.5L9 7h4" />
                <path d="M4.2 1.5v1M9 1.5v1" strokeLinecap="round" />
              </svg>
            </button>
          )}
          <span className="lb-label">Gain</span>
          <GainField region={focus} />
          {!fit.has("wave") && (
            <>
              <span className="lb-label" title="Waveform zoom">
                Wave
              </span>
              <input
                className="zoom vzoom"
                type="range"
                min={0}
                max={1}
                step={0.001}
                value={Math.log(amp) / Math.log(ed.MAX_AMP)}
                title={`Waveform zoom ×${amp.toFixed(1)} (${fmt("Alt")}-scroll, ${fmt("Mod+ArrowUp")} / ${fmt("Mod+ArrowDown")})`}
                onChange={(e) => ed.setAmp(Math.pow(ed.MAX_AMP, parseFloat(e.target.value)))}
                onDoubleClick={() => ed.setAmp(1)}
              />
            </>
          )}
          {!fit.has("zoom") && (
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
                value={zoomValue}
                title={`Horizontal zoom (${fmt("Mod")}-scroll, ${fmt("Mod+ArrowLeft")} / ${fmt("Mod+ArrowRight")}, Z)`}
                onChange={(e) => setZoom(lo * Math.pow(hi / lo, parseFloat(e.target.value)))}
              />
            </>
          )}
          {!fit.has("detach") && <DetachButton panel="editor" />}
        </div>
      </div>
      {project && <InfoStrip project={project} track={track} region={focus} sel={sel} />}
      <div className="aed-body">
        <canvas ref={scaleRef} className="aed-scale" />
        <div className="aed-main">
          <canvas ref={canvasRef} className="aed-canvas" />
          <div className="aed-playhead" ref={playheadRef} style={{ ["--ph" as string]: theme.playhead }} />
          <div className="aed-scroll" ref={scrollRef} onScroll={(e) => {
              lastLeft = e.currentTarget.scrollLeft;
              requestDraw();
            }}
            onMouseDown={onMouseDown} onMouseMove={onHover} onContextMenu={onContextMenu}>
            <div style={{ width: contentW, height: 1 }} />
          </div>
        </div>
      </div>
    </div>
  );
}
