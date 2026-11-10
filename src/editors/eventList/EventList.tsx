// The Event List (D): Logic's list editor for MIDI regions. Each note and controller event of
// the selected regions is a row; every value can be dragged up or down, or double-clicked and
// typed. The maths lives in model.ts. Edits work on the Piano Roll's working list (midiEdit.ts)
// and go back with `set_midi_content`, one undo step each; a drag previews and sends its result
// on release. The note selection is the store's `midiSelection`, which the Piano Roll shares.
//
// The parts beside this file: scope.ts (what is listed), core.ts (the working list, edits, sound),
// commands.ts, gestures.ts and keys.ts (what the list does, by pointer and key), EventListBar.tsx
// and EventRow.tsx. This component owns the refs and the effects and lays out the list.
//
// Only the rows in view are rendered, so a region with thousands of events scrolls freely.
import { useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";
import { framePos, onFrame } from "../../state/frameLoop";
import { currentTick, useStore, type State } from "../../state/store";
import { tempoMap } from "../../lib/tempo";
import { listCommands } from "./commands";
import { EMPTY_LISTS, listCore, ROW_H, type Editing, type Live, type Work } from "./core";
import { EventListBar } from "./EventListBar";
import { EventRow } from "./EventRow";
import { listGestures } from "./gestures";
import { installMenuHandler, listKeys } from "./keys";
import * as el from "./model";
import { prefs, usePrefs } from "./prefs";
import { scopeKey, scopeName, scopeOf, scopeOfState, type Scope } from "./scope";

/** Rows rendered beyond each edge of the view, so a quick scroll doesn't show a gap. */
const OVERSCAN_ROWS = 4;

/** What the list shows, for its window's title. */
export function eventListTitle(s: State): string {
  const sc = scopeOfState(s);
  const name = sc && scopeName(sc);
  return name ? `Event List — ${name}` : "Event List";
}

/** What the list says in place of rows, or null when it has some. */
function emptyMessage(scope: Scope | null, rows: el.Row[], listed: el.Lists, show: Record<el.EventType, boolean>): string | null {
  if (!scope) return "Select a MIDI region or a software instrument track to list its events.";
  if (!scope.regions.length) return "No MIDI regions on this track yet. Click + to add an event at the playhead.";
  if (rows.length) return null;
  const filtered = el.EVENT_TYPES.some((t) => !show[t.id]) && listed.notes.length + listed.cc.length > 0;
  return filtered ? "No events of the types shown." : "No events. Click + to add one at the playhead.";
}

export function EventList() {
  const project = useStore((s) => s.snap?.project);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const selectedTrack = useStore((s) => s.snap?.selected ?? null);
  const midiSel = useStore((s) => s.midiSelection);
  const p = usePrefs();
  const [, bump] = useReducer((x: number) => x + 1, 0);
  const [editingState, setEditing] = useState<Editing | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewH, setViewH] = useState(200);
  const [playIdx, setPlayIdx] = useState(-1);

  const scope = scopeOf(project, selectedRegions, selectedTrack);

  const work = useRef<Work>({ source: [], key: "", sel: null, regions: [], lists: EMPTY_LISTS });
  const inflight = useRef(0);
  const preview = useRef<el.Lists | null>(null);
  const anchor = useRef<number | null>(null);
  const reveal = useRef(false);
  const editing = useRef<Editing | null>(null);
  const sounding = useRef<number | null>(null);
  const live = useRef<Live>({ project, scope, rows: [] });
  const rootRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  // The list's parts. They hold no state, only closures over the refs above.
  const core = listCore({ work, inflight, preview, anchor, reveal, editing, setEditing, sounding, live, rootRef, scrollRef, bump });
  const commands = listCommands(core);
  const gestures = listGestures(core, commands);
  const onKeyDown = listKeys(core, commands);

  // The working list follows the snapshot and the shared selection; edits replace it ahead of
  // the next snapshot, which isn't used until every edit sent has been answered.
  if (core.isStale(scope, midiSel)) core.rebuild(scope, midiSel);
  const listed = work.current.lists;
  const rows = useMemo(() => el.buildRows(listed, (t) => p.show[t]), [listed, p.show]);
  const shown = preview.current ?? listed;
  live.current = { project, scope, rows };

  useEffect(() => () => core.sound(null), []);
  useEffect(() => installMenuHandler(rootRef, commands));

  // After an edit or a key, the first selected row comes into view.
  useLayoutEffect(() => {
    if (!reveal.current) return;
    reveal.current = false;
    const l = work.current.lists;
    core.scrollToRow(rows.findIndex((r) => el.isSelected(l, r)));
  });

  // Back to the top for another region or track.
  const listedKey = scopeKey(scope);
  useLayoutEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
    anchor.current = null;
  }, [listedKey]);

  // The view's height, and the row at the playhead (followed with Catch Playhead while playing).
  useEffect(() => {
    const sc = scrollRef.current;
    if (!sc) return;
    const ro = new ResizeObserver(() => setViewH(sc.clientHeight));
    ro.observe(sc);
    let last = -2;
    let lastRows: el.Row[] = [];
    const off = onFrame((now) => {
      const i = el.rowAt(live.current.rows, framePos(now));
      if (i === last && live.current.rows === lastRows) return;
      last = i;
      lastRows = live.current.rows;
      setPlayIdx(i);
      if (currentTick()?.playing && prefs.catchPlayhead) core.scrollToRow(i);
    });
    return () => {
      ro.disconnect();
      off();
    };
  }, []);

  const map = project ? tempoMap(project) : null;
  const first = Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN_ROWS);
  const last = Math.min(rows.length, Math.ceil((scrollTop + viewH) / ROW_H) + OVERSCAN_ROWS);
  const empty = emptyMessage(scope, rows, listed, p.show);

  return (
    <div className="editor event-list" ref={rootRef} tabIndex={-1} onKeyDown={onKeyDown}>
      <EventListBar title={scope ? scopeName(scope) : ""} canAdd={!!scope} count={rows.length} c={commands} />
      <div className="el-row el-head">
        <span className="el-cell el-pos">Position</span>
        <span className="el-cell el-status">Status</span>
        <span className="el-cell el-ch">Ch</span>
        <span className="el-cell el-num">Num</span>
        <span className="el-cell el-val">Val</span>
        <span className="el-cell el-len">Length/Info</span>
      </div>
      <div className="el-scroll" ref={scrollRef} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
        {empty ? (
          <div className="editor-empty">{empty}</div>
        ) : (
          <div className="el-space" style={{ height: rows.length * ROW_H }}>
            {map &&
              rows.slice(first, Math.max(first, last)).map((r, k) => <EventRow key={r.key} i={first + k} row={r} shown={shown} map={map} playing={first + k === playIdx} editing={editingState} g={gestures} c={commands} />)}
          </div>
        )}
      </div>
    </div>
  );
}
