// The words Settings and the Recovery sheet show, with no screen in them: MIDI messages, plug-in
// types and what crashed work is left. Tested in tests/settingsText.test.ts.
import type { PluginDesc } from "./api";
import type { RecordingRecovery, Recovery } from "./prefs";

const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/** A MIDI message in words, Logic-style (C3 = middle C). */
export function describeMidi(d: [number, number, number]): [string, string] {
  const st = d[0] & 0xf0;
  const ch = (d[0] & 0x0f) + 1;
  const note = (n: number) => `${NOTE_NAMES[n % 12]}${Math.floor(n / 12) - 2}`;
  const kind =
    st === 0x90 && d[2] > 0
      ? `Note On  ${note(d[1])}  vel ${d[2]}`
      : st === 0x80 || st === 0x90
        ? `Note Off ${note(d[1])}`
        : st === 0xb0
          ? `Control ${d[1]} = ${d[2]}${d[1] === 64 ? " (sustain)" : d[1] === 1 ? " (mod wheel)" : ""}`
          : st === 0xe0
            ? `Pitch Bend ${((d[2] << 7) | d[1]) - 8192}`
            : st === 0xc0
              ? `Program ${d[1] + 1}`
              : st === 0xd0
                ? `Aftertouch ${d[1]}`
                : st === 0xa0
                  ? `Poly Pressure ${note(d[1])} ${d[2]}`
                  : `0x${d[0].toString(16)}`;
  return [`Ch ${ch}`, kind];
}

export function pluginType(p: PluginDesc): string {
  const parts = p.subcategories.split("|").filter((s) => s && s !== "Fx" && s !== "Instrument" && s !== "Mono" && s !== "Stereo");
  return `${p.is_instrument ? "Instrument" : "Effect"}${parts.length ? ` · ${parts.join(", ")}` : ""}`;
}

export function when(secs: number): string {
  const d = new Date(secs * 1000);
  const today = new Date().toDateString() === d.toDateString();
  return today ? `today at ${d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}` : d.toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}

/** "2 takes (up to 1:05) on Vocal and Guitar": what an interrupted recording left. */
export function recordingSummary(r: RecordingRecovery): string {
  const m = Math.floor(r.seconds / 60);
  const sec = String(Math.floor(r.seconds % 60)).padStart(2, "0");
  const tracks = r.tracks.length > 1 ? `${r.tracks.slice(0, -1).join(", ")} and ${r.tracks[r.tracks.length - 1]}` : r.tracks[0];
  return `${r.takes} ${r.takes === 1 ? "take" : "takes"} (${r.takes === 1 ? "" : "up to "}${m}:${sec})${tracks ? ` on ${tracks}` : ""}`;
}

export function recoveryIntro(list: Recovery[]): string {
  const single = list.length === 1;
  const recording = list.some((r) => r.recording);
  if (single && recording && !list[0].has_autosave)
    return "Marbble quit while this project was recording. The audio recorded up to that moment can be put back on its tracks, where it was recorded.";
  if (single)
    return recording
      ? "This project has autosaved changes that were never saved, and audio that was still being recorded when Marbble quit."
      : "This project has changes that were autosaved but never saved — most likely Marbble quit before you could save them.";
  return recording
    ? "These projects have unsaved work — autosaved changes, or audio that was still being recorded — most likely because Marbble quit."
    : "These projects have autosaved changes that were never saved — most likely Marbble quit before you could save them.";
}
