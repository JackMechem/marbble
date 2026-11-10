// One line of the Event List: position, status, channel, number, value and length or info. A
// press on a value drags it and a double-click types it (gestures.ts); positions and lengths are
// four parts, each dragged on its own.
import type { Bbt, TempoMap } from "../../lib/tempo";
import type { Commands } from "./commands";
import { ROW_H, type Editing } from "./core";
import type { Gestures } from "./gestures";
import * as el from "./model";

interface Props {
  /** The row's index in the list, and the row. */
  i: number;
  row: el.Row;
  /** The lists the values are read from (a drag's preview while there is one). */
  shown: el.Lists;
  map: TempoMap;
  playing: boolean;
  /** The cell being typed in, if any (of any row). */
  editing: Editing | null;
  g: Gestures;
  c: Pick<Commands, "startEdit" | "finishEdit">;
}

export function EventRow({ i, row, shown, map, playing, editing, g, c }: Props) {
  const n = row.note ? shown.notes[row.idx] : null;
  const cc = row.note ? null : shown.cc[row.idx];
  if (!n && !cc) return null;

  /** A position or length as four draggable parts. */
  const parts = (b: Bbt, field: "pos" | "len") =>
    [b.bar, b.beat, b.div, b.tick].map((v, k) => (
      <span key={k} className={`el-part p${k}`} onMouseDown={(e) => g.onCellDown(e, i, field, k)} onDoubleClick={() => c.startEdit(i, field)}>
        {v}
      </span>
    ));

  const cell = (field: el.Field, cls: string, content: React.ReactNode) => {
    if (editing?.key === row.key && editing.field === field) return <EditingCell cls={cls} text={editing.text} finish={c.finishEdit} />;
    // A position or length's parts take the presses themselves.
    if (field === "pos" || field === "len") return <span className={`el-cell ${cls} el-edit`}>{content}</span>;
    return (
      <span className={`el-cell ${cls} el-edit`} onMouseDown={(e) => g.onCellDown(e, i, field)} onDoubleClick={() => c.startEdit(i, field)}>
        {content}
      </span>
    );
  };

  let info: React.ReactNode;
  if (n) info = cell("len", "el-len", parts(el.lengthBbt(map, n.start, n.length), "len"));
  else if (cc!.kind.type === "controller") info = <span className="el-cell el-info">{el.ccName(cc!.kind.cc)}</span>;
  else if (cc!.kind.type === "pitch_bend") info = <span className="el-cell el-info">{`${cc!.value >= 8192 ? "+" : "−"}${Math.abs(cc!.value - 8192)}`}</span>;
  else info = <span className="el-cell el-info" />;

  const cls = ["el-row", i % 2 ? "odd" : "", n?.sel || cc?.sel ? "sel" : "", playing ? "playing" : "", n?.muted ? "muted" : ""].filter(Boolean).join(" ");
  return (
    <div className={cls} style={{ transform: `translateY(${i * ROW_H}px)` }} onMouseDown={(e) => g.onRowDown(e, i)}>
      {cell("pos", "el-pos", parts(el.positionBbt(map, n?.start ?? cc!.start), "pos"))}
      <span className="el-cell el-status">{el.eventType(row.type).status}</span>
      <span className="el-cell el-ch">1</span>
      {el.hasField(row.type, "num") ? cell("num", "el-num", row.note ? el.noteName(el.numOf(shown, row)) : el.numOf(shown, row)) : <span className="el-cell el-num" />}
      {cell("val", "el-val", el.valOf(shown, row))}
      {info}
    </div>
  );
}

/** The field a value is typed in: Enter or leaving it sends, Escape cancels. */
function EditingCell({ cls, text, finish }: { cls: string; text: string; finish: (text: string | null) => void }) {
  return (
    <span className={`el-cell ${cls}`}>
      <input
        className="el-input"
        autoFocus
        defaultValue={text}
        onFocus={(e) => e.currentTarget.select()}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") finish(e.currentTarget.value);
          else if (e.key === "Escape") finish(null);
        }}
        onBlur={(e) => finish(e.currentTarget.value)}
      />
    </span>
  );
}
