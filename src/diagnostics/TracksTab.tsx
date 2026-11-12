// The Diagnostics window's Tracks table, as a task manager lists its processes: per track, the
// thread that played its latest buffer, its share of that buffer and its held peak (cells shaded
// by how much they take), its delay compensation, its plug-ins and anything wrong with them. A
// column's heading sorts by it; again, the other way; a third time, back to the project's order.
import { useState, type CSSProperties } from "react";
import { sortRows, trackRows, type SortKey, type TrackRow } from "./facts";
import type { HeldPeaks } from "./peaks";
import type { Diagnostics } from "./snapshot";

const SORT_KEY = "marbble.diagnostics.sort";

interface Sort {
  key: SortKey;
  desc: boolean;
}

const COLUMNS: { key: SortKey | null; label: string; num?: boolean; title?: string; first?: "desc" }[] = [
  { key: "name", label: "Track" },
  { key: "kind", label: "Kind" },
  { key: "thread", label: "Thread" },
  { key: "share", label: "Share", num: true, first: "desc", title: "The track's time in the latest buffer, against the buffer's audio" },
  { key: "peak", label: "Peak", num: true, first: "desc", title: "The worst share in the last 2 s" },
  { key: "latency", label: "Latency", num: true, first: "desc", title: "Its plug-ins' latency, in frames" },
  { key: "delay", label: "Delay", num: true, first: "desc", title: "Its delay compensation, in frames" },
  { key: null, label: "Plug-ins" },
  { key: null, label: "State" },
];

function savedSort(): Sort {
  try {
    const s = JSON.parse(localStorage.getItem(SORT_KEY) ?? "null");
    if (s && typeof s.key === "string" && COLUMNS.some((c) => c.key === s.key)) return { key: s.key, desc: !!s.desc };
  } catch {
    // Remembering the order is a nicety.
  }
  return { key: "order", desc: false };
}

/** A load cell, shaded by how much of the buffer it is, as a task manager shades CPU. */
function LoadCell({ text, v }: { text: string; v: number }) {
  const heat = Math.min(1, v / 0.5);
  return (
    <td className={"num diag-heat" + (v > 1 ? " alert" : "")} style={{ "--heat": `${Math.round(heat * 60)}%` } as CSSProperties}>
      {text}
    </td>
  );
}

export function TracksTab({ d, held }: { d: Diagnostics; held: HeldPeaks }) {
  const [sort, setSort] = useState<Sort>(savedSort);
  const choose = (key: SortKey, first: "asc" | "desc") => {
    const desc0 = first === "desc";
    const next: Sort = sort.key !== key ? { key, desc: desc0 } : sort.desc === desc0 ? { key, desc: !desc0 } : { key: "order", desc: false };
    setSort(next);
    try {
      localStorage.setItem(SORT_KEY, JSON.stringify(next));
    } catch {
      // Remembering the order is a nicety.
    }
  };
  const rows: TrackRow[] = sortRows(trackRows(d, held), sort.key, sort.desc);
  if (!rows.length) return <div className="diag-empty">No tracks.</div>;
  return (
    <div className="diag-table-wrap">
      <table className="diag-table">
        <thead>
          <tr>
            {COLUMNS.map((c) => {
              const on = c.key !== null && sort.key === c.key;
              return (
                <th key={c.label} className={c.num ? "num" : undefined} title={c.title} aria-sort={on ? (sort.desc ? "descending" : "ascending") : undefined}>
                  {c.key ? (
                    <button type="button" className={"diag-sort" + (on ? " sorted" : "")} onClick={() => choose(c.key!, c.first ?? "asc")}>
                      {c.label}
                      <span className="diag-sort-mark" aria-hidden="true">
                        {on ? (sort.desc ? "▾" : "▴") : ""}
                      </span>
                    </button>
                  ) : (
                    c.label
                  )}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.id} className={r.level ?? undefined}>
              <td className="diag-name" title={r.name}>
                {r.name}
              </td>
              <td>{r.kind}</td>
              <td>{r.thread}</td>
              <LoadCell text={r.share} v={r.shareOf} />
              <LoadCell text={r.peak} v={r.peakOf} />
              <td className="num">{r.latency}</td>
              <td className="num">{r.delay}</td>
              <td className="diag-plugins" title={r.plugins}>
                {r.plugins}
              </td>
              <td className="diag-level">{r.state}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
