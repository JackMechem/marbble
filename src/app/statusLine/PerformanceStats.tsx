// The status line's performance figures, at its right end: blocks run late (red, only when there
// are some), the CPU meter unless Settings › Appearance puts it in the control bar, the memory
// Marbble holds and the threads playing. A click on a figure opens the Diagnostics window; on the
// late count or a red meter it clears them. The text is in performanceStatsModel.ts.
import { useEffect, useState } from "react";
import { call } from "../../engine/commands";
import { CpuMeter, openDiagnostics, useAudioLoad, useMeterPlace } from "../cpuMeter/CpuMeter";
import { lateText, lateTitle, MEMORY_POLL_MS, memoryText, memoryTitle, threadsText, threadsTitle } from "./performanceStatsModel";

/** The resident size in bytes, asked for every couple of seconds; null where the system doesn't say. */
function useResidentMemory(): number | null {
  const [bytes, setBytes] = useState<number | null>(null);
  useEffect(() => {
    let live = true;
    const poll = () => call<number | null>("resident_memory").then((b) => live && setBytes(b ?? null));
    poll();
    const timer = window.setInterval(poll, MEMORY_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, []);
  return bytes;
}

export function PerformanceStats() {
  const load = useAudioLoad();
  const inStatusLine = useMeterPlace() === "status_line";
  const memory = useResidentMemory();
  return (
    <div className="status-stats">
      {/* Left of the meter, so it coming and going moves nothing to its right. */}
      {inStatusLine && load.late > 0 && (
        <button className="status-stat late" title={lateTitle(load.late)} aria-label={lateTitle(load.late)} onClick={load.clear}>
          {lateText(load.late)}
        </button>
      )}
      {inStatusLine && <CpuMeter load={load} inline />}
      {memory !== null && (
        <button className="status-stat memory" title={memoryTitle(memory)} aria-label={memoryTitle(memory)} onClick={openDiagnostics}>
          {memoryText(memory)}
        </button>
      )}
      <button className="status-stat threads" title={threadsTitle(load.threads)} aria-label={threadsTitle(load.threads)} onClick={openDiagnostics}>
        {threadsText(load.threads)}
      </button>
    </div>
  );
}
