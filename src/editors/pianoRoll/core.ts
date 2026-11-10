// The Piano Roll's working parts everything else builds on: the view and grid maths on the live
// values, drawing, sending edits, sound and the help tag. The gesture, command and menu modules beside
// this one take its result, and PianoRoll.tsx puts them together on each render.
import { invoke } from "@tauri-apps/api/core";
import { type Id, type Project, type Track } from "../../engine/project";
import { type Region } from "../../engine/regions";
import { act, actMade } from "../../engine/commands";
import { framePos } from "../../state/frameLoop";
import * as me from "../midiEdit";
import { drawGrid, drawKeys, drawLane, drawLaneScale, drawNotes, drawRuler, drawRubber, KEY_W, PR_RULER_H, setupCanvas, velocityColor, xOf, type Grid, type PrView } from "./draw";
import { getState, setState, type MidiSelection } from "../../state/store";
import { lengthBbt, samplesPerBeat, toBbt } from "../../lib/time";
import { barLines, musicalWarp, tempoMap, type Warp } from "../../lib/tempo";
import { type CurveShape } from "./ccCurves";
import { gridFloor, gridSpanAt, inMusicalTime, midiRegions, regionForTime, snapToGrid, timeBase, laneKind } from "./model";
import type { Prefs } from "./prefs";
import type { PCc, PNote } from "../midiEdit";

/** Where a press-and-drag's coordinates are relative to. */
export const local = (e: { clientX: number; clientY: number }, el: HTMLElement) => {
  const r = el.getBoundingClientRect();
  return { x: e.clientX - r.left, y: e.clientY - r.top };
};

/** A press-and-drag on the window, with the handlers removed on release. */
export function dragWith(move: (e: MouseEvent) => void, up: (e: MouseEvent) => void) {
  const m = (e: MouseEvent) => move(e);
  const u = (e: MouseEvent) => {
    window.removeEventListener("mousemove", m);
    window.removeEventListener("mouseup", u);
    up(e);
  };
  window.addEventListener("mousemove", m);
  window.addEventListener("mouseup", u);
}

export interface Work {
  /** The snapshot's region list this was built from (identity). */
  source: Region[];
  /** The MIDI regions as they are after our last edit (ahead of the snapshot until it arrives). */
  regions: Region[];
  notes: PNote[];
  cc: PCc[];
  selNotes: Set<string>;
  selCc: Set<string>;
}

/** What's live for the imperative code; refreshed on each render. */
export interface Live {
  project: Project;
  track: Track;
  focus: Id | null;
  pr: Prefs;
  themeId: string;
}

type El<T> = React.RefObject<T | null>;
type Box = { x0: number; y0: number; x1: number; y1: number };

/** The component's refs and two stable callbacks, which every part of the editor works through.
 *  The parts hold no state of their own, so building them anew on each render, as the component
 *  did with its closures, can't lose anything. */
export interface Refs {
  work: React.RefObject<Work>;
  inflight: React.RefObject<number>;
  seenSel: React.RefObject<MidiSelection | null>;
  preview: React.RefObject<{ notes: PNote[]; cc: PCc[] } | null>;
  rubber: React.RefObject<Box | null>;
  laneLine: React.RefObject<(Box & { shape?: CurveShape }) | null>;
  laneRubber: React.RefObject<Box | null>;
  cyclePreview: React.RefObject<{ start: number; end: number } | null>;
  pressedKey: React.RefObject<number | null>;
  hoverPitch: React.RefObject<number | null>;
  lastArea: React.RefObject<"notes" | "lane">;
  lastVelocity: React.RefObject<number>;
  toolArmed: React.RefObject<number>;
  pendingScroll: React.RefObject<{ left?: number; top?: number } | null>;
  sounding: React.RefObject<number | null>;
  lastPh: React.RefObject<string>;
  live: React.RefObject<Live>;
  rootRef: El<HTMLDivElement>;
  scrollRef: El<HTMLDivElement>;
  notesCv: El<HTMLCanvasElement>;
  keysCv: El<HTMLCanvasElement>;
  rulerCv: El<HTMLCanvasElement>;
  laneCv: El<HTMLCanvasElement>;
  scaleCv: El<HTMLCanvasElement>;
  laneWrap: El<HTMLDivElement>;
  playheads: El<HTMLDivElement>[];
  tagRef: El<HTMLDivElement>;
  toolBtn: El<HTMLButtonElement>;
  requestDraw: () => void;
  bump: () => void;
}

