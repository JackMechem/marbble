// Record Everything (the Diagnostics window): the activity log written to files by the app
// (src-tauri/src/diagnostics/activity.rs). Commands sent to the engine go in by themselves; what the
// interface does without one (a key command, one a MIDI controller fired) is noted here, and the
// app drops it while the log is off.
import { invoke } from "@tauri-apps/api/core";

export interface ActivityStatus {
  on: boolean;
  /** The file being written, while it's on. */
  file: string | null;
  folder: string;
}

export function noteActivity(kind: "key" | "midi" | "ui", what: string) {
  invoke("log_activity", { kind, what }).catch(() => {});
}
