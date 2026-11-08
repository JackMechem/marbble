// The Tracks area's canvas. This component holds the refs and the effects (viewport, playhead, recording
// previews); tracksCanvas/draw.ts paints it and tracksCanvas/pointer.ts handles the pointer. Region drawing
// itself is in regionDraw.ts, what a press grabs in regionGrab.ts.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { type Id, type Project } from "../api";
import { getState, liveTakes, onTick, setState, currentTick, useStore } from "../store";
import { ensureWave } from "../waveform";
import { coalesced, framePos, onFrame } from "../frame";
import { samplesPerBeat } from "../time";
import { useLayout, zoomTracks } from "../layout";
import { onViewport, registerScroller, setViewport, viewport } from "../view";
import { regionEnd } from "../loops";
import { automationLeave } from "../automation";
import { getSettings } from "../prefs";
import { ensureTakeWaves } from "../takes";
import { onRevealPlayhead, revealLeft } from "../view";
import { timeWheel } from "../wheel";
import { escapeMarquee, toolLeave } from "../trackTools";
import { drawTracks } from "./tracksCanvas/draw";
import type { Drag, TracksLive } from "./tracksCanvas/drag";
import { tracksPointer } from "./tracksCanvas/pointer";
import { onDrumMarks } from "../drumReplace";

// ---- peaks ----------------------------------------------------------------------------------

let redraw: () => void = () => {};

function ensurePeaks(project: Project) {
  for (const t of project.tracks) for (const r of t.regions) if (r.content.type === "audio") ensureWave(r.content.clip, () => redraw());
  ensureTakeWaves(project.tracks, () => redraw());
}


