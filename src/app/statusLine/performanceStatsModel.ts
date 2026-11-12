// The status line's performance figures beside the CPU meter, as text and tooltips, kept pure so
// they can be tested. PerformanceStats.tsx shows them.

/** Polling the memory figure: often enough to follow a project loading, cheap enough to ignore. */
export const MEMORY_POLL_MS = 2000;

/** A resident size, compact: "640 MB", "2.8 GB" (binary units, as the system monitor counts). */
export function memoryText(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  return mb < 1000 ? `${Math.max(1, Math.round(mb))} MB` : `${(mb / 1024).toFixed(1)} GB`;
}

export function memoryTitle(bytes: number): string {
  return `Memory: Marbble has ${memoryText(bytes)} in RAM (its resident size), plug-ins and decoded audio included.\nClick for the Diagnostics window.`;
}

/** "1 thread", "4 threads". */
export function threadsText(threads: number): string {
  return `${threads} ${threads === 1 ? "thread" : "threads"}`;
}

export function threadsTitle(threads: number): string {
  const now = threads > 1 ? `Tracks are playing on ${threads} threads: the audio thread and ${threads - 1} more.` : "Tracks are playing on the audio thread alone.";
  return `${now} A project shares its tracks out over more threads when it needs them, up to the Processing Threads in Settings › Audio.\nClick for the Diagnostics window.`;
}

/** "3 late": blocks over 100% since the meter was last cleared. */
export function lateText(late: number): string {
  return `${late} late`;
}

export function lateTitle(late: number): string {
  const blocks = late === 1 ? "1 block" : `${late} blocks`;
  return `${blocks} over 100% since the meter was last cleared: each took longer to finish than its audio lasts, which you hear as a dropout.\nClick to clear.`;
}
