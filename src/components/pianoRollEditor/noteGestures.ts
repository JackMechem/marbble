// Pressing and dragging in the Piano Roll's note area, keyboard column and ruler. A drag draws a preview
// of the edited list, and releasing commits it as one undo step.
import { act } from "../../api";
import * as me from "../../midiEdit";
import { fmt } from "../../keymap";
import { CYCLE_H, KEY_W, noteName, pitchAt, timeAt, xOf } from "../../pianoRollDraw";
import { openMenu } from "../Menu";
import { ccClipboard, noteClipboard, type Commands } from "./commands";
import { dragWith, local, type Core } from "./core";
import type { Menus } from "./menus";
import { setPrefs } from "./prefs";
import { cycleGrab, keyAtPoint, noteAt } from "../../pianoRollModel";
import type { PNote } from "../../midiEdit";

export function noteGestures(ed: Core, c: Commands, m: Menus) {
  const { view, grid, warp, musical, gridSpan, snapT, floorT, setSelection, commit, commitWithRegion, regionFor, play, ensureSelectedTrack, showTag, posLabel, lenLabel, selectRegionOf, requestDraw, rootRef, scrollRef, keysCv, rulerCv, work, preview, rubber, cyclePreview, pressedKey, hoverPitch, lastArea, lastVelocity, live } = ed;
  const { cut, copy, paste, duplicate, deleteSelected } = c;
  const { functionItems } = m;

  /** The note under a point, and which part of it. */
  const hitNote = (notes: PNote[], x: number, y: number) => noteAt(view(), notes, x, y);

  // ---- note area --------------------------------------------------------------------------

  const onNotesDown = (e: React.MouseEvent) => {
    rootRef.current?.focus({ preventScroll: true });
    lastArea.current = "notes";
    const el = scrollRef.current!;
    const { x, y } = local(e, el);
    if (x > el.clientWidth || y > el.clientHeight) return; // the scrollbars
    if (e.button !== 0) return;
    e.preventDefault();
    ensureSelectedTrack();
    const v = view();
    let tool = live.current.pr.tool;
    // ⌘/Ctrl-click is the Pencil, like Logic's secondary tool.
    if ((e.ctrlKey || e.metaKey) && (tool === "pointer" || tool === "line")) tool = "pencil";
    if (tool === "line") tool = "pointer";
    const notes = work.current.notes;
    const hit = hitNote(notes, x, y);

    if (tool === "eraser") return sweep(() => null, "Delete Notes", hit?.n);
    if (tool === "mute") {
      const muted = hit ? !hit.n.muted : true;
      // On a selected note the whole selection toggles: muted unless all of it already is.
      if (hit?.n.sel) return commit(me.toggleMute(notes), work.current.cc, notes.every((n) => !n.sel || n.muted) ? "Unmute Notes" : "Mute Notes");
      return sweep((n) => ({ ...n, muted }), muted ? "Mute Notes" : "Unmute Notes", hit?.n);
    }
    if (hit && tool === "scissors") {
      const only = new Set(hit.n.sel ? notes.filter((n) => n.sel) : [hit.n]);
      const at = snapT(timeAt(v, x), e.altKey);
      return commit(me.splitAt(notes, at, only), work.current.cc, "Split Notes");
    }
    if (hit && tool === "glue") {
      const base = hit.n.sel ? notes : notes.map((n) => (n === hit.n ? { ...n, sel: true } : n));
      return commit(me.glue(base), work.current.cc, "Glue Notes");
    }

    if (hit) {
      selectRegionOf(hit.n);
      const wasSelected = hit.n.sel;
      let base: PNote[];
      if (e.shiftKey) base = notes.map((n) => (n === hit.n ? { ...n, sel: !n.sel } : n));
      else if (!hit.n.sel) base = notes.map((n) => (n === hit.n ? (n.sel ? n : { ...n, sel: true }) : n.sel ? { ...n, sel: false } : n));
      else base = notes;
      const grabbed = base[notes.indexOf(hit.n)];
      setSelection(base);
      if (!grabbed.sel) return;
      lastVelocity.current = grabbed.velocity;
      if (tool === "velocity") return velocityDrag(e, base, grabbed);
      play(grabbed.pitch, grabbed.velocity);
      return noteDrag(e, hit.zone === "body" ? "move" : hit.zone, base, grabbed, wasSelected && !e.shiftKey);
    }

    // Empty space.
    if (tool === "pencil" || (tool === "pointer" && e.detail === 2)) return pencil(e, x, y, tool === "pencil");
    if (tool === "pointer" || tool === "velocity" || tool === "scissors" || tool === "glue") return rubberBand(e, x, y);
  };

  /** Moving, copying (Alt) or resizing the selected notes. */
  const noteDrag = (e0: React.MouseEvent, kind: "move" | "left" | "right", base: PNote[], grabbed: PNote, reselectOnClick: boolean) => {
    const sx = e0.clientX;
    const sy = e0.clientY;
    const v = view();
    const g = grid();
    let moved = false;
    let copying = false;
    let lastPitch = grabbed.pitch;
    const el = scrollRef.current!;
    dragWith(
      (ev) => {
        if (!moved && Math.abs(ev.clientX - sx) < 3 && Math.abs(ev.clientY - sy) < 3) return;
        moved = true;
        const dxT = (ev.clientX - sx) / v.pps;
        const { x, y } = local(ev, el);
        let notes: PNote[];
        if (kind === "move") {
          const free = ev.ctrlKey || ev.metaKey;
          const target = snapT(grabbed.start + dxT, free);
          const dp = -Math.round((ev.clientY - sy) / v.noteH);
          // Across tempo changes the selection moves in musical time, keeping its rhythm.
          const w = warp();
          const shifted = w ? musical(base, (n) => me.nudge(n, w.toV(target) - w.toV(grabbed.start), dp)) : me.nudge(base, Math.round(target - grabbed.start), dp);
          copying = ev.altKey;
          notes = copying ? [...base.map((n) => (n.sel ? { ...n, sel: false } : n)), ...shifted.filter((n) => n.sel)] : shifted;
          const g2 = shifted[base.indexOf(grabbed)];
          if (g2.pitch !== lastPitch) {
            lastPitch = g2.pitch;
            play(g2.pitch, g2.velocity);
          }
          showTag(`${noteName(g2.pitch)}   ${posLabel(g2.start)}${copying ? "   copy" : ""}`, x, y);
        } else {
          const free = ev.ctrlKey || ev.metaKey || ev.altKey;
          const min = free ? 1 : Math.max(1, Math.min(g.grid, grabbed.length));
          if (kind === "right") {
            const end = snapT(grabbed.start + grabbed.length + dxT, free);
            const dl = end - (grabbed.start + grabbed.length);
            notes = base.map((n) => (n.sel ? { ...n, length: Math.max(Math.min(min, n.length), Math.round(n.length + dl)) } : n));
          } else {
            const ds = snapT(grabbed.start + dxT, free) - grabbed.start;
            notes = base.map((n) => {
              if (!n.sel) return n;
              const end = n.start + n.length;
              const s = Math.round(Math.max(0, Math.min(end - Math.min(min, n.length), n.start + ds)));
              return { ...n, start: s, length: end - s };
            });
          }
          const g2 = notes[base.indexOf(grabbed)];
          showTag(`${kind === "right" ? "Length" : "Start"}   ${kind === "right" ? lenLabel(g2.length, g2.start) : posLabel(g2.start)}`, x, y);
        }
        preview.current = { notes, cc: work.current.cc };
        requestDraw();
      },
      () => {
        play(null);
        showTag(null);
        if (!moved) {
          preview.current = null;
          // A click on a note in a selection selects just that note.
          if (reselectOnClick) setSelection(base.map((n) => (n === grabbed ? n : n.sel ? { ...n, sel: false } : n)));
          requestDraw();
          return;
        }
        const notes = preview.current?.notes ?? base;
        commit(notes, work.current.cc, kind === "move" ? (copying ? "Copy Notes" : "Move Notes") : "Resize Notes");
      },
    );
  };

  /** The Velocity tool: drag up or down on a note to change the selection's velocities. */
  const velocityDrag = (e0: React.MouseEvent, base: PNote[], grabbed: PNote) => {
    const sy = e0.clientY;
    const el = scrollRef.current!;
    let moved = false;
    dragWith(
      (ev) => {
        const delta = Math.round((sy - ev.clientY) / 2);
        if (!moved && delta === 0) return;
        moved = true;
        const notes = me.offsetVelocity(base, delta);
        const g2 = notes[base.indexOf(grabbed)];
        const { x, y } = local(ev, el);
        showTag(`Velocity ${g2.velocity}`, x, y);
        preview.current = { notes, cc: work.current.cc };
        requestDraw();
      },
      () => {
        showTag(null);
        if (!moved || !preview.current) return;
        const g2 = preview.current.notes[base.indexOf(grabbed)];
        lastVelocity.current = g2.velocity;
        play(g2.pitch, g2.velocity);
        window.setTimeout(() => play(null), 180);
        commit(preview.current.notes, work.current.cc, "Note Velocity");
      },
    );
  };

  /** Eraser and Mute: apply `f` to every note the pointer passes over (null deletes). */
  const sweep = (f: (n: PNote) => PNote | null, label: string, first?: PNote) => {
    const el = scrollRef.current!;
    const base = work.current.notes;
    const hits = new Set<PNote>(first ? [first] : []);
    const update = () => {
      const notes: PNote[] = [];
      for (const n of base) {
        if (!hits.has(n)) notes.push(n);
        else {
          const r = f(n);
          if (r) notes.push(r);
        }
      }
      preview.current = { notes, cc: work.current.cc };
      requestDraw();
    };
    update();
    dragWith(
      (ev) => {
        const { x, y } = local(ev, el);
        const h = hitNote(base, x, y);
        if (h && !hits.has(h.n)) {
          hits.add(h.n);
          update();
        }
      },
      () => {
        if (!hits.size || !preview.current) {
          preview.current = null;
          requestDraw();
          return;
        }
        commit(preview.current.notes, work.current.cc, label);
      },
    );
  };

  /** Creates a note of the grid length at the press; dragging right sets its length. */
  const pencil = (e0: React.MouseEvent, x: number, y: number, drag: boolean) => {
    const v = view();
    const start = Math.max(0, floorT(timeAt(v, x), e0.altKey));
    const pitch = pitchAt(v, y);
    const rid = regionFor(start) ?? -1;
    const note: PNote = { region: rid, start, length: gridSpan(start), pitch, velocity: lastVelocity.current, muted: false, sel: true };
    const base = me.selectNone(work.current.notes);
    preview.current = { notes: [...base, note], cc: work.current.cc };
    requestDraw();
    play(pitch, note.velocity);
    const sx = e0.clientX;
    const el = scrollRef.current!;
    dragWith(
      (ev) => {
        if (!drag) return;
        const free = ev.altKey || ev.ctrlKey || ev.metaKey;
        const end = snapT(start + gridSpan(start) + (ev.clientX - sx) / v.pps, free);
        note.length = Math.round(Math.max(free ? 1 : gridSpan(start), end - start));
        preview.current = { notes: [...base, { ...note }], cc: work.current.cc };
        const { x: lx, y: ly } = local(ev, el);
        showTag(`Length   ${lenLabel(note.length, start)}`, lx, ly);
        requestDraw();
      },
      () => {
        play(null);
        showTag(null);
        commitWithRegion([...base, { ...note }], start, "Create Note");
      },
    );
  };

  const rubberBand = (e0: React.MouseEvent, x0: number, y0: number) => {
    const el = scrollRef.current!;
    const add = e0.shiftKey;
    const base = work.current.notes;
    const v0 = view();
    // Positions in content coordinates, so scrolling during the drag keeps the anchor.
    const ax = x0 + v0.left;
    const ay = y0 + v0.top;
    let moved = false;
    dragWith(
      (ev) => {
        const { x, y } = local(ev, el);
        const v = view();
        if (!moved && Math.hypot(x + v.left - ax, y + v.top - ay) < 3) return;
        moved = true;
        rubber.current = { x0: ax - v.left, y0: ay - v.top, x1: x, y1: y };
        const notes = me.selectRect(base, timeAt(v, ax - v.left), timeAt(v, x), pitchAt(v, ay - v.top), pitchAt(v, y), add);
        preview.current = { notes, cc: work.current.cc };
        requestDraw();
      },
      () => {
        rubber.current = null;
        if (moved && preview.current) setSelection(preview.current.notes);
        else if (!add) setSelection(me.selectNone(base));
        else requestDraw();
      },
    );
  };

  const onNotesHover = (e: React.MouseEvent) => {
    if (e.buttons) return;
    const el = scrollRef.current!;
    const { x, y } = local(e, el);
    const p = pitchAt(view(), y);
    if (p !== hoverPitch.current) {
      hoverPitch.current = p;
      requestDraw();
    }
    const tool = live.current.pr.tool;
    if (tool !== "pointer" && tool !== "line" && tool !== "pencil") {
      el.style.cursor = "";
      return;
    }
    const h = hitNote(work.current.notes, x, y);
    el.style.cursor = h && h.zone !== "body" ? "ew-resize" : "";
  };

  // ---- note context menu ------------------------------------------------------------------

  const onNotesContext = (e: React.MouseEvent) => {
    e.preventDefault();
    rootRef.current?.focus({ preventScroll: true });
    const el = scrollRef.current!;
    const { x, y } = local(e, el);
    const hit = hitNote(work.current.notes, x, y);
    if (hit && !hit.n.sel) setSelection(work.current.notes.map((n) => (n === hit.n ? { ...n, sel: true } : n.sel ? { ...n, sel: false } : n)));
    const any = work.current.notes.some((n) => n.sel);
    openMenu(e.clientX, e.clientY, [
      { label: "Cut", hint: fmt("Mod+X"), disabled: !any, onClick: () => cut() },
      { label: "Copy", hint: fmt("Mod+C"), disabled: !any, onClick: () => copy() },
      { label: "Paste", hint: fmt("Mod+V"), disabled: !noteClipboard.length && !ccClipboard.length, onClick: () => paste() },
      { label: "Duplicate", hint: fmt("Mod+D"), disabled: !any, onClick: () => duplicate() },
      { label: "Delete", hint: fmt("Backspace"), disabled: !any, onClick: () => deleteSelected() },
      { separator: true },
      ...functionItems(),
    ]);
  };

  // ---- keyboard (piano) ---------------------------------------------------------------------

  const onKeysDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    rootRef.current?.focus({ preventScroll: true });
    ensureSelectedTrack();
    const cv = keysCv.current!;
    const keyAt = (ev: { clientX: number; clientY: number }) => {
      const { x, y } = local(ev, cv);
      return keyAtPoint(view(), x, y, KEY_W);
    };
    const first = keyAt(e);
    // A click on a key also selects that pitch's notes (Shift adds), as in Logic.
    const notes = work.current.notes;
    setSelection(notes.map((n) => ({ ...n, sel: n.pitch === first.p || (e.shiftKey && n.sel) })));
    const press = (k: { p: number; vel: number }) => {
      pressedKey.current = k.p;
      play(k.p, k.vel);
      requestDraw();
    };
    press(first);
    dragWith(
      (ev) => {
        const k = keyAt(ev);
        if (k.p !== pressedKey.current) press(k);
      },
      () => {
        pressedKey.current = null;
        play(null);
        requestDraw();
      },
    );
  };

  // ---- ruler ------------------------------------------------------------------------------

  const onRulerDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    rootRef.current?.focus({ preventScroll: true });
    const cv = rulerCv.current!;
    const { x, y } = local(e, cv);
    const v = view();
    const project = live.current.project;
    const at = timeAt(v, x);
    if (y > CYCLE_H) {
      const locate = (t: number) => act("locate", { pos: Math.max(0, Math.round(snapT(t))) });
      locate(at);
      dragWith(
        (ev) => locate(timeAt(view(), local(ev, cv).x)),
        () => {},
      );
      return;
    }
    // The cycle strip: drag to set the locators, drag the range or its ends, click to toggle.
    const c = project.cycle;
    const has = c.end > c.start;
    const sx = xOf(v, c.start);
    const ex = xOf(v, c.end);
    const mode = cycleGrab(has, x, sx, ex);
    const g = grid();
    let moved = false;
    dragWith(
      (ev) => {
        const cx = local(ev, cv).x;
        if (!moved && Math.abs(cx - x) < 3) return;
        moved = true;
        const cur = timeAt(view(), cx);
        let s = c.start;
        let en = c.end;
        if (mode === "new") {
          s = snapT(Math.min(at, cur));
          en = snapT(Math.max(at, cur));
          if (en <= s) en = s + g.beat;
        } else if (mode === "move") {
          const d = snapT(c.start + (cur - at)) - c.start;
          s = c.start + d;
          en = c.end + d;
        } else if (mode === "start") s = Math.min(snapT(cur), c.end - g.grid);
        else en = Math.max(snapT(cur), c.start + g.grid);
        cyclePreview.current = { start: Math.max(0, s), end: Math.max(0, en) };
        requestDraw();
      },
      () => {
        const p = cyclePreview.current;
        cyclePreview.current = null;
        if (moved && p) act("set_cycle", { enabled: true, start: Math.round(p.start), end: Math.round(p.end) });
        else if (mode !== "new") act("set_cycle", { enabled: !c.enabled, start: c.start, end: c.end });
        requestDraw();
      },
    );
  };

  // ---- the divider above the lane -------------------------------------------------------------

  const onSplitDown = (e: React.MouseEvent) => {
    e.preventDefault();
    const sy = e.clientY;
    const h0 = live.current.pr.laneH;
    dragWith(
      (ev) => setPrefs({ laneH: Math.round(Math.max(40, Math.min(420, h0 + sy - ev.clientY))) }),
      () => {},
    );
  };

  return { onNotesDown, onNotesHover, onNotesContext, onKeysDown, onRulerDown, onSplitDown };
}
