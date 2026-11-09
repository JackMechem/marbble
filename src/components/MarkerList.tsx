// The Marker List sheet.
import { useEffect, useState } from "react";
import { act, type Marker } from "../api";
import { keyHint } from "../keymap";
import { createMarker, deleteMarker, markerEnd, selectMarker } from "../markers";
import { formatLength, parseLength } from "../eventList";
import { tempoMap } from "../tempo";
import { setState, useStore } from "../store";
import { toBbt } from "../time";
import { useListKeys, type RowProps } from "../listKeys";
import { Sheet } from "./Dialogs";

/** A marker's length in bars, beats, divisions and ticks: its own, or (dimmed) to the next
 *  marker. Type a length to give it one; clear the field to make it reach the next marker. */
function LengthField({ m, tabIndex }: { m: Marker; tabIndex?: number }) {
  const project = useStore((s) => s.snap?.project);
  const own = !!m.length && m.length > 0;
  const end = project ? markerEnd(project, m) : null;
  const shown = project && end !== null ? formatLength(tempoMap(project), m.pos, end - m.pos) : "";
  const [text, setText] = useState(own ? shown : "");
  useEffect(() => setText(own ? shown : ""), [own, shown]);
  if (!project) return null;
  const commit = () => {
    const t = text.trim();
    if (!t) {
      if (own) act("set_marker_length", { id: m.id, length: null });
      return;
    }
    const len = parseLength(t, tempoMap(project), m.pos);
    if (len !== null && len > 0 && len !== m.length) act("set_marker_length", { id: m.id, length: Math.round(len) });
    else setText(own ? shown : "");
  };
  return (
    <input
      className="group-name marker-length"
      aria-label="Length"
      tabIndex={tabIndex}
      value={text}
      placeholder={shown || "—"}
      title="Length (bars beats divisions ticks); empty: to the next marker"
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
    />
  );
}

/** A marker's row. It is a Tab stop when `stop` (listKeys.ts), and so are its fields and buttons:
 *  in the other rows they leave the Tab order, so Tab goes from the row into its own fields and
 *  then out of the list. */
function MarkerRow({ m, row, stop }: { m: Marker; row: RowProps; stop: boolean }) {
  const project = useStore((s) => s.snap?.project);
  const selected = useStore((s) => s.selectedMarker === m.id);
  const [name, setName] = useState(m.name);
  useEffect(() => setName(m.name), [m.name]);
  if (!project) return null;
  const b = toBbt(project, m.pos);
  const tab = stop ? undefined : -1;
  const commit = () => {
    const n = name.trim();
    if (n && n !== m.name) act("rename_marker", { id: m.id, name: n });
    else setName(m.name);
  };
  return (
    <div
      className={"marker-row" + (selected ? " selected" : "")}
      role="row"
      aria-selected={selected}
      aria-label={`${m.name}, ${b.bar} ${b.beat} ${b.div} ${b.tick}`}
      onMouseDown={() => selectMarker(m.id)}
      {...row}
    >
      <span className="swatch" role="gridcell" style={{ background: m.color ?? "#9a9aa0" }} />
      <span className="pos" role="gridcell">
        {b.bar} {b.beat} {b.div} {b.tick}
      </span>
      <span className="marker-cell" role="gridcell">
        <input
          className="group-name"
          aria-label="Name"
          tabIndex={tab}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      </span>
      <span className="marker-cell" role="gridcell">
        <LengthField m={m} tabIndex={tab} />
      </span>
      <span className="marker-cell" role="gridcell">
        <button className="push" title="Go to this marker" tabIndex={tab} onClick={() => act("locate", { pos: m.pos })}>
          Go To
        </button>
      </span>
      <span className="marker-cell" role="gridcell">
        <button className="push" title="Delete this marker" tabIndex={tab} onClick={() => deleteMarker(m.id)}>
          Delete
        </button>
      </span>
    </div>
  );
}

/** Logic's Marker List: every marker with its position, an editable name and length, and a Go
 *  To button. */
export function MarkerList() {
  const markers = useStore((s) => s.snap?.project.markers) ?? [];
  const selected = useStore((s) => s.selectedMarker);
  const close = () => setState({ showMarkerList: false });
  // The rows are one Tab stop (listKeys.ts): Up and Down select, letters type ahead by name,
  // Return goes to the marker as Go To does, Delete deletes it (undoable) as Delete does.
  const byKey = (k: string) => markers.find((m) => String(m.id) === k);
  const { listProps, rowProps, stop } = useListKeys({
    keys: markers.map((m) => String(m.id)),
    labels: markers.map((m) => m.name),
    selected: selected === null ? null : String(selected),
    move: (k) => {
      const m = byKey(k);
      if (m) selectMarker(m.id);
    },
    open: (k) => {
      const m = byKey(k);
      if (m) act("locate", { pos: m.pos });
    },
    remove: (k) => {
      const m = byKey(k);
      if (m) deleteMarker(m.id);
    },
  });
  return (
    <Sheet title="Marker List" onClose={close} width={580}>
      <div className="marker-list" role="grid" aria-label="Markers" {...listProps}>
        {markers.length === 0 && <div className="pref-hint">No markers yet. Create Marker ({keyHint("create-marker")}) adds one at the playhead.</div>}
        {markers.length > 0 && (
          <div className="marker-row marker-head" role="row">
            <span role="columnheader" aria-label="Color" />
            <span role="columnheader">Position</span>
            <span role="columnheader">Name</span>
            <span role="columnheader">Length</span>
            <span role="columnheader" aria-label="Go To" />
            <span role="columnheader" aria-label="Delete" />
          </div>
        )}
        {markers.map((m) => (
          <MarkerRow key={m.id} m={m} row={rowProps(String(m.id))} stop={stop === String(m.id)} />
        ))}
      </div>
      <div className="sheet-foot">
        <button className="push" onClick={() => createMarker()}>
          Create Marker
        </button>
        <div className="sheet-buttons">
          <button className="push default" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </Sheet>
  );
}
