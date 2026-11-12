// A load over the last minute, drawn as a task manager draws CPU: an area against faint lines at
// every quarter, newest on the right. A buffer's whole time is 100 %; past it is a dropout, so the
// scale grows to show how far, with the 100 % line marked. Late buffers tick the bottom edge.
// Hovering reads the value under the pointer.
import { useState, type MouseEvent } from "react";
import { HISTORY_MS } from "./history";
import { pct } from "./format";

/** A value at a time (`performance.now()`). */
export interface Point {
  at: number;
  v: number;
}

const W = 600;
const H = 100;

/** The top of the scale: 100 %, or the next quarter above the highest value. */
export const scaleTop = (points: Point[]) => Math.max(1, Math.ceil(Math.max(0, ...points.map((p) => p.v)) * 4) / 4);

export function LoadGraph({ points, now, late = [], label, big = false }: { points: Point[]; now: number; late?: number[]; label: string; big?: boolean }) {
  const [hover, setHover] = useState<Point | null>(null);
  const top = scaleTop(points);
  const x = (at: number) => ((at - (now - HISTORY_MS)) / HISTORY_MS) * W;
  const y = (v: number) => H - (Math.min(v, top) / top) * H;
  const line = points.map((p) => `${x(p.at).toFixed(1)},${y(p.v).toFixed(1)}`).join(" ");
  const area = points.length ? `${x(points[0].at).toFixed(1)},${H} ${line} ${x(points[points.length - 1].at).toFixed(1)},${H}` : "";
  const grid = [0.25, 0.5, 0.75, 1].map((q) => q * top);

  const onMove = (e: MouseEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const at = now - HISTORY_MS + ((e.clientX - r.left) / r.width) * HISTORY_MS;
    let best: Point | null = null;
    for (const p of points) if (!best || Math.abs(p.at - at) < Math.abs(best.at - at)) best = p;
    setHover(best);
  };

  const shown = hover ?? points[points.length - 1];
  return (
    <div className={"diag-graph" + (big ? " big" : "")} onMouseMove={onMove} onMouseLeave={() => setHover(null)} role="img" aria-label={`${label}: ${shown ? pct(shown.v) : "no data"} now, over the last minute`}>
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
        {grid.map((v) => (
          <line key={v} className={"diag-graph-grid" + (Math.abs(v - 1) < 1e-6 && top > 1 ? " limit" : "")} x1={0} x2={W} y1={y(v)} y2={y(v)} />
        ))}
        {area ? <polygon className="diag-graph-area" points={area} /> : null}
        {line ? <polyline className="diag-graph-line" points={line} /> : null}
        {late.map((at) => (
          <line key={at} className="diag-graph-late" x1={x(at)} x2={x(at)} y1={H} y2={H - 10} />
        ))}
        {hover ? <line className="diag-graph-cursor" x1={x(hover.at)} x2={x(hover.at)} y1={0} y2={H} /> : null}
      </svg>
      {big ? <span className="diag-graph-top">{pct(top)}</span> : null}
      {hover ? (
        <span className="diag-graph-tip" style={{ left: `${Math.min(88, Math.max(0, (x(hover.at) / W) * 100))}%` }}>
          {pct(hover.v, 1)} · {((now - hover.at) / 1000).toFixed(0)} s ago
        </span>
      ) : null}
    </div>
  );
}