export type Core = ReturnType<typeof editorCore>;

export function editorCore(refs: Refs) {
  const { work, inflight, seenSel, preview, rubber, laneLine, laneRubber, cyclePreview, pressedKey, hoverPitch, sounding, lastPh, live, scrollRef, notesCv, keysCv, rulerCv, laneCv, scaleCv, laneWrap, playheads, tagRef, requestDraw, bump } = refs;

  // ---- geometry ---------------------------------------------------------------------------

  const view = (): PrView => {
    const el = scrollRef.current;
    const { project, pr } = live.current;
    return {
      pps: pr.pxPerBeat / samplesPerBeat(project),
      noteH: pr.noteH,
      left: el?.scrollLeft ?? 0,
      top: el?.scrollTop ?? 0,
      W: el?.clientWidth ?? 0,
      H: el?.clientHeight ?? 0,
    };
  };
  const grid = (): Grid => {
    const { project, pr } = live.current;
    const tb = timeBase(project);
    const len = me.gridLength(tb, pr.grid);
    return { bar: me.barLength(tb), beat: samplesPerBeat(project), grid: len, map: tempoMap(project), gridQ: len / me.quarterLength(tb) };
  };
  /** With tempo or signature changes, grid maths runs in musical time (see `musicalWarp`):
   *  grid lengths above are at the project's starting tempo, and warped positions keep
   *  their bar and beat. Null when one tempo and signature hold throughout. */
  const warp = (): Warp | null => {
    const p = live.current.project;
    const m = tempoMap(p);
    return m.isConstant ? null : musicalWarp(m, me.quarterLength(timeBase(p)));
  };
  /** Runs `f` on the notes in musical time, so grid-based edits follow the tempo map. */
  const musical = (notes: PNote[], f: (n: PNote[]) => PNote[]): PNote[] => inMusicalTime(warp(), notes, f);
  /** Bar lines for grid maths on warped positions (a plain bar length without changes). */
  const warpedBars = (w: Warp | null) => w?.bars ?? grid().bar;
  /** A grid-length span starting at `t`, in samples there. */
  const gridSpan = (t: number, len = grid().grid) => gridSpanAt(warp(), t, len);
  /** Snaps to the grid unless snapping is off or `free` (a modifier) is held. */
  const snapT = (t: number, free = false) => snapToGrid(grid(), warp(), live.current.pr.snap, t, free);
  /** The grid line at or before `t`, where the pencil puts a note. */
  const floorT = (t: number, free = false) => gridFloor(grid(), warp(), live.current.pr.snap, t, free);

  const shown = () => preview.current ?? work.current;

  const noteColor = (n: PNote) => {
    if (live.current.pr.color === "velocity") return velocityColor(n.velocity);
    const r = work.current.regions.find((x) => x.id === n.region);
    return r?.color ?? live.current.track.color;
  };

  // ---- drawing ----------------------------------------------------------------------------

  const draw = () => {
    const el = scrollRef.current;
    const { project, track, focus, pr, themeId } = live.current;
    if (!el || !project) return;
    const v = view();
    const g = grid();
    const { notes, cc } = shown();
    const scene = { project, regions: work.current.regions, focus, track, scale: pr.scale ? me.scaleOf(project.key) : null };
    if (notesCv.current) {
      const ctx = setupCanvas(notesCv.current, v.W, v.H);
      drawGrid(ctx, v, g, scene);
      const muted = new Set(work.current.regions.filter((r) => r.muted).map((r) => r.id));
      drawNotes(ctx, v, notes, { color: noteColor, focus, mutedRegions: muted, themeId });
      if (rubber.current) drawRubber(ctx, rubber.current, themeId);
    }
    if (keysCv.current) drawKeys(setupCanvas(keysCv.current, KEY_W, v.H), v, pressedKey.current, hoverPitch.current, themeId);
    if (rulerCv.current) drawRuler(setupCanvas(rulerCv.current, v.W, PR_RULER_H), v, g, scene, cyclePreview.current);
    const lh = laneWrap.current?.clientHeight ?? 0;
    if (pr.showLane && laneCv.current && lh > 0) {
      drawLane(setupCanvas(laneCv.current, v.W, lh), v, g, lh, {
        kind: laneKind(pr.lane),
        notes,
        cc,
        color: noteColor,
        focus,
        line: laneLine.current,
        rubber: laneRubber.current,
        themeId,
        trackColor: track.color,
        ends: new Map(work.current.regions.map((r) => [r.id, r.start + r.length])),
      });
      if (scaleCv.current) drawLaneScale(setupCanvas(scaleCv.current, KEY_W, lh), lh, laneKind(pr.lane));
    }
    lastPh.current = "";
    placePlayhead();
  };

  const placePlayhead = (pos: number = framePos()) => {
    const v = view();
    const x = xOf(v, pos);
    const vis = x >= -1 && x <= v.W + 1;
    const key = vis ? x.toFixed(1) : "hidden";
    if (key === lastPh.current) return;
    lastPh.current = key;
    for (const r of playheads) {
      if (!r.current) continue;
      r.current.style.transform = `translate3d(${x.toFixed(1)}px,0,0)`;
      r.current.style.display = vis ? "block" : "none";
    }
  };

  // ---- committing edits -------------------------------------------------------------------

  /** Shares the selection with the Event List. */
  const publishSelection = () => {
    const { selNotes, selCc } = work.current;
    const prev = getState().midiSelection;
    if (prev.notes.length === selNotes.size && prev.cc.length === selCc.size && prev.notes.every((k) => selNotes.has(k)) && prev.cc.every((k) => selCc.has(k))) return;
    seenSel.current = { notes: [...selNotes], cc: [...selCc] };
    setState({ midiSelection: seenSel.current });
  };

  const setSelection = (notes: PNote[], cc: PCc[] = work.current.cc) => {
    work.current = { ...work.current, notes, cc, selNotes: me.selectionKeys(notes), selCc: me.selectionKeys(cc) };
    publishSelection();
    preview.current = null;
    requestDraw();
    bump();
  };

  /** Sends an edited list to the engine as one undo step. */
  const commit = (notes: PNote[], cc: PCc[], label: string, key?: string) => {
    preview.current = null;
    const m = tempoMap(live.current.project);
    const { edits, selNotes, selCc } = me.toEdits(notes, cc, work.current.regions, m.isConstant ? grid().bar : barLines(m));
    if (!edits.length) {
      setSelection(notes, cc);
      return;
    }
    const next = me.applyEdits(work.current.regions, edits);
    const w = me.fromRegions(next, selNotes, selCc);
    work.current = { ...work.current, regions: next, notes: w.notes, cc: w.cc, selNotes, selCc };
    publishSelection();
    requestDraw();
    bump();
    inflight.current++;
    act("set_midi_content", { edits, label, key: key ?? null }).finally(() => {
      if (--inflight.current === 0) {
        resync();
        requestDraw();
        bump();
      }
    });
  };

  /** Rebuilds the working list from the store, after an edit that went through another command. */
  const resync = () => {
    const t = getState().snap?.project.tracks.find((x) => x.id === live.current.track.id);
    if (!t) return;
    const regs = midiRegions(t);
    const w = me.fromRegions(regs, work.current.selNotes, work.current.selCc);
    work.current = { ...work.current, source: t.regions, regions: regs, notes: w.notes, cc: w.cc };
  };

  /** The region a note or event at `t` goes in: one covering it (the edited one first), else
   *  one ending or starting within a bar of it, which then grows to hold it. Null when there
   *  is none, so a new region is made there. */
  const regionFor = (t: number): Id | null => regionForTime(work.current.regions, live.current.focus, grid().bar, t);

  /** Commits notes that may sit in a region that doesn't exist yet (id -1): makes a one-bar
   *  region at `t` first. Both go in one undo step (a shared checkpoint key). */
  const commitWithRegion = async (notes: PNote[], t: number, label: string) => {
    if (!notes.some((n) => n.region === -1)) return commit(notes, work.current.cc, label);
    const m = tempoMap(live.current.project);
    const bar = m.barAt(Math.max(0, t));
    const start = Math.round(m.barStart(bar));
    const key = `new-region:${Date.now()}`;
    const ids = await actMade("create_midi_region", { track: live.current.track.id, start, length: Math.round(m.barStart(bar + 1)) - start, key });
    if (!ids?.length) {
      // Nothing was made (a frozen track, say): drop the note drawn in advance.
      preview.current = null;
      setSelection(work.current.notes);
      return;
    }
    setState({ selectedRegions: ids, focus: "regions" });
    resync();
    commit(
      notes.map((n) => (n.region === -1 ? { ...n, region: ids[0] } : n)),
      work.current.cc,
      label,
      key,
    );
  };

  // ---- sound ------------------------------------------------------------------------------

  const play = (pitch: number | null, velocity = 100) => {
    // MIDI Out off silences edits, but not the keyboard column (a pressed key), which is
    // played on purpose. Note-offs always go out, so nothing hangs.
    if (!live.current.pr.midiOut && pitch !== null && pressedKey.current === null) return;
    if (sounding.current !== null) invoke("midi_event", { data: [0x80, sounding.current, 0] }).catch(() => {});
    sounding.current = pitch;
    if (pitch !== null) invoke("midi_event", { data: [0x90, pitch, Math.max(1, Math.min(127, Math.round(velocity)))] }).catch(() => {});
  };

  const ensureSelectedTrack = () => {
    if (getState().snap?.selected !== live.current.track.id) act("select_track", { id: live.current.track.id });
  };

  // ---- help tag ---------------------------------------------------------------------------

  const showTag = (text: string | null, x = 0, y = 0) => {
    const el = tagRef.current;
    if (!el) return;
    if (text === null) {
      el.style.display = "none";
      return;
    }
    el.textContent = text;
    el.style.display = "block";
    el.style.transform = `translate(${Math.round(x + 14)}px, ${Math.round(y + 14)}px)`;
  };
  const posLabel = (t: number) => {
    const b = toBbt(live.current.project, t);
    return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
  };
  /** A length from `start`, counted through the tempo map. */
  const lenLabel = (len: number, start = 0) => {
    const b = lengthBbt(live.current.project, start, len);
    return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
  };

  const showLaneTag = (text: string, ev: MouseEvent) => {
    const el = scrollRef.current!;
    const { x } = local(ev, el);
    showTag(text, x, el.clientHeight - 30);
  };

  /** Controller events live in regions; say so when one is drawn where there's none. */
  const noRegionHere = () => setState({ toast: { text: "Controller events go in a MIDI region: draw over a region, or a note first to make one", at: Date.now(), ms: 4000 } });

  const selectRegionOf = (n: PNote) => {
    if (n.region !== live.current.focus) setState({ selectedRegions: [n.region], focus: "regions" });
  };

  return { ...refs, view, grid, warp, warpedBars, musical, gridSpan, snapT, floorT, shown, draw, placePlayhead, setSelection, commit, resync, regionFor, commitWithRegion, play, ensureSelectedTrack, showTag, posLabel, lenLabel, showLaneTag, noRegionHere, selectRegionOf };
}
