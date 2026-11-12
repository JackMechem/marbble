// The Diagnostics window's Performance page, laid out as a task manager's: the engine's load over
// the last minute, the figures that matter beside it, a graph per thread as a task manager draws
// one per core, and below, a card per section of facts.ts (device, load, threads, memory, MIDI).
import { headline, overview, type Counts, type Fact } from "./facts";
import { threadNames, type Sample } from "./history";
import { LoadGraph } from "./LoadGraph";
import { kb, pct } from "./format";
import type { Diagnostics } from "./snapshot";
import { FactCard } from "./FactViews";

/** A figure in the stats beside the graph: a small label over a large value. */
function Stat({ fact }: { fact: Fact }) {
  return (
    <div className={"diag-stat" + (fact.level ? ` ${fact.level}` : "")} title={fact.hint}>
      <span className="diag-stat-label">{fact.label}</span>
      <span className="diag-stat-value">{fact.value}</span>
    </div>
  );
}

export function PerformanceTab({ d, counts, history, now }: { d: Diagnostics; counts: Counts; history: Sample[]; now: number }) {
  const load = history.map((s) => ({ at: s.at, v: s.load }));
  const late = history.filter((s) => s.late > 0).map((s) => s.at);
  const names = threadNames(history);
  const stats: Fact[] = [
    ...headline(d, counts),
    { label: "Recent peak", value: pct(d.load.cpu_peak), level: d.load.cpu_peak > 1 ? "alert" : d.load.cpu_peak > 0.7 ? "warn" : undefined, hint: "The worst recent buffer, as the CPU meter's peak shows it." },
    { label: "Median", value: pct(d.load.p50), hint: "Half the buffers took less than this share of their time." },
    { label: "Worst", value: pct(d.load.max), level: d.load.max > 1 ? "alert" : undefined, hint: "The slowest buffer since the last reset. Over 100% is a dropout." },
    { label: "Memory", value: kb(d.memory.resident_kb), hint: "What Marbble has in RAM." },
  ];
  return (
    <div className="diag-perf">
      <section className="diag-perf-main" aria-label="Engine load">
        <header className="diag-perf-head">
          <h2>Engine load</h2>
          <span className="diag-perf-sub">each buffer's time to finish against its audio's · 60 s</span>
        </header>
        <LoadGraph points={load} now={now} late={late} label="Engine load" big />
        <div className="diag-stats">
          {stats.map((f) => (
            <Stat key={f.label} fact={f} />
          ))}
        </div>
      </section>
      {names.length ? (
        <section aria-label="Threads">
          <header className="diag-perf-head">
            <h2>Threads</h2>
            <span className="diag-perf-sub">each thread's share of the buffer, summed over the tracks it played</span>
          </header>
          <div className="diag-thread-graphs">
            {names.map((n) => {
              const points = history.map((s) => ({ at: s.at, v: s.threads[n] ?? 0 }));
              const last = points[points.length - 1]?.v ?? 0;
              return (
                <div key={n} className={"diag-thread-graph" + (last > 1 ? " alert" : last > 0.7 ? " warn" : "")}>
                  <div className="diag-thread-graph-head">
                    <span>{n}</span>
                    <span className="diag-thread-graph-value">{pct(last, 1)}</span>
                  </div>
                  <LoadGraph points={points} now={now} label={n} />
                </div>
              );
            })}
          </div>
        </section>
      ) : null}
      <div className="diag-overview">
        {overview(d, counts).map((s) => (
          <FactCard key={s.id} section={s} />
        ))}
      </div>
    </div>
  );
}
