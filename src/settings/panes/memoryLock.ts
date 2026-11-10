// Settings › Audio › Lock audio memory, kept pure so it can be tested. The engine says whether the
// memory is locked, or why not (`crates/engine/src/audio_io/memlock.rs`).

/** Whether Marbble's memory is locked in RAM. */
export type MemoryLock =
  /** Not on this system (macOS, Windows): the switch is hidden. */
  | { state: "unsupported" }
  | { state: "off" }
  | { state: "locked" }
  /** RLIMIT_MEMLOCK is finite (in bytes), so nothing was locked. */
  | { state: "limit_too_small"; limit: number }
  | { state: "failed"; error: string };

/** Whether Settings › Audio offers the switch. */
export const offersMemoryLock = (m: MemoryLock | null): m is MemoryLock => !!m && m.state !== "unsupported";

const size = (bytes: number) => (bytes >= 1 << 20 ? `${Math.round(bytes / (1 << 20))} MB` : `${Math.round(bytes / 1024)} kB`);

/** What the setting has done, under the switch; `warn` when the switch is on but nothing is locked. */
export function memoryLockNote(m: MemoryLock): { text: string; warn: boolean } {
  switch (m.state) {
    case "locked":
      return { text: "Marbble's memory is locked: the system can't swap out the audio it plays.", warn: false };
    case "limit_too_small":
      return {
        text:
          `Not locked: the system's memory lock limit (RLIMIT_MEMLOCK) is ${size(m.limit)}, and Marbble needs it unlimited. ` +
          "Add yourself to the audio group, or set memlock to unlimited in /etc/security/limits.conf, then log in again.",
        warn: true,
      };
    case "failed":
      return { text: `Not locked: ${m.error}.`, warn: true };
    default:
      return { text: "When the system runs short of memory it can swap out what Marbble plays, and the audio drops out while it reads it back.", warn: false };
  }
}