export function Arrange() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const playheadRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<Drag | null>(null);
  // A Quick Swipe Comping drag in progress: the take and take-time range.
  const swipeRef = useRef<{ region: Id; take: Id; start: number; end: number } | null>(null);
  // A rubber-band selection in progress, in content coordinates.
  const marqueeRef = useRef<{ left: number; top: number; right: number; bottom: number } | null>(null);
  // Region under the pointer, which shows its fade handles.
  const hoverRef = useRef<Id | null>(null);
  const drawRef = useRef<() => void>(() => {});
  // Repaints are coalesced to at most one per display frame.
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));
  const project = useStore((s) => s.snap?.project);
  const selectedTrack = useStore((s) => s.snap?.selected ?? null);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const layout = useLayout();
  const baseHeight = useStore((s) => s.trackHeight);
  // Redraw while a file drag moves over the tracks.
  useStore((s) => s.dropTarget);
  // Re-render (and so redraw) when the theme changes.
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);
  useStore((s) => s.waveStyle);
  const showAutomation = useStore((s) => s.showAutomation);
  useStore((s) => s.autoLane);
  useStore((s) => s.marquee);

  const pps = project ? pxPerBeat / samplesPerBeat(project) : 0;
  const lengthSamples = project
    ? Math.max(
        project.sample_rate * 60 * 6,
        ...project.tracks.flatMap((t) => t.regions.map(regionEnd)).map((e) => e + project.sample_rate * 60),
      )
    : 0;
  const contentW = Math.ceil(lengthSamples * pps);
  const contentH = layout.total + (layout.output ? layout.output.height + layout.output.autoRows : 0) + baseHeight * 4;

  // Keep the latest values for the imperative draw routine.
  const live = useRef<TracksLive>({ project, selectedTrack, selectedRegions, pps, layout });
  live.current = { project, selectedTrack, selectedRegions, pps, layout };

  const draw = () => drawTracks({ canvasRef, live, dragRef, swipeRef, marqueeRef, hoverRef, positionPlayhead });
  drawRef.current = draw;
  redraw = requestDraw;

  const lastPlayhead = useRef("");
  const positionPlayhead = (pos: number = framePos()) => {
    const ph = playheadRef.current;
    const { project, pps } = live.current;
    if (!ph || !project) return;
    const x = pos * pps - viewport().left;
    const tf = `translate3d(${x.toFixed(1)}px,0,0)`;
    const visible = !(x < -2 || x > viewport().width + 2);
    // Touch the DOM only when something changed.
    const key = visible ? tf : "hidden";
    if (key === lastPlayhead.current) return;
    lastPlayhead.current = key;
    ph.style.transform = tf;
    ph.style.display = visible ? "block" : "none";
  };

  useEffect(() => {
    if (project) ensurePeaks(project);
  }, [project]);

  useLayoutEffect(() => {
    requestDraw();
  });
  // Drum Replacement's markers follow its sheet.
  useEffect(() => onDrumMarks(requestDraw), [requestDraw]);

  // Viewport sizing
  useEffect(() => {
    const el = scrollRef.current!;
    const ro = new ResizeObserver(() => {
      setViewport({ width: el.clientWidth, height: el.clientHeight });
    });
    ro.observe(el);
    registerScroller((left, top) => {
      el.scrollLeft = left;
      if (top !== undefined) el.scrollTop = top;
    });
    const off = onViewport(() => requestDraw());
    // Ctrl/Cmd + wheel zooms horizontally around the pointer.
    const wheel = (e: WheelEvent) => {
      if (e.altKey) {
        e.preventDefault();
        zoomTracks(e.deltaY < 0 ? 1.1 : 1 / 1.1);
        return;
      }
      if (!(e.ctrlKey || e.metaKey)) {
        // Settings › Editing › Mouse Wheel Scrolls: Horizontally. The default is left to native
        // scrolling.
        if (getSettings().ui.wheel_scrolls !== "horizontal") return;
        e.preventDefault();
        const { dx, dy } = timeWheel(e, el.clientWidth);
        el.scrollLeft += dx;
        el.scrollTop += dy;
        return;
      }
      e.preventDefault();
      const { project, pps } = live.current;
      if (!project) return;
      const cx = e.clientX - el.getBoundingClientRect().left;
      const sample = (el.scrollLeft + cx) / pps;
      const s = getState();
      const next = Math.min(400, Math.max(2, s.pxPerBeat * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
      setState({ pxPerBeat: next });
      const nextPps = next / samplesPerBeat(project);
      requestAnimationFrame(() => {
        el.scrollLeft = Math.max(0, sample * nextPps - cx);
      });
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      ro.disconnect();
      off();
      el.removeEventListener("wheel", wheel);
    };
  }, []);

  // Playhead, catch and live recording previews
  useEffect(() => {
    let wasRecording = false;
    return onTick((t) => {
      if (!live.current.project) return;
      for (const p of t.previews) {
        let take = liveTakes.get(p.track);
        if (!take || take.start !== p.start) {
          take = { start: p.start, frames: 0, peaks: [] };
          liveTakes.set(p.track, take);
        }
        take.peaks.length = p.from;
        take.peaks.push(...p.peaks);
        take.frames = p.frames;
      }
      const finished = wasRecording && !t.recording;
      if (finished) liveTakes.clear();
      wasRecording = t.recording;
      if (finished || t.previews.length || liveTakes.size) requestDraw();
    });
  }, []);

  // Playhead and Catch follow the interpolated position every display frame.
  useEffect(
    () =>
      onFrame((now) => {
        const { project, pps } = live.current;
        if (!project) return;
        const pos = framePos(now);
        positionPlayhead(pos);
        const t = currentTick();
        if (t?.playing && getState().catchPlayhead) {
          const vp = viewport();
          const x = pos * pps - vp.left;
          const el = scrollRef.current!;
          if (x > vp.width - 30) el.scrollLeft = pos * pps - 60;
          else if (x < 0) el.scrollLeft = Math.max(0, pos * pps - 60);
        }
      }),
    [],
  );

  // Go to Beginning, a second Stop and the like move the playhead while stopped, when Catch
  // doesn't follow it: bring it into view (horizontally only).
  useEffect(
    () =>
      onRevealPlayhead((pos) => {
        const el = scrollRef.current!;
        const left = revealLeft(pos * live.current.pps, el.scrollLeft, el.clientWidth);
        if (left !== null) el.scrollLeft = left;
      }),
    [],
  );

  // Esc removes the marquee.
  useEffect(() => {
    window.addEventListener("keydown", escapeMarquee);
    return () => window.removeEventListener("keydown", escapeMarquee);
  }, []);

  const { onMouseDown, onHover, onContextMenu } = tracksPointer({ scrollRef, live, dragRef, swipeRef, marqueeRef, hoverRef, requestDraw, showAutomation });

  return (
    <div className="arrange">
      <canvas ref={canvasRef} className="arrange-canvas" />
      <div className="playhead" ref={playheadRef} />
      <div
        className="arrange-scroll"
        ref={scrollRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          setViewport({ left: el.scrollLeft, top: el.scrollTop });
        }}
        onMouseDown={onMouseDown}
        onMouseMove={onHover}
        onMouseLeave={() => {
          automationLeave({ redraw: requestDraw });
          toolLeave(requestDraw);
        }}
        onContextMenu={onContextMenu}
      >
        <div style={{ width: contentW, height: contentH }} />
      </div>
    </div>
  );
}
