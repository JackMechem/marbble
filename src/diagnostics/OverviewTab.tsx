// The Diagnostics window's Overview: a card per section of facts.ts (device, load, threads, memory,
// MIDI), with the threads' shares of the latest buffer drawn as bars under Threads and cores.
import { overview, threadShares, type Counts } from "./facts";
import { pct } from "./format";
import type { Diagnostics } from "./snapshot";
import { FactCard } from "./FactViews";

/** Each thread's share of the latest buffer, summed over the tracks it played. */
function ThreadBars({ d }: { d: Diagnostics }) {
  const shares = threadShares(d);
  if (!shares.length) return null;
  return (
    <div className="diag-threads" aria-label="Share of the latest buffer by thread">
      {shares.map(([name, share]) => (
        <div key={name} className={"diag-thread" + (share > 1 ? " alert" : share > 0.7 ? " warn" : "")} title={`${name}: its tracks took ${pct(share, 1)} of the latest buffer's time`}>
          <span className="diag-thread-name">{name}</span>
          <span className="diag-bar-track">
            <span className="diag-bar-fill" style={{ width: `${Math.min(100, share * 100)}%` }} />
          </span>
          <span className="diag-thread-value">{pct(share, 1)}</span>
        </div>
      ))}
    </div>
  );
}

export function OverviewTab({ d, counts }: { d: Diagnostics; counts: Counts }) {
  return (
    <div className="diag-overview">
      {overview(d, counts).map((s) => (
        <FactCard key={s.id} section={s}>
          {s.id === "threads" ? <ThreadBars d={d} /> : null}
        </FactCard>
      ))}
    </div>
  );
}
