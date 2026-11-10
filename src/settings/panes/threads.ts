// Settings › Audio › Processing Threads, kept pure so it can be tested. The engine says what this
// machine offers (`crates/engine/src/threads.rs`).

/** Threads the running setting asked for that play nothing, and why. */
export interface LeftOut {
  /** Threads that play tracks, the audio thread included. */
  playing: number;
  /** Threads asked for, the audio thread included. */
  threads: number;
  /** Refused real-time priority by the system. */
  refused: number;
  /** Stopped after a plug-in kept one past the time limit. */
  stuck: number;
}

/** What the machine offers: what Automatic comes to here, and the highest number. */
export interface ThreadChoices {
  automatic: number;
  max: number;
  left_out?: LeftOut | null;
}

/** The setting's value for Automatic. */
export const AUTOMATIC_THREADS = 0;

const plural = (n: number) => `${n} ${n === 1 ? "thread" : "threads"}`;

/** The menu: Automatic with what it means here, then the numbers. */
export function threadOptions(c: ThreadChoices): [number, string][] {
  const options: [number, string][] = [[AUTOMATIC_THREADS, `Automatic (up to ${plural(c.automatic)})`]];
  options.push([1, "1 thread (the audio thread alone)"]);
  for (let n = 2; n <= c.max; n++) options.push([n, plural(n)]);
  return options;
}

/** Why fewer threads play than the setting asked for, or null when they all do. */
export function threadNote(c: ThreadChoices): string | null {
  const l = c.left_out;
  if (!l || l.refused + l.stuck === 0) return null;
  const on = l.playing <= 1 ? "the audio thread alone" : plural(l.playing);
  const parts: string[] = [];
  if (l.refused > 0) {
    const extra = l.threads - 1;
    parts.push(
      l.refused >= extra
        ? "The system refused real-time priority to the extra threads"
        : `The system refused real-time priority to ${l.refused} of the ${extra} extra threads`,
    );
  }
  if (l.stuck > 0) parts.push(`${l.stuck === 1 ? "a thread was" : `${l.stuck} threads were`} stopped after a plug-in on it stopped responding`);
  const why = parts.join(", and ");
  const first = why.charAt(0).toUpperCase() + why.slice(1);
  const back = l.stuck > 0 ? " Reset Audio Engine starts the threads afresh." : "";
  return `${first}, so tracks play on ${on}.${back}`;
}
