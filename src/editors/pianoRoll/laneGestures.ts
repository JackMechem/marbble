// Pressing and dragging in the Piano Roll's velocity and controller lane: stalks, freehand and line
// drawing, moving and rubber-banding events.
import { type CcKind } from "../../engine/regions";
import * as me from "../midiEdit";
import { laneValue, laneY, timeAt, xOf, type PrView } from "./draw";
import { densityStep, drawCcCurve, nearestEvent } from "./ccCurves";
import { local, type Core } from "./core";
import { DRAG_THRESHOLD_PX, dragWith, pastDragThreshold } from "../../ui/pointerDrag";
import { ccDotAt, laneKind, laneShort, stalkAt, timeBase, type Tool } from "./model";
import type { PCc, PNote } from "../midiEdit";
import { clamp } from "../../lib/clamp";

/** A press in a controller lane: the event, where (in the lane `H` px high) and when (`t0`),
 *  the lane's kind and top value, and its events at the press. */
interface CcPress {
  e: React.MouseEvent;
  x: number;
  y: number;
  H: number;
  v: PrView;
  kind: CcKind;
  max: number;
  all: PCc[];
  t0: number;
}

export function laneGestures(ed: Core) {
  const { view, snapT, setSelection, commit, regionFor, ensureSelectedTrack, showTag, showLaneTag, noRegionHere, posLabel, selectRegionOf, requestDraw, rootRef, laneCv, work, preview, laneLine, laneRubber, lastArea, lastVelocity, live } = ed;

  const onLaneDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    rootRef.current?.focus({ preventScroll: true });
    lastArea.current = "lane";
    ensureSelectedTrack();
    const cv = laneCv.current!;
    const H = cv.clientHeight;
    const { x, y } = local(e, cv);
    const v = view();
    const kind = laneKind(live.current.pr.lane);
    let tool = live.current.pr.tool;
    // As in the note area, Mod-click is the Pencil, but here the Line tool is kept (it draws
    // controller ramps, while over notes it acts as the Pointer) and Shift turns the Pencil
    // into a line.
    if ((e.ctrlKey || e.metaKey) && tool === "pointer") tool = "pencil";
    if (tool === "pencil" && e.shiftKey) tool = "line";
    if (!kind) return velocityLaneDown(x, y, H, v, tool);
    return ccLaneDown(e, x, y, H, v, tool, kind);
  };

  // ---- the velocity lane ------------------------------------------------------------------

  /** A press at `x`, `y` in the velocity lane `H` px high: on a stalk (but with the Pencil or
   *  Line) drags the selection's velocities, the Pencil draws them freehand, and otherwise a
   *  line ramps the velocities under it. */
  const velocityLaneDown = (x: number, y: number, H: number, v: PrView, tool: Tool) => {
    const notes = work.current.notes;
    const hit = stalkAt(v, notes, x);
    if (hit && tool !== "pencil" && tool !== "line") return dragStalk(hit, H);
    if (tool === "pencil") return drawVelocities(x, y, H);
    lineDrag(x, y, (l) => {
      const vv = view();
      const next = me.rampVelocity(work.current.notes, timeAt(vv, l.x0), laneValue(H, l.y0, 127), timeAt(vv, l.x1), laneValue(H, l.y1, 127));
      commit(next, work.current.cc, "Velocity Ramp");
    });
  };

  /** Selects the note of stalk `h` (unless it's in the selection) and sets the selection's
   *  velocities by as much as the pointer moves it from its own. */
  const dragStalk = (h: PNote, H: number) => {
    const cv = laneCv.current!;
    const notes = work.current.notes;
    const base = h.sel ? notes : notes.map((n) => (n === h ? { ...n, sel: true } : n.sel ? { ...n, sel: false } : n));
    const grabbed = base[notes.indexOf(h)];
    setSelection(base);
    selectRegionOf(grabbed);
    let moved = false;
    dragWith(
      (ev) => {
        moved = true;
        const ly = local(ev, cv).y;
        const val = laneValue(H, ly, 127);
        const next = me.offsetVelocity(base, val - grabbed.velocity);
        const g2 = next[base.indexOf(grabbed)];
        showLaneTag(`Velocity ${g2.velocity}`, ev);
        preview.current = { notes: next, cc: work.current.cc };
        requestDraw();
      },
      () => {
        showTag(null);
        if (!moved || !preview.current) return;
        lastVelocity.current = preview.current.notes[base.indexOf(grabbed)].velocity;
        commit(preview.current.notes, work.current.cc, "Note Velocity");
      },
    );
  };

  /** The Pencil, freehand: notes the pointer passes get the value under it. */
  const drawVelocities = (x: number, y: number, H: number) => {
    const cv = laneCv.current!;
    let last = { x, y };
    let cur = work.current.notes;
    const apply = (a: { x: number; y: number }, b: { x: number; y: number }) => {
      const vv = view();
      const [l, r] = a.x < b.x ? [a, b] : [b, a];
      cur = me.rampVelocity(cur, timeAt(vv, l.x - 2), laneValue(H, l.y, 127), timeAt(vv, r.x + 2), laneValue(H, r.y, 127));
      preview.current = { notes: cur, cc: work.current.cc };
      requestDraw();
    };
    apply(last, last);
    dragWith(
      (ev) => {
        const p = local(ev, cv);
        apply(last, p);
        last = p;
      },
      () => preview.current && commit(preview.current.notes, work.current.cc, "Draw Velocity"),
    );
  };

  /** A line from `x`, `y` to the pointer, shown while dragging and handed to `done` on release
   *  (not for a click). */
  const lineDrag = (x: number, y: number, done: (l: { x0: number; y0: number; x1: number; y1: number }) => void) => {
    const cv = laneCv.current!;
    let moved = false;
    dragWith(
      (ev) => {
        const p = local(ev, cv);
        if (!moved && Math.hypot(p.x - x, p.y - y) < DRAG_THRESHOLD_PX) return;
        moved = true;
        // A controller line shows the curve it will draw.
        const shape = laneKind(live.current.pr.lane) ? live.current.pr.curve : undefined;
        laneLine.current = { x0: x, y0: y, x1: p.x, y1: Math.max(0, Math.min(cv.clientHeight, p.y)), shape };
        requestDraw();
      },
      () => {
        const l = laneLine.current;
        laneLine.current = null;
        requestDraw();
        if (moved && l) done(l);
      },
    );
  };

  // ---- a controller lane ------------------------------------------------------------------

  /** A press in a controller lane, by tool: the Eraser (or Option on an event) erases, the
   *  Pencil draws freehand, the Line tool draws a curve; with the Pointer, a press on an event
   *  drags the selection, a double-click adds one and a drag on empty space rubber-bands. */
  const ccLaneDown = (e: React.MouseEvent, x: number, y: number, H: number, v: PrView, tool: Tool, kind: CcKind) => {
    const all = work.current.cc;
    const hit = ccDotAt(v, all, kind, x, y, H);
    const press: CcPress = { e, x, y, H, v, kind, max: me.ccMax(kind), all, t0: timeAt(v, x) };
    if (tool === "eraser" || (hit && e.altKey && tool === "pointer")) return eraseCc(press);
    if (tool === "pencil") return drawCc(press);
    if (tool === "line") return lineCc(press);
    if (hit) return dragCc(press, hit);
    if (e.detail === 2) return addCc(press);
    rubberBandCc(press);
  };

  /** Everything of this kind between the press and the pointer goes. */
  const eraseCc = ({ v, kind, all, t0 }: CcPress) => {
    const cv = laneCv.current!;
    const pad = 4 / v.pps;
    // Only in the region the press is in, as the Pencil draws into one region.
    const inRegion = regionFor(t0) ?? undefined;
    const erase = (b: number) => {
      const next = me.eraseCc(all, kind, Math.min(t0, b) - pad, Math.max(t0, b) + pad, inRegion);
      preview.current = { notes: work.current.notes, cc: next };
      requestDraw();
    };
    erase(t0);
    dragWith(
      (ev) => erase(timeAt(view(), local(ev, cv).x)),
      () => {
        const next = preview.current?.cc;
        if (next && next.length !== all.length) commit(work.current.notes, next, "Delete Controller Events");
        else {
          preview.current = null;
          requestDraw();
        }
      },
    );
  };

  /** The Pencil: draws events freehand along the pointer, into the region pressed in. */
  const drawCc = ({ y, H, kind, max, all, t0 }: CcPress) => {
    const cv = laneCv.current!;
    const rid = regionFor(t0);
    if (rid === null) return noRegionHere();
    let last = { t: t0, v: laneValue(H, y, max) };
    let cur = me.drawCcLine(all, rid, kind, last.t, last.v, last.t, last.v, 1);
    preview.current = { notes: work.current.notes, cc: cur };
    requestDraw();
    // About one point per 1/64 note keeps freehand curves smooth but light.
    const step = me.quarterLength(timeBase(live.current.project)) / 16;
    dragWith(
      (ev) => {
        const p = local(ev, cv);
        const t = timeAt(view(), p.x);
        const val = laneValue(H, p.y, max);
        cur = me.drawCcLine(cur, rid, kind, last.t, last.v, t, val, step);
        last = { t, v: val };
        preview.current = { notes: work.current.notes, cc: cur };
        showLaneTag(`${laneShort(live.current.pr.lane)} ${val}`, ev);
        requestDraw();
      },
      () => {
        showTag(null);
        commit(work.current.notes, cur, "Draw Controller");
      },
    );
  };

  /** The Line tool: a curve of the lane's shape and density between the ends of a drag. */
  const lineCc = ({ x, y, H, kind, max }: CcPress) =>
    lineDrag(x, y, (l) => {
      const vv = view();
      // An end on an existing event joins it: the line starts or stops at its time and value.
      const at = (px: number, py: number) => {
        const e = nearestEvent(work.current.cc, kind, px, py, (t) => xOf(vv, t), (val) => laneY(H, val, max));
        return e ? { t: e.start, v: e.value } : { t: timeAt(vv, px), v: laneValue(H, py, max) };
      };
      const [a, b] = [at(l.x0, l.y0), at(l.x1, l.y1)];
      const rid = regionFor(Math.min(a.t, b.t));
      if (rid === null) return noRegionHere();
      const { pr, project } = live.current;
      const step = densityStep(me.quarterLength(timeBase(project)), pr.density);
      commit(work.current.notes, drawCcCurve(work.current.cc, rid, kind, a.t, a.v, b.t, b.v, step, pr.curve), "Controller Line");
    });

  /** Selects event `h` (Shift toggles it), then drags the selected events in time and value. */
  const dragCc = ({ e, H, v, max, all }: CcPress, h: PCc) => {
    let base: PCc[];
    if (e.shiftKey) base = all.map((c) => (c === h ? { ...c, sel: !c.sel } : c));
    else if (!h.sel) base = all.map((c) => (c === h ? { ...c, sel: true } : c.sel ? { ...c, sel: false } : c));
    else base = all;
    const grabbed = base[all.indexOf(h)];
    setSelection(work.current.notes, base);
    if (!grabbed.sel) return;
    const sx = e.clientX;
    const sy = e.clientY;
    let moved = false;
    dragWith(
      (ev) => {
        if (!moved && !pastDragThreshold(ev.clientX - sx, ev.clientY - sy)) return;
        moved = true;
        const free = ev.ctrlKey || ev.metaKey;
        const dt = Math.round(snapT(grabbed.start + (ev.clientX - sx) / v.pps, free) - grabbed.start);
        // The lane's value range spans its height less 6 px margins top and bottom (laneY).
        const dv = Math.round(((sy - ev.clientY) / (H - 12)) * max);
        const next = base.map((c) => (c.sel ? { ...c, start: Math.max(0, c.start + dt), value: clamp(c.value + dv, 0, max) } : c));
        const g2 = next[base.indexOf(grabbed)];
        showLaneTag(`${laneShort(live.current.pr.lane)} ${g2.value}   ${posLabel(g2.start)}`, ev);
        preview.current = { notes: work.current.notes, cc: next };
        requestDraw();
      },
      () => {
        showTag(null);
        if (moved && preview.current) commit(work.current.notes, preview.current.cc, "Move Controller Events");
      },
    );
  };

  /** A double-click on empty space: an event there (snapped unless Option is held), selected. */
  const addCc = ({ e, y, H, kind, max, all, t0 }: CcPress) => {
    const t = Math.max(0, snapT(t0, e.altKey));
    const rid = regionFor(t);
    if (rid === null) return noRegionHere();
    const next = [...all.map((c) => (c.sel ? { ...c, sel: false } : c)), { region: rid, start: t, kind, value: laneValue(H, y, max), sel: true }];
    commit(work.current.notes, next.sort((a, b) => a.start - b.start), "Create Controller Event");
  };

  /** A rubber band over the events of this kind selects them (Shift adds to the selection); a
   *  click on empty space deselects. */
  const rubberBandCc = ({ e, x, y, H, kind, max, all }: CcPress) => {
    const cv = laneCv.current!;
    const add = e.shiftKey;
    let moved = false;
    dragWith(
      (ev) => {
        const p = local(ev, cv);
        if (!moved && Math.hypot(p.x - x, p.y - y) < DRAG_THRESHOLD_PX) return;
        moved = true;
        laneRubber.current = { x0: x, y0: y, x1: p.x, y1: p.y };
        const vv = view();
        const [ta, tb] = [timeAt(vv, Math.min(x, p.x)), timeAt(vv, Math.max(x, p.x))];
        const [va, vb] = [laneValue(H, Math.max(y, p.y), max), laneValue(H, Math.min(y, p.y), max)];
        const next = all.map((c) => {
          const inside = me.sameKind(c.kind, kind) && c.start >= ta && c.start <= tb && c.value >= va && c.value <= vb;
          const sel = add ? c.sel || inside : inside;
          return sel === c.sel ? c : { ...c, sel };
        });
        preview.current = { notes: work.current.notes, cc: next };
        requestDraw();
      },
      () => {
        laneRubber.current = null;
        if (moved && preview.current) setSelection(work.current.notes, preview.current.cc);
        else if (!add) setSelection(work.current.notes, all.map((c) => (c.sel ? { ...c, sel: false } : c)));
        else requestDraw();
      },
    );
  };

  return { onLaneDown };
}
