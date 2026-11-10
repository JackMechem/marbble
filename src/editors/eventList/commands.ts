// What the Event List does, apart from the pointer and the keys that call it: selecting rows,
// deleting events, typing a value into a cell and adding an event at the playhead.
import { type Id, type Project } from "../../engine/project";
import { type Region } from "../../engine/regions";
import { actMade } from "../../engine/commands";
import { framePos } from "../../state/frameLoop";
import { currentTick, getState, setState, showToast } from "../../state/store";
import { clamp } from "../../lib/clamp";
import { tempoMap } from "../../lib/tempo";
import { isEditorSelect, selectEvents } from "../noteSelect";
import type { Core } from "./core";
import * as el from "./model";
import { prefs, setPrefs } from "./prefs";
import { scopeOfState } from "./scope";

export type Commands = ReturnType<typeof listCommands>;

export function listCommands(core: Core) {
  const { work, live, anchor, reveal, editing, setEditing, setSelection, commit, rebuild, focusList } = core;

  // ---- selection --------------------------------------------------------------------------

  /** A click on row `i`, with Cmd/Ctrl to toggle it or Shift to select up to it. */
  const selectRow = (i: number, mods: { toggle: boolean; range: boolean }) => {
    const l = work.current.lists;
    const keys = el.clickSelect(live.current.rows, el.selectedKeys(l), i, anchor.current, mods);
    if (!mods.range) anchor.current = i;
    setSelection(el.selectKeys(l, keys));
  };

  /** Selects just row `i` (with key `key`) of `lists`, and anchors Shift-clicks there. */
  const selectOnly = (i: number, key: string, lists = work.current.lists) => {
    anchor.current = i;
    const only = el.selectKeys(lists, new Set([key]));
    setSelection(only);
    return only;
  };

  const selectAll = () => setSelection(el.selectKeys(work.current.lists, new Set(live.current.rows.map((r) => r.key))));
  const selectNone = () => setSelection(el.selectKeys(work.current.lists, new Set()));
  const anySelected = () => live.current.rows.some((r) => el.isSelected(work.current.lists, r));

  /** The Up or Down arrow (`step` -1 or 1): selects the row past the selection that way (from the
   *  far end with none), or with `extend` adds it to the selection. */
  const stepSelection = (step: -1 | 1, extend: boolean) => {
    const rows = live.current.rows;
    if (!rows.length) return;
    const l = work.current.lists;
    const sel = rows.map((r, i) => (el.isSelected(l, r) ? i : -1)).filter((i) => i >= 0);
    const edge = step < 0 ? sel[0] : sel[sel.length - 1];
    const i = sel.length ? clamp(edge + step, 0, rows.length - 1) : step < 0 ? rows.length - 1 : 0;
    if (extend) setSelection(el.selectKeys(l, new Set([...el.selectedKeys(l), rows[i].key])));
    else selectOnly(i, rows[i].key, l);
    reveal.current = true;
  };

  /** Edit › Select commands (Invert Selection, All Following…) act on the events listed while
   *  the Event List has focus, as in Logic; false for those that stay with the regions. */
  const selectCommand = (id: string): boolean => {
    if (!isEditorSelect(id)) return false;
    const l = work.current.lists;
    const rows = live.current.rows;
    const items = rows.map((r) => {
      if (r.note) {
        const n = l.notes[r.idx];
        return { start: r.start, sel: !!n.sel, equal: `n${n.pitch}`, velocity: n.velocity, muted: n.muted };
      }
      const c = l.cc[r.idx];
      return { start: r.start, sel: !!c.sel, equal: `c${JSON.stringify(c.kind)}:${c.value}` };
    });
    const c = live.current.project?.cycle;
    const sel = selectEvents(id, items, { playhead: currentTick()?.pos ?? 0, locators: c && c.end > c.start ? c : null });
    if (!sel) return false;
    setSelection(el.selectKeys(l, new Set(rows.filter((_, i) => sel[i]).map((r) => r.key))));
    return true;
  };

  /** Deletes the selected events shown (not those of types filtered out). */
  const deleteEvents = () => {
    const l = work.current.lists;
    const keys = new Set(live.current.rows.filter((r) => el.isSelected(l, r)).map((r) => r.key));
    if (keys.size) commit(el.deleteSelected(el.selectKeys(l, keys)), "Delete Events");
  };

  // ---- typing a value ---------------------------------------------------------------------

  /** Opens `field` of row `i` for typing. */
  const startEdit = (i: number, field: el.Field) => {
    const pr = live.current.project;
    const row = live.current.rows[i];
    if (!pr || !row || !el.hasField(row.type, field)) return;
    editing.current = { key: row.key, field, text: el.cellText(tempoMap(pr), work.current.lists, row, field) };
    setEditing(editing.current);
  };

  /** Closes the cell being typed in, sending `text` (null cancels) if it changed and reads. */
  const finishEdit = (text: string | null) => {
    const ed = editing.current;
    editing.current = null;
    setEditing(null);
    if (!ed) return;
    focusList();
    const pr = live.current.project;
    const row = live.current.rows.find((r) => r.key === ed.key);
    if (text === null || !pr || !row) return;
    const m = tempoMap(pr);
    const l = work.current.lists;
    if (text.trim() === el.cellText(m, l, row, ed.field)) return;
    const ch = el.parseCell(m, l, row, ed.field, text);
    if (!ch) {
      showToast(`“${text}” isn't a ${fieldNoun(ed.field, row.note)}`, 2000);
      return;
    }
    commit(el.applyChange(m, l, [row], ch), el.changeLabel(row.type, ed.field));
  };

  /** Focuses the list, and whether a value was being typed: a press then only ends that (focus
   *  leaving the field commits it). */
  const pressEndsTyping = () => {
    const typing = !!editing.current;
    focusList();
    return typing;
  };

  // ---- new events -------------------------------------------------------------------------

  /** The listed region an event at `t` goes in: one holding it, else one within a bar, else
   *  (with regions selected) the nearest, which grows. Null when a region must be made. */
  const regionFor = (t: number, pr: Project): Id | null => {
    const regs = work.current.regions;
    const covering = regs.find((r) => t >= r.start && t < r.start + r.length);
    if (covering) return covering.id;
    const dist = (r: Region) => Math.min(Math.abs(t - r.start), Math.abs(t - (r.start + r.length)));
    const near = [...regs].sort((a, b) => dist(a) - dist(b))[0];
    if (!near) return null;
    const m = tempoMap(pr);
    return dist(near) <= m.samplesPerBarAt(t) || !live.current.scope?.all ? near.id : null;
  };

  /** Makes a one-bar region at `at` on `track`, as part of the undo step `key`, selects it and
   *  lists it. Its id, or null if none was made. */
  const makeRegion = async (track: Id, pr: Project, at: number, key: string): Promise<Id | null> => {
    const m = tempoMap(pr);
    const bar = m.barAt(at);
    const start = Math.round(m.barStart(bar));
    const ids = await actMade("create_midi_region", { track, start, length: Math.round(m.barStart(bar + 1)) - start, key });
    if (!ids?.length) return null;
    setState({ selectedRegions: ids, focus: "regions" });
    const s = getState();
    rebuild(scopeOfState(s), s.midiSelection);
    return ids[0];
  };

  /** Adds an event of `type` at the playhead, like the selected one of that type if any (as
   *  Logic's Create does), in a region made for it if none is near. */
  const addEvent = async (type: el.EventType = prefs.addType) => {
    const { project: pr, scope: sc } = live.current;
    if (!pr || !sc || sc.track.kind !== "instrument") return;
    const at = Math.max(0, Math.round(framePos()));
    const l0 = work.current.lists;
    const model = live.current.rows.find((r) => r.type === type && el.isSelected(l0, r));
    const like = model ? (model.note ? l0.notes[model.idx] : l0.cc[model.idx]) : null;
    let region = regionFor(at, pr);
    let key: string | undefined;
    if (region === null) {
      // The new region goes in the same undo step as the event.
      key = `new-region:${Date.now()}`;
      region = await makeRegion(sc.track.id, pr, at, key);
      if (region === null) return;
    }
    const ev = el.newEvent(tempoMap(pr), type, at, region, like);
    const base = el.selectKeys(work.current.lists, new Set());
    if (!prefs.show[type]) setPrefs({ show: { ...prefs.show, [type]: true } });
    commit({ notes: ev.note ? [...base.notes, ev.note] : base.notes, cc: ev.cc ? [...base.cc, ev.cc] : base.cc }, `Create ${el.eventType(type).name}`, key);
  };

  return { selectRow, selectOnly, selectAll, selectNone, anySelected, stepSelection, selectCommand, deleteEvents, startEdit, finishEdit, pressEndsTyping, addEvent };
}

/** What a field's typed text should have been, for the message when it doesn't read. */
function fieldNoun(field: el.Field, note: boolean) {
  if (field === "pos") return "position";
  if (field === "len") return "length";
  return note && field === "num" ? "note" : "value";
}
