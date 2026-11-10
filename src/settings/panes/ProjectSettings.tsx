// File › Project Settings…: tempo, key and signature, the project's own recording and metronome
// settings, and its notes.
import { useEffect, useRef, useState } from "react";
import { supportedRates, type AudioConfig, type DeviceList } from "../../engine/devices";
import { act, call } from "../../engine/commands";
import { useSettings } from "../appSettings";
import { setState, useStore } from "../../state/store";
import { Check, NumberField, Select } from "../../ui/Form";
import { Foot, Row, Section, SheetFrame, TabIcon } from "./parts";
import { BIT_DEPTHS } from "./RecordingPane";
import { MetronomeDefaults, MetronomeFields } from "./MetronomePane";

const KEYS = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
const SIGNATURES: [number, number][] = [
  [2, 4],
  [3, 4],
  [4, 4],
  [5, 4],
  [6, 8],
  [7, 8],
  [9, 8],
  [12, 8],
];
const RATES = [44100, 48000, 88200, 96000, 176400, 192000];

function ProjectGeneral() {
  const p = useStore((s) => s.snap?.project);
  const audio = useStore((s) => s.snap?.audio);
  const [pending, setPending] = useState<number | null>(null);
  // Only the rates the audio devices can run at: the project's rate is the device's.
  const [rates, setRates] = useState<number[]>(RATES);
  useEffect(() => {
    (async () => {
      const cfg = await call<AudioConfig>("get_audio_config");
      const devices = cfg && (await call<DeviceList>("audio_devices", { host: cfg.host }));
      if (cfg && devices) setRates(RATES.filter((r) => supportedRates(cfg, devices).includes(r)));
    })();
  }, [audio?.output_name, audio?.input_name]);
  if (!p) return null;
  const hasAudio = p.tracks.some((t) => t.regions.some((r) => r.content.type === "audio"));
  const sigKey = `${p.sig_num}/${p.sig_den}`;
  const sigs: [string, string][] = SIGNATURES.map(([n, d]) => [`${n}/${d}`, `${n}/${d}`]);
  if (!sigs.some((s) => s[0] === sigKey)) sigs.push([sigKey, sigKey]);
  const applyRate = async (rate: number) => {
    const cfg = await call<AudioConfig>("get_audio_config");
    if (cfg) await act("set_audio_config", { config: { ...cfg, sample_rate: rate } });
    setPending(null);
  };
  return (
    <>
      <Section title="Tempo and Key">
        <Row label="Tempo:">
          <div className="row-inline">
            <NumberField value={p.tempo} min={5} max={990} step={1} resolution={0.01} onChange={(bpm) => act("set_tempo", { bpm })} width={96} />
            <span className="dim">{p.tempo_map.length > 1 ? "BPM at the start (see the Tempo track)" : "BPM"}</span>
          </div>
        </Row>
        <Row label="Time Signature:">
          <Select
            value={sigKey}
            options={sigs}
            onChange={(v) => {
              const [num, den] = v.split("/").map(Number);
              act("set_signature", { num, den });
            }}
            width={110}
          />
        </Row>
        <Row label="Key:">
          <Select
            value={p.key}
            options={KEYS.flatMap((k) => [[`${k} major`, `${k} major`] as [string, string], [`${k} minor`, `${k} minor`] as [string, string]])}
            onChange={(key) => act("set_key", { key })}
            width={140}
          />
        </Row>
      </Section>
      <Section title="Audio">
        <Row
          label="Sample Rate:"
          note={
            pending !== null && hasAudio ? (
              <span className="pref-warning">
                This project's audio was recorded at {(p.sample_rate / 1000).toFixed(1)} kHz. Marbble plays audio files without converting them, so at{" "}
                {(pending / 1000).toFixed(1)} kHz they would play {pending > p.sample_rate ? "faster and higher" : "slower and lower"}.{" "}
                <button className="link-btn" onClick={() => applyRate(pending)}>
                  Change Anyway
                </button>{" "}
                <button className="link-btn" onClick={() => setPending(null)}>
                  Cancel
                </button>
              </span>
            ) : (
              `The audio device runs at the project's rate${audio?.running ? ` (now ${(audio.sample_rate / 1000).toFixed(1)} kHz on ${audio.output_name})` : ""}. Imported files are converted to it.`
            )
          }
        >
          <Select
            value={pending ?? p.sample_rate}
            options={[...new Set([...rates, p.sample_rate])].sort((a, b) => a - b).map((r) => [r, `${(r / 1000).toFixed(1)} kHz`] as [number, string])}
            onChange={(r) => (r === p.sample_rate ? setPending(null) : hasAudio ? setPending(r) : applyRate(r))}
            width={140}
          />
        </Row>
      </Section>
    </>
  );
}

