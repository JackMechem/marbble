// The Event List's pointer: a press on a row selects it (and plays a note), a press on a value
// selects its row and a vertical drag then changes that value on every selected event of the type.
import { tempoMap, type TempoMap } from "../../lib/tempo";
import { dragWith } from "../../ui/pointerDrag";
import type { Commands } from "./commands";
import type { Core } from "./core";
import * as el from "./model";

/** Pixels of vertical drag per step of a value. */
const DRAG_PX = 2;

export type Gestures = ReturnType<typeof listGestures>;

export function listGestures(core: Core, c: Commands) {
  const { work, live, preview, sound, commit, bump } = core;
  const { selectRow, selectOnly, pressEndsTyping } = c;

  /** A press on row `i` selects it (Cmd/Ctrl toggles, Shift selects up to it); a plain press on a
   *  note plays it until the button comes up. */
  const onRowDown = (e: React.MouseEvent, i: number) => {
    if (e.button !== 0) return;
    if (pressEndsTyping()) return;
    selectRow(i, { toggle: e.metaKey || e.ctrlKey, range: e.shiftKey });
    const r = live.current.rows[i];
    if (r?.note && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
      const n = work.current.lists.notes[r.idx];
      sound(n.pitch, n.velocity);
      window.addEventListener("mouseup", () => sound(null), { once: true });
    }
  };

  /** A press on a value: selects its row, then a vertical drag changes the value of every
   *  selected event of that type by the same amount. `part` is the part of a position or
   *  length pressed (0 bar, 1 beat, 2 division, 3 tick); other values ignore it. */
  const onCellDown = (e: React.MouseEvent, i: number, field: el.Field, part = 3) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    if (pressEndsTyping()) return;
    const toggle = e.metaKey || e.ctrlKey;
    if (toggle || e.shiftKey) {
      selectRow(i, { toggle, range: e.shiftKey });
      return;
    }
    const row = live.current.rows[i];
    const wasSelected = el.isSelected(work.current.lists, row);
    const lists = wasSelected ? work.current.lists : selectOnly(i, row.key);
    const pr = live.current.project;
    if (pr && el.hasField(row.type, field)) dragValue(e.clientY, tempoMap(pr), i, field, part, lists, wasSelected);
  };

  /** Drags `field` of row `i` and the other selected events of its type in `lists`, previewing
   *  the change by steps of DRAG_PX and sending it on release. A click without a drag on one of
   *  several selected rows selects just it. */
  const dragValue = (y0: number, m: TempoMap, i: number, field: el.Field, part: number, lists: el.Lists, wasSelected: boolean) => {
    const row = live.current.rows[i];
    const targets = live.current.rows.filter((r) => r.type === row.type && el.isSelected(lists, r));
    const hear = (l: el.Lists) => {
      if (row.note) sound(l.notes[row.idx].pitch, l.notes[row.idx].velocity);
    };
    if (row.note && (field === "num" || field === "val")) hear(lists);
    let steps = 0;
    let moved = false;
    const move = (ev: MouseEvent) => {
      const s = Math.trunc((y0 - ev.clientY) / DRAG_PX);
      if (s === steps) return;
      steps = s;
      moved = true;
      preview.current = s ? el.applyChange(m, lists, targets, { field, part, delta: s }) : null;
      if (row.note && field === "num") hear(preview.current ?? lists);
      bump();
    };
    const up = () => {
      sound(null);
      const result = preview.current;
      preview.current = null;
      if (result && steps) commit(result, el.changeLabel(row.type, field));
      else if (!moved && wasSelected && targets.length > 1) selectOnly(i, row.key, lists);
      else bump();
    };
    dragWith(move, up);
  };

  return { onRowDown, onCellDown };
}
