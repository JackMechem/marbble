// The Diagnostics window's Tracks table: per track, the thread that played its latest buffer, its
// share of that buffer and its held peak (with a bar), its delay compensation, its plug-ins and
// anything wrong with them. In the project's order, or heaviest first.
import { useState } from "react";
import { heaviestFirst, trackRows, type TrackRow } from "./facts";
import type { HeldPeaks } from "./peaks";
import type { Diagnostics } from "./snapshot";
import { Icon } from "../ui/icons";

const SORT_KEY = "marbble.diagnostics.heaviestFirst";

function savedSort(): boolean {
  try {
    return localStorage.getItem(SORT_KEY) === "1";
  } catch {
    return false;
  }
}

/** The share and the peak, as a bar of the peak with the latest share inside it. */
function LoadBar({ row }: { row: TrackRow }) {
  return (
    <span className="diag-bar-track" aria-hidden="true">
      <span className="diag-bar-peak" style={{ width: `${Math.min(100, row.peakOf * 100)}%` }} />
      <span className="diag-bar-fill" style={{ width: `${Math.min(100, row.shareOf * 100)}%` }} />
    </span>
  );
}

export function TracksTab({ d, held }: { d: Diagnostics; held: HeldPeaks }) {
  const [heaviest, setHeaviest] = useState(savedSort);
  const toggle = () => {
    setHeaviest(!heaviest);
    try {
      localStorage.setItem(SORT_KEY, heaviest ? "0" : "1");
    } catch {
      // Remembering the order is a nicety.
    }
  };
  const rows = trackRows(d, held);
  const shown = heaviest ? heaviestFirst(rows) : rows;
  return (
    <div className="diag-tracks">
      <div className="diag-toolbar">
        <button type="button" className={"diag-btn" + (heaviest ? " on" : "")} aria-pressed={heaviest} title="Heaviest first: sort by peak share of the buffer" aria-label="Heaviest first" onClick={toggle}>
          <Icon.sortDown />
        </button>
        <span className="diag-note">Share: the track's time in the latest buffer, against the buffer's audio. Peak: the worst in the last 2 s.</span>
      </div>
      {shown.length === 0 ? (
        <div className="diag-empty">No tracks.</div>
      ) : (
        <div className="diag-table-wrap">
          <table className="diag-table">
            <thead>
              <tr>
                <th>Track</th>
                <th>Kind</th>
                <th>Thread</th>
                <th className="num">Share</th>
                <th className="num">Peak</th>
                <th className="diag-load-col">Load</th>
                <th className="num" title="Its plug-ins' latency, in frames">Latency</th>
                <th className="num" title="Its delay compensation, in frames">Delay</th>
                <th>Plug-ins</th>
                <th>State</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((r) => (
                <tr key={r.id} className={r.level ?? undefined}>
                  <td className="diag-name" title={r.name}>
                    {r.name}
                  </td>
                  <td>{r.kind}</td>
                  <td>{r.thread}</td>
                  <td className="num">{r.share}</td>
                  <td className="num diag-level">{r.peak}</td>
                  <td className="diag-load-col">
                    <LoadBar row={r} />
                  </td>
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
      )}
    </div>
  );
}