function ProjectRecording() {
  const depth = useStore((s) => s.snap?.project.bit_depth ?? null);
  const countIn = useStore((s) => s.snap?.project.count_in ?? false);
  const app = useSettings((s) => s.prefs.recording);
  return (
    <>
      <Section title="Recording">
        <Row label="Bit Depth:" note={`“Use Settings” follows Settings › Recording (now ${BIT_DEPTHS.find((b) => b[0] === app.bit_depth)?.[1]}).`}>
          <Select
            value={depth ?? "app"}
            options={[["app", "Use Settings"] as [string, string], ...BIT_DEPTHS]}
            onChange={(v) => act("set_record_bit_depth", { depth: v === "app" ? null : v })}
          />
        </Row>
        <Row label="Count-in:" note={`${app.count_in_bars} bar${app.count_in_bars > 1 ? "s" : ""}, set in Settings › Recording.`}>
          <Check checked={countIn} onChange={(on) => act("set_count_in", { on })}>
            Count in before recording
          </Check>
        </Row>
      </Section>
      <div className="pref-note">
        Overlapping recordings, take names and the recording folder are set for all projects in{" "}
        <button className="link-btn" onClick={() => setState({ dialog: "settings", settingsTab: "recording" })}>
          Settings › Recording
        </button>
        .
      </div>
    </>
  );
}

function ProjectNotes() {
  const notes = useStore((s) => s.snap?.project.notes ?? "");
  const [text, setText] = useState(notes);
  const latest = useRef({ text, notes });
  latest.current = { text, notes };
  useEffect(() => setText(notes), [notes]);
  // Save when leaving the tab or closing the sheet, too.
  useEffect(
    () => () => {
      const { text, notes } = latest.current;
      if (text !== notes) act("set_project_notes", { notes: text });
    },
    [],
  );
  return (
    <div className="pref-group">
      <textarea
        className="text-field project-notes"
        value={text}
        placeholder="Lyrics, session notes, who played what…"
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text !== notes && act("set_project_notes", { notes: text })}
        onKeyDown={(e) => e.stopPropagation()}
      />
      <div className="pref-note">Saved with the project.</div>
    </div>
  );
}

/** File › Project Settings… : what is saved with this project. */
export function ProjectSettingsSheet() {
  const name = useStore((s) => s.snap?.project.name ?? "");
  const [tab, setTab] = useState<"general" | "recording" | "metronome" | "notes">("general");
  const close = () => setState({ dialog: null });
  const tabs = [
    { id: "general" as const, label: "General", icon: <TabIcon.project /> },
    { id: "recording" as const, label: "Recording", icon: <TabIcon.recording /> },
    { id: "metronome" as const, label: "Metronome", icon: <TabIcon.metronome /> },
    { id: "notes" as const, label: "Notes", icon: <TabIcon.notes /> },
  ];
  return (
    // One height for every tab, as Settings has (settings-window.css).
    <SheetFrame title={`Project Settings — ${name}`} onClose={close} width={620} className="settings-sheet project-settings-sheet">
      <div className="settings-tabs">
        {tabs.map((t) => (
          <button key={t.id} className={"settings-tab" + (tab === t.id ? " on" : "")} aria-pressed={tab === t.id} onClick={() => setTab(t.id)}>
            {t.icon}
            <span>{t.label}</span>
          </button>
        ))}
      </div>
      <div className="settings-body" key={tab}>
        {tab === "general" && <ProjectGeneral />}
        {tab === "recording" && <ProjectRecording />}
        {tab === "metronome" && (
          <>
            <MetronomeFields />
            <MetronomeDefaults />
          </>
        )}
        {tab === "notes" && <ProjectNotes />}
        <Foot close={close} note="Saved with the project. Changes can be undone." />
      </div>
    </SheetFrame>
  );
}
