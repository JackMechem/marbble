// The Piano Roll's commands, whichever way they are reached (Edit and Functions menus, keys, the
// native menu bar): zoom, the clipboards, Quantize and the other edits on the notes.
import * as me from "../../midiEdit";
import { exportMidi } from "../../midiFiles";
import { isEditorSelect, selectEvents } from "../../noteSelect";
import { timeAt } from "../../pianoRollDraw";
import { currentTick } from "../../store";
import { samplesPerBeat } from "../../time";
import { askName } from "../NameSheet";
import type { Core } from "./core";
import { setPrefs } from "./prefs";
import { laneKind, timeBase } from "../../pianoRollModel";
import type { PNote } from "../../midiEdit";

/** Notes copied in the Piano Roll (separate from the region clipboard). */
export let noteClipboard: me.ClipNote[] = [];
/** Controller events copied from the lane. One clipboard at a time: copying either clears the
 *  other, so Paste puts back what was copied last. */
export let ccClipboard: me.ClipCc[] = [];

export type Commands = ReturnType<typeof editorCommands>;

export function editorCommands(ed: Core) {
  const { view, grid, warp, warpedBars, musical, commit, commitWithRegion, regionFor, setSelection, noRegionHere, work, live, lastArea, scrollRef, pendingScroll } = ed;

  // ---- zoom -----------------------------------------------------------------------------------

  const zoomX = (factor: number, anchorX?: number) => {
    const v = view();
    const ax = anchorX ?? v.W / 2;
    const t = timeAt(v, ax);
    const next = Math.max(6, Math.min(1600, live.current.pr.pxPerBeat * factor));
    const pps = next / samplesPerBeat(live.current.project);
    pendingScroll.current = { ...pendingScroll.current, left: t * pps - ax };
    setPrefs({ pxPerBeat: next });
  };
  const zoomY = (factor: number, anchorY?: number) => {
    const v = view();
    const ay = anchorY ?? v.H / 2;
    const rows = (ay + v.top) / v.noteH;
    const next = Math.max(5, Math.min(32, Math.round(live.current.pr.noteH * factor * 2) / 2));
    if (next === v.noteH) return;
    pendingScroll.current = { ...pendingScroll.current, top: rows * next - ay };
    setPrefs({ noteH: next });
  };

  /** Fits the edited region (or every region) and its notes into view. */
  const zoomToFit = () => {
    const el = scrollRef.current!;
    const regs = work.current.regions.filter((r) => live.current.focus === null || r.id === live.current.focus);
    if (!regs.length) return;
    const lo = me.minOf(regs, (r) => r.start);
    const hi = me.maxOf(regs, (r) => r.start + r.length);
    const spb = samplesPerBeat(live.current.project);
    const W = el.clientWidth - 40;
    const pxPerBeat = Math.max(6, Math.min(1600, (W / (hi - lo)) * spb));
    const notes = work.current.notes.filter((n) => regs.some((r) => r.id === n.region));
    const pLo = notes.length ? me.minOf(notes, (n) => n.pitch) : 48;
    const pHi = notes.length ? me.maxOf(notes, (n) => n.pitch) : 72;
    const noteH = Math.max(5, Math.min(24, Math.floor(el.clientHeight / (pHi - pLo + 5))));
    pendingScroll.current = { left: lo * (pxPerBeat / spb) - 20, top: (127 - pHi - 2) * noteH };
    setPrefs({ pxPerBeat, noteH });
  };

  // ---- edits ----------------------------------------------------------------------------------

  const notesNow = () => work.current.notes;
  const anySelected = () => work.current.notes.some((n) => n.sel);
  const playhead = () => currentTick()?.pos ?? 0;

  /** Whether Copy, Cut and Delete act on controller events: some are selected in the lane
   *  shown, and the lane was clicked last or no notes are selected. */
  const ccTarget = () => !!laneKind(live.current.pr.lane) && work.current.cc.some((c) => c.sel) && (lastArea.current === "lane" || !anySelected());
  const canEdit = () => anySelected() || ccTarget();
  const copy = () => {
    if (ccTarget()) {
      ccClipboard = me.copyCc(work.current.cc);
      noteClipboard = [];
      return ccClipboard.length;
    }
    const c = me.copyNotes(notesNow());
    if (c.length) [noteClipboard, ccClipboard] = [c, []];
    return c.length;
  };
  const deleteSelected = () => {
    if (ccTarget()) {
      commit(notesNow(), work.current.cc.filter((c) => !c.sel), "Delete Controller Events");
      return;
    }
    if (!anySelected()) return;
    commit(
      notesNow().filter((n) => !n.sel),
      work.current.cc,
      "Delete Notes",
    );
  };
  const cut = () => {
    const cc = ccTarget();
    if (!copy()) return;
    if (cc) return commit(notesNow(), work.current.cc.filter((c) => !c.sel), "Cut Controller Events");
    commit(
      notesNow().filter((n) => !n.sel),
      work.current.cc,
      "Cut Notes",
    );
  };
  const paste = () => {
    const at = Math.round(playhead());
    if (ccClipboard.length) {
      const rid = regionFor(at);
      if (rid === null) return noRegionHere();
      commit(notesNow(), me.pasteCc(work.current.cc, ccClipboard, at, rid), "Paste Controller Events");
      return;
    }
    if (!noteClipboard.length) return;
    const rid = regionFor(at) ?? -1;
    commitWithRegion(me.pasteNotes(notesNow(), noteClipboard, at, rid), at, "Paste Notes");
  };
  const duplicate = () => {
    if (anySelected()) commit(musical(notesNow(), (n) => me.duplicate(n, grid().grid)), work.current.cc, "Duplicate Notes");
  };
  const selectAll = () => setSelection(me.selectAll(notesNow()));
  /** Edit › Select commands (Invert Selection, All Following…) act on the notes while the
   *  Piano Roll has focus, as in Logic; false for those that stay with the regions. */
  const selectCommand = (id: string): boolean => {
    if (!isEditorSelect(id)) return false;
    const c = live.current.project.cycle;
    const notes = notesNow();
    const items = notes.map((n) => ({ start: n.start, sel: n.sel, equal: String(n.pitch), velocity: n.velocity, muted: n.muted }));
    const sel = selectEvents(id, items, { playhead: playhead(), locators: c.end > c.start ? c : null });
    if (!sel) return false;
    setSelection(
      notes.map((n, i) => (n.sel === sel[i] ? n : { ...n, sel: sel[i] })),
      work.current.cc.map((x) => (x.sel ? { ...x, sel: false } : x)),
    );
    return true;
  };
  const apply = (f: (n: PNote[]) => PNote[], label: string, key?: string) => commit(f(notesNow()), work.current.cc, label, key);
  const quantizeNow = () => {
    const { project, pr } = live.current;
    const tb = timeBase(project);
    const bar = warpedBars(warp());
    apply((n) => musical(n, (v) => me.quantize(v, { grid: me.gridLength(tb, pr.qValue), bar, strength: pr.qStrength / 100, swing: pr.qSwing / 100, ends: pr.qEnds })), "Quantize");
  };
  const msToSamples = (ms: number) => (ms / 1000) * live.current.project.sample_rate;
  const humanize = (ms: number, vel: number) => apply((n) => me.humanize(n, { time: msToSamples(ms), velocity: vel, length: 0 }), "Humanize");
  /** Asks for one or more numbers (Transpose by…, Set Velocity to…) in the name sheet. */
  const ask = async (title: string, label: string, def: string, f: (v: number[]) => void) => {
    const numbers = (text: string) => text.split(/[,\s]+/).filter(Boolean).map(Number).filter((x) => Number.isFinite(x));
    const a = await askName({ title, label, value: def, check: (t) => (numbers(t).length ? null : "Type a number.") });
    if (a !== null) f(numbers(a));
  };
  const exportRegion = () => {
    const r = work.current.regions.find((x) => x.id === live.current.focus) ?? work.current.regions[0];
    if (r) exportMidi([r.id], r.name);
  };
  const nudgeKey = () => `nudge-notes:${live.current.track.id}`;

  return { zoomX, zoomY, zoomToFit, notesNow, anySelected, playhead, canEdit, copy, deleteSelected, cut, paste, duplicate, selectAll, selectCommand, apply, quantizeNow, msToSamples, humanize, ask, exportRegion, nudgeKey };
}
