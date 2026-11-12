// The CPU meter's logic, kept pure so it can be tested; CpuMeter.tsx draws it in the status line or
// the control bar. The engine measures the load (`BlockLoad` in crates/engine/src/dsp/mod.rs) and
// sends it with each tick.

/** Where the meter shows (Settings › Appearance): the status line at the bottom, or the control bar
 *  at the top as in Logic Pro. */
export type MeterPlace = "status_line" | "control_bar";

/** The setting as saved; anything unknown (or not saved yet) is the status line. */
export function meterPlace(saved: string | null | undefined): MeterPlace {
  return saved === "control_bar" ? "control_bar" : "status_line";
}

/** Where the peak tick goes. The average is smoothed over blocks, so with a long buffer it can
 *  still be coming down after the peak has fallen; a peak under the average would make no sense. */
export function shownPeak(cpu: number, peak: number): number {
  return Math.max(cpu, peak);
}

/** The meter's tooltip. `late` is the blocks over 100% since the meter was last cleared, `threads`
 *  how many threads are playing tracks. A click clears them, or with none opens Diagnostics. */
export function cpuTitle(cpu: number, peak: number, late: number, threads: number): string {
  const pct = (v: number) => `${(v * 100).toFixed(0)}%`;
  const lines = [
    "Audio load: how much of each buffer's time it takes to finish the buffer, from start to end, however many threads work on it. Over 100% means dropouts. This isn't the same as your system monitor's CPU figure, which is shared across all cores.",
    `Average ${pct(cpu)}, recent peak ${pct(peak)}.`,
    threads > 1 ? `Playing on ${threads} threads.` : "Playing on the audio thread alone.",
  ];
  lines.push(late > 0 ? `${late} ${late === 1 ? "block" : "blocks"} over 100%. Click to clear.` : "Click for the Diagnostics window.");
  return lines.join("\n");
}

/** The figure beside the meter: the average load as a whole percentage ("14%"; over 100% when
 *  blocks are late). */
export function cpuPercent(cpu: number): string {
  return `${Math.max(0, Math.round(cpu * 100))}%`;
}

/** The bar's colour band: grey up to half, amber past it, red past 80%. */
export function cpuLevel(cpu: number): "low" | "mid" | "high" {
  return cpu > 0.8 ? "high" : cpu > 0.5 ? "mid" : "low";
}
