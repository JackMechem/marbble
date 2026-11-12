// The CPU meter: the label and average load over a bar with the recent peak, red once a block runs
// late. It shows in the status line, or in the control bar if Settings › Appearance says so; the
// logic is in cpuMeterModel.ts.
import { useEffect, useState, useSyncExternalStore } from "react";
import { call } from "../../engine/commands";
import { onTick } from "../../state/store";
import { useSettings } from "../../settings/appSettings";
import { cpuLevel, cpuPercent, cpuTitle, meterPlace, shownPeak, type MeterPlace } from "./cpuMeterModel";

/** The engine's load as the tick gives it, with the late blocks counted from the last clear. */
export interface AudioLoad {
  cpu: number;
  peak: number;
  /** Blocks over 100% since the last clear. */
  late: number;
  threads: number;
  clear: () => void;
}

/** The late-block count at the last clear, shared by every meter and the status line's count, so
 *  a clear holds wherever the meter moves. */
let clearedAt = 0;
const clearListeners = new Set<() => void>();

function onClear(l: () => void) {
  clearListeners.add(l);
  return () => {
    clearListeners.delete(l);
  };
}

export function useAudioLoad(): AudioLoad {
  const [cpu, setCpu] = useState(0);
  const [blockPeak, setBlockPeak] = useState(0);
  const [lateBlocks, setLateBlocks] = useState(0);
  const [threads, setThreads] = useState(1);
  const cleared = useSyncExternalStore(onClear, () => clearedAt);
  useEffect(
    () =>
      onTick((t) => {
        setCpu((c) => (Math.abs(c - t.cpu) > 0.01 ? t.cpu : c));
        setBlockPeak((p) => (Math.abs(p - t.cpu_peak) > 0.01 ? t.cpu_peak : p));
        setLateBlocks(t.late_blocks);
        setThreads(t.threads);
      }),
    [],
  );
  const clear = () => {
    clearedAt = lateBlocks;
    clearListeners.forEach((l) => l());
  };
  return { cpu, peak: shownPeak(cpu, blockPeak), late: Math.max(0, lateBlocks - cleared), threads, clear };
}

/** Where the meter shows, from the settings. */
export function useMeterPlace(): MeterPlace {
  return useSettings((s) => meterPlace(s.ui.cpu_meter));
}

/** Window › Diagnostics. */
export function openDiagnostics() {
  call("open_diagnostics_window");
}

/** `inline` lays it out on one line, for the status line: "CPU", the bar, the figure. A click clears
 *  the late blocks, or with none opens the Diagnostics window. */
export function CpuMeter({ load, inline }: { load: AudioLoad; inline?: boolean }) {
  const { cpu, peak, late, threads } = load;
  return (
    <div
      className={"cpu" + (inline ? " inline" : "") + (late > 0 ? " over" : "")}
      title={cpuTitle(cpu, peak, late, threads)}
      onClick={() => (late > 0 ? load.clear() : openDiagnostics())}
    >
      <span className="cpu-label">
        CPU <span className="cpu-pct">{cpuPercent(cpu)}</span>
      </span>
      <div className="cpu-bar">
        <div className={"cpu-fill " + cpuLevel(cpu)} style={{ width: `${Math.min(100, cpu * 100)}%` }} />
        <div className="cpu-peak" style={{ left: `${Math.min(100, peak * 100)}%` }} />
      </div>
    </div>
  );
}
