// The Piano Roll: Logic's MIDI editor for a software instrument track. It shows every MIDI
// region on the track in project time (the selected one highlighted, the others dimmed but
// editable), with a keyboard, a ruler, and a velocity / controller lane underneath.
//
// Pure maths lives in pianoRollModel.ts and midiEdit.ts and drawing in pianoRollDraw.ts. The parts
// in pianoRollEditor/ handle the pointer, keys and menus: a drag builds a preview of the edited list,
// and releasing sends the result with `set_midi_content` as one undo step. This component owns the
// refs and the effects and lays out the panes.
import { useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { type Id, type Project, type Region, type Track } from "../api";
import { coalesced, framePos, onFrame } from "../frame";
import { Icon } from "../icons";
import * as me from "../midiEdit";
import { KEY_W, PR_RULER_H, xOf } from "../pianoRollDraw";
import { currentTick, useStore, type MidiSelection } from "../store";
import { samplesPerBeat } from "../time";
import { openMenu, openMenuAt } from "./Menu";
import { onRevealPlayhead, revealLeft } from "../view";
import { type CurveShape } from "../ccCurves";
import { editorCommands } from "./pianoRollEditor/commands";
import { editorCore, type Live, type Work } from "./pianoRollEditor/core";
import { editorKeys, installMenuHandler } from "./pianoRollEditor/keys";
import { laneGestures } from "./pianoRollEditor/laneGestures";
import { editorMenus } from "./pianoRollEditor/menus";
import { noteGestures } from "./pianoRollEditor/noteGestures";
import { PianoRollBar } from "./pianoRollEditor/PianoRollBar";
import { usePrefs } from "./pianoRollEditor/prefs";
import { QuantizePanel } from "./pianoRollEditor/QuantizePanel";
import { installWheel } from "./pianoRollEditor/wheel";
import { fitPitchScroll, LANE_MIN_PX, laneShort, midiRegions, timeBase } from "../pianoRollModel";
import type { PCc, PNote } from "../midiEdit";

// App.tsx, DetachedWindow.tsx and TrackTools.tsx import these from here.
export { pianoRollHasFocus, pianoRollMenu } from "./pianoRollEditor/keys";
export { ToolIcon } from "./pianoRollEditor/ToolIcon";

/** Piano Roll for a software instrument track; `region` is the one selected in the tracks area. */
export function PianoRoll({ track, region }: { track: Track; region: Region | null }) {
  const project = useStore((s) => s.snap?.project) as Project;
  const themeId = useStore((s) => s.themeId);
  const showStepInput = useStore((s) => s.showStepInput);
  useStore((s) => s.uiScale);
  const pr = usePrefs();
  const [, bump] = useReducer((x: number) => x + 1, 0);
  const [qOpen, setQOpen] = useState(false);

  // The working list follows the snapshot; edits replace it ahead of the next snapshot.
  const work = useRef<Work>({ source: [], regions: [], notes: [], cc: [], selNotes: new Set(), selCc: new Set() });
  // Edits sent but not yet answered. Snapshots arriving meanwhile are older than the working
  // list, so it isn't rebuilt from them until the last answer is in.
  const inflight = useRef(0);
  if (work.current.source !== track.regions && inflight.current === 0) {
    const regions = midiRegions(track);
    const w = me.fromRegions(regions, work.current.selNotes, work.current.selCc);
    work.current = { ...work.current, source: track.regions, regions, notes: w.notes, cc: w.cc };
  }
  // The note selection is shared with the Event List (the store's midiSelection).
  const sharedSel = useStore((s) => s.midiSelection);
  const seenSel = useRef<MidiSelection | null>(null);
  if (sharedSel !== seenSel.current) {
    seenSel.current = sharedSel;
    const selNotes = new Set(sharedSel.notes);
    const selCc = new Set(sharedSel.cc);
    const w = me.fromRegions(work.current.regions, selNotes, selCc);
    work.current = { ...work.current, notes: w.notes, cc: w.cc, selNotes, selCc };
  }
  const regions = work.current.regions;
  const focus = region && regions.some((r) => r.id === region.id) ? region.id : null;

  /** What a drag in progress would make the list; drawn instead of the working list. */
  const preview = useRef<{ notes: PNote[]; cc: PCc[] } | null>(null);
  const rubber = useRef<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const laneLine = useRef<{ x0: number; y0: number; x1: number; y1: number; shape?: CurveShape } | null>(null);
  const laneRubber = useRef<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const cyclePreview = useRef<{ start: number; end: number } | null>(null);
  const pressedKey = useRef<number | null>(null);
  const hoverPitch = useRef<number | null>(null);
  const lastArea = useRef<"notes" | "lane">("notes");
  const lastVelocity = useRef(100);
  const toolArmed = useRef(0);
  const pendingScroll = useRef<{ left?: number; top?: number } | null>(null);

  const rootRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const notesCv = useRef<HTMLCanvasElement>(null);
  const keysCv = useRef<HTMLCanvasElement>(null);
  const rulerCv = useRef<HTMLCanvasElement>(null);
  const laneCv = useRef<HTMLCanvasElement>(null);
  const scaleCv = useRef<HTMLCanvasElement>(null);
  const laneWrap = useRef<HTMLDivElement>(null);
  const playheads = [useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null)];
  const tagRef = useRef<HTMLDivElement>(null);
  const toolBtn = useRef<HTMLButtonElement>(null);
  const qBtn = useRef<HTMLButtonElement>(null);
  // The sound playing for the note being dragged, and the last playhead position written to the DOM.
  const sounding = useRef<number | null>(null);
  const lastPh = useRef("");
  const live = useRef<Live>({ project, track, focus, pr, themeId });
  live.current = { project, track, focus, pr, themeId };

  // Repaints are coalesced to one per frame; drawRef always holds this render's draw.
  const drawRef = useRef<() => void>(() => {});
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));

  // The editor's parts. They hold no state, only closures over the refs above.
  const ed = editorCore({ work, inflight, seenSel, preview, rubber, laneLine, laneRubber, cyclePreview, pressedKey, hoverPitch, lastArea, lastVelocity, toolArmed, pendingScroll, sounding, lastPh, live, rootRef, scrollRef, notesCv, keysCv, rulerCv, laneCv, scaleCv, laneWrap, playheads, tagRef, toolBtn, requestDraw, bump });
  drawRef.current = ed.draw;
  const { view, placePlayhead, commit } = ed;
  const commands = editorCommands(ed);
  const { zoomX, zoomY, quantizeNow } = commands;
  const menus = editorMenus(ed, commands);
  const { curveItems, laneItems } = menus;
  const { onNotesDown, onNotesHover, onNotesContext, onKeysDown, onRulerDown, onSplitDown } = noteGestures(ed, commands, menus);
  const { onLaneDown } = laneGestures(ed);
  const onKeyDown = editorKeys(ed, commands, menus);

  useLayoutEffect(() => {
    const ps = pendingScroll.current;
    const el = scrollRef.current;
    if (ps && el) {
      if (ps.left !== undefined) el.scrollLeft = Math.max(0, ps.left);
      if (ps.top !== undefined) el.scrollTop = Math.max(0, ps.top);
      pendingScroll.current = null;
    }
    requestDraw();
  });

  useEffect(() => {
    const el = scrollRef.current!;
    const ro = new ResizeObserver(() => requestDraw());
    ro.observe(el);
    if (laneWrap.current) ro.observe(laneWrap.current);
    const off = onFrame((now) => {
      const pos = framePos(now);
      placePlayhead(pos);
      const t = currentTick();
      if (t?.playing && live.current.pr.catchPlayhead) {
        const v = view();
        const x = xOf(v, pos);
        // Page along like Logic: jump when the playhead reaches the right edge.
        if (x > v.W - 24 || x < 0) el.scrollLeft = Math.max(0, pos * v.pps - v.W * 0.08);
      }
    });
    // Go to Beginning and a second Stop bring the playhead into view, as in the Tracks area.
    const offReveal = onRevealPlayhead((pos) => {
      const v = view();
      const left = revealLeft(pos * v.pps, v.left, v.W, v.W * 0.08);
      if (left !== null) el.scrollLeft = left;
    });
    return () => {
      ro.disconnect();
      off();
      offReveal();
    };
  }, []);

  // Scroll to the edited region when the track changes, or when another region is picked and
  // it's out of view.
  const scrolledFor = useRef<{ track: Id; focus: Id | null } | null>(null);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const prev = scrolledFor.current;
    if (prev && prev.track === track.id && prev.focus === focus) return;
    scrolledFor.current = { track: track.id, focus };
    const r = regions.find((x) => x.id === focus) ?? regions[0];
    const v = view();
    if (prev && prev.track === track.id && r) {
      const x0 = xOf(v, r.start);
      if (x0 >= 0 && x0 < v.W - 40) return;
    }
    // The region's notes in view: their pitch range centred (pianoRollModel's fitPitchScroll).
    const pitches = work.current.notes.filter((n) => !r || n.region === r.id).map((n) => n.pitch);
    el.scrollLeft = r ? Math.max(0, r.start * v.pps - 24) : el.scrollLeft;
    el.scrollTop = fitPitchScroll(pitches.length ? Math.min(...pitches) : null, pitches.length ? Math.max(...pitches) : null, v.noteH, el.clientHeight || v.H);
    requestDraw();
  }, [track.id, focus]);

  useEffect(() => installWheel(scrollRef.current!, laneWrap.current, rulerCv.current, keysCv.current, zoomX, zoomY), [pr.showLane]);

  // Native menu bar: Edit commands go to the Piano Roll while it has focus.
  useEffect(() => installMenuHandler(rootRef, commands));

  // ---- layout -------------------------------------------------------------------------------

  const v = view();
  const pps = pr.pxPerBeat / samplesPerBeat(project);
  const bar = me.barLength(timeBase(project));
  const lastEnd = Math.max(0, ...project.tracks.flatMap((t) => t.regions.map((r) => r.start + r.length)), project.cycle.end);
  const contentW = Math.max(v.W + 1, Math.ceil((lastEnd + 16 * bar) * pps));
  const contentH = 128 * pr.noteH;
  const selNotes = work.current.notes.filter((n) => n.sel);
  const selVel = selNotes.length ? Math.round(selNotes.reduce((a, n) => a + n.velocity, 0) / selNotes.length) : null;
  const focusRegion = regions.find((r) => r.id === focus);
  const title = focusRegion?.name ?? track.name;
  const hZoom = Math.log(pr.pxPerBeat / 6) / Math.log(1600 / 6);
  const toolClass = `pr-cursor-${pr.tool}`;

  // Velocity slider: previews while dragging, commits on release.
  const velPreview = (val: number) => {
    preview.current = { notes: me.setVelocity(work.current.notes, val), cc: work.current.cc };
    requestDraw();
    bump();
  };
  const velCommit = () => {
    if (preview.current) commit(preview.current.notes, work.current.cc, "Set Velocity", `velocity:${track.id}`);
  };
  const shownVel = preview.current ? (() => {
    const s = preview.current.notes.filter((n) => n.sel);
    return s.length ? Math.round(s.reduce((a, n) => a + n.velocity, 0) / s.length) : selVel;
  })() : selVel;

  return (
    <div className="editor piano-roll" ref={rootRef} tabIndex={-1} onKeyDown={onKeyDown}>
      <PianoRollBar pr={pr} title={title} toolBtn={toolBtn} qBtn={qBtn} qOpen={qOpen} setQOpen={setQOpen} selNotes={selNotes} shownVel={shownVel} showStepInput={showStepInput} velPreview={velPreview} velCommit={velCommit} hZoom={hZoom} commands={commands} menus={menus} />
      {qOpen && (
        <QuantizePanel
          anchor={qBtn.current}
          prefs={pr}
          onClose={() => setQOpen(false)}
          onQuantize={() => {
            quantizeNow();
            setQOpen(false);
          }}
        />
      )}
      <div className="pr-body" style={{ gridTemplateColumns: `${KEY_W}px 1fr`, gridTemplateRows: `${PR_RULER_H}px 1fr ${pr.showLane ? `5px max(${LANE_MIN_PX}px, calc((100% - ${PR_RULER_H + 5}px) * ${pr.laneShare}))` : "0 0"}` }}>
        <div className="pr-corner" />
        <div className="pr-ruler">
          <canvas ref={rulerCv} onMouseDown={onRulerDown} />
          <div className="pr-playhead" ref={playheads[0]} />
        </div>
        <div className="pr-keys">
          <canvas
            ref={keysCv}
            onMouseDown={onKeysDown}
            onMouseLeave={() => {
              hoverPitch.current = null;
              requestDraw();
            }}
          />
        </div>
        <div className="pr-notes">
          <canvas ref={notesCv} />
          <div className="pr-playhead" ref={playheads[1]} />
          {!regions.length && <div className="pr-hint">No MIDI regions on this track yet. Double-click or draw with the Pencil to add notes.</div>}
          <div
            ref={scrollRef}
            className={"pr-scroll " + toolClass}
            onScroll={requestDraw}
            onMouseDown={onNotesDown}
            onMouseMove={onNotesHover}
            onContextMenu={onNotesContext}
            onMouseLeave={() => {
              hoverPitch.current = null;
              requestDraw();
            }}
          >
            <div style={{ width: contentW, height: contentH }} />
          </div>
          <div className="pr-tag" ref={tagRef} />
        </div>
        {pr.showLane && (
          <>
            <div className="pr-split" onMouseDown={onSplitDown} title="Drag to resize the lane" />
            <div className="pr-lane-head">
              <canvas ref={scaleCv} />
              <button className="pr-lane-pick" title="Choose what the lane shows" onMouseDown={(e) => openMenuAt(e.currentTarget, laneItems())}>
                {laneShort(pr.lane)} <Icon.chevron />
              </button>
            </div>
            <div className="pr-lane" ref={laneWrap}>
              <canvas
                ref={laneCv}
                className={toolClass}
                onMouseDown={onLaneDown}
                onContextMenu={(e) => {
                  // A controller lane's right-click menu: the curve settings and ramp functions.
                  e.preventDefault();
                  const items = curveItems().slice(1);
                  if (items.length) openMenu(e.clientX, e.clientY, items);
                }}
              />
              <div className="pr-playhead" ref={playheads[2]} />
            </div>
          </>
        )}
      </div>
    </div>
  );
}
