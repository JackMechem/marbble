// Track › New Tracks…: the kind of track, how many, and the input or instrument they start with.
import { useState } from "react";
import { BUILTIN_SYNTH, type InputSel, type TrackKind } from "../engine/project";
import { act } from "../engine/commands";
import { Icon } from "../ui/icons";
import { NumberField, Select } from "../ui/Form";
import { setState, useStore } from "../state/store";
import { inputLabel } from "../engine/project";
import { Sheet } from "../ui/Sheet";

export function NewTrackDialog() {
  const plugins = useStore((s) => s.plugins);
  const audio = useStore((s) => s.snap?.audio);
  const [kind, setKind] = useState<TrackKind>("audio");
  const [count, setCount] = useState(1);
  const [inputKey, setInputKey] = useState("mono:0");
  const [ascending, setAscending] = useState(true);
  const [instrument, setInstrument] = useState(BUILTIN_SYNTH);
  const [monitor, setMonitor] = useState(false);
  const [armed, setArmed] = useState(true);
  const close = () => setState({ dialog: null });
  const ch = Math.max(2, audio?.input_channels ?? 2);
  const inputs: [string, string][] = [["none:0", "No Input"]];
  for (let c = 0; c < ch; c++) inputs.push([`mono:${c}`, inputLabel({ type: "mono", ch: c })]);
  for (let c = 0; c + 1 < ch; c += 2) inputs.push([`stereo:${c}`, inputLabel({ type: "stereo", ch: c })]);
  const instList = useStore((s) => s.instList);
  const builtIn: [string, string][] = instList.length ? instList.map((i) => [i.id, i.category === "Marbble" ? i.name : `${i.category}: ${i.name}`]) : [[BUILTIN_SYNTH, "Marbble Synth"]];
  const instruments: [string, string][] = [...builtIn, ...plugins.filter((p) => p.is_instrument).map((p) => [p.id, `${p.name} — ${p.vendor}`] as [string, string])];

  const create = async () => {
    const [t, c] = inputKey.split(":");
    const input: InputSel = t === "none" ? { type: "none" } : { type: t as "mono" | "stereo", ch: parseInt(c) };
    const snap = await act("add_tracks", {
      kind,
      count,
      input,
      ascending,
      instrument: kind === "instrument" ? instrument : null,
      armed,
      monitor: kind === "audio" && monitor,
    });
    if (snap) close();
  };

  return (
    <Sheet title="Choose a track type" onClose={close} width={600}>
      <div className="track-types">
        <button className={"track-type" + (kind === "instrument" ? " on" : "")} aria-pressed={kind === "instrument"} onClick={() => setKind("instrument")} onDoubleClick={create}>
          <Icon.piano />
          <span className="tt-title">Software Instrument</span>
          <span className="tt-sub">Play and record using a VST3 instrument or the built-in synth.</span>
        </button>
        <button className={"track-type" + (kind === "audio" ? " on" : "")} aria-pressed={kind === "audio"} onClick={() => setKind("audio")} onDoubleClick={create}>
          <Icon.mic />
          <span className="tt-title">Audio</span>
          <span className="tt-sub">Record using a microphone or a line input — vocals, guitar, bass.</span>
        </button>
      </div>
      <div className="sheet-details">
        <div className="details-title">Details:</div>
        {kind === "instrument" ? (
          <div className="form-row">
            <label>Instrument:</label>
            <Select value={instrument} options={instruments} onChange={setInstrument} />
          </div>
        ) : (
          <>
            <div className="form-row">
              <label>Audio Input:</label>
              <Select value={inputKey} options={inputs} onChange={setInputKey} />
              <label className="check">
                <input type="checkbox" checked={ascending} onChange={(e) => setAscending(e.target.checked)} /> Ascending
              </label>
            </div>
            <div className="form-hint">{audio?.input_name ? `Input device: ${audio.input_name}` : "No input device — choose one in Settings › Audio"}</div>
          </>
        )}
        <div className="form-row">
          <label>Audio Output:</label>
          {/* New tracks always start on Stereo Out; the strip's Output slot routes them elsewhere. */}
          <span className="dim">Stereo Out — change it later in the channel strip's Output slot</span>
        </div>
        <div className="form-row checks">
          <label className="check">
            <input type="checkbox" checked={armed} onChange={(e) => setArmed(e.target.checked)} /> Record Enable
          </label>
          {kind === "audio" && (
            <label className="check">
              <input type="checkbox" checked={monitor} onChange={(e) => setMonitor(e.target.checked)} /> Input Monitoring
            </label>
          )}
        </div>
      </div>
      <div className="sheet-foot">
        <div className="count">
          <label>Number of tracks:</label>
          <NumberField value={count} min={1} max={64} onChange={(v) => setCount(Math.round(v))} width={84} />
        </div>
        <div className="sheet-buttons">
          <button className="push" onClick={close}>
            Cancel
          </button>
          <button className="push default" onClick={create}>
            Create
          </button>
        </div>
      </div>
    </Sheet>
  );
}
