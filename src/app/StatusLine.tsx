// The status line along the bottom of the main window: the project's name, plug-in latency, the
// audio devices (a click opens Settings › Audio) and the MIDI inputs.
import { setState, useStore } from "../state/store";
import { PdcStatus } from "../mixer/Latency";

export function StatusLine() {
  const audio = useStore((s) => s.snap?.audio);
  const midi = useStore((s) => s.snap?.midi_inputs) ?? [];
  const name = useStore((s) => s.snap?.project.name);
  const dirty = useStore((s) => s.snap?.dirty);
  return (
    <div className="status-line">
      <span>
        {name}
        {dirty ? " — Edited" : ""}
      </span>
      <span className="spacer" />
      <PdcStatus />
      <span className={audio?.running ? "" : "bad"} onClick={() => setState({ dialog: "settings", settingsTab: "audio" })} title="Audio settings">
        {audio?.running
          ? `${audio.input_name ?? "No input"} → ${audio.output_name} · ${(audio.sample_rate / 1000).toFixed(1)} kHz · ${audio.buffer_size}`
          : `Audio stopped${audio?.error ? ` — ${audio.error}` : ""}`}
      </span>
      <span title={midi.join("\n")}>MIDI: {midi.length ? midi.length + " input" + (midi.length > 1 ? "s" : "") : "none"}</span>
    </div>
  );
}
