// Settings › Audio: the devices, sample rate and buffer, processing threads, memory locking, plug-in
// latency, and the warning when the system mixer turns the input down. Changes wait for Apply.
import { useEffect, useState } from "react";
import { threadNote, threadOptions, type ThreadChoices } from "./threads";
import { memoryLockNote, offersMemoryLock, type MemoryLock } from "./memoryLock";
import { ask } from "@tauri-apps/plugin-dialog";
import { type Tick } from "../../engine/snapshot";
import { supportedRates, type AudioConfig, type DeviceList, type InputLevel } from "../../engine/devices";
import { act, call } from "../../engine/commands";
import { NumberField, Select } from "../../ui/Form";
import { onTick, setState, useStore } from "../../state/store";
import { Foot } from "./parts";
import { PdcSettings } from "../../mixer/Latency";

function LatencyReadout() {
  const [lat, setLat] = useState<Tick["latency"] | null>(null);
  useEffect(() => onTick((t) => setLat((p) => (p && Math.abs(p.compensation_ms - t.latency.compensation_ms) < 0.05 ? p : t.latency))), []);
  if (!lat || (lat.input_ms === 0 && lat.output_ms === 0)) return null;
  return (
    <div className="latency-readout">
      <span>
        Input <b>{lat.input_ms.toFixed(1)} ms</b>
      </span>
      <span>
        Output <b>{lat.output_ms.toFixed(1)} ms</b>
      </span>
      <span>
        Recordings shifted <b>{lat.compensation_ms.toFixed(1)} ms</b> earlier to line up with what you heard
      </span>
    </div>
  );
}

/** Warns when the sound server's own input volume is attenuating recordings. */
function InputLevelCheck() {
  const audio = useStore((s) => s.snap?.audio);
  const [level, setLevel] = useState<InputLevel | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    // Read again now and then and when the window comes back: the level can be changed in the
    // system mixer while this pane is open.
    const read = () => call<InputLevel | null>("input_level").then(setLevel);
    read();
    const timer = window.setInterval(read, 5000);
    window.addEventListener("focus", read);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", read);
    };
  }, [audio?.input_name, audio?.host]);
  if (!level?.attenuating) return null;
  const pct = Math.round(level.volume * 100);
  return (
    <div className="level-warning">
      <div>
        <b>
          {level.device} is {level.muted ? "muted" : `set to ${pct}%`} in the system mixer
        </b>
        {level.muted
          ? ", so Marbble receives silence."
          : `, which lowers everything you record by ${Math.abs(level.gain_db).toFixed(0)} dB before it reaches Marbble.`}{" "}
        Recording software expects 100% — set your level with the gain knob on the interface instead.
      </div>
      <button
        className="push default"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          const l = await call<InputLevel | null>("reset_input_level");
          setBusy(false);
          // No answer (the mixer couldn't be set or read back): keep the warning up rather than
          // hiding it as if it were fixed. Errors were reported by call().
          if (!l) return;
          setLevel(l);
          if (!l.attenuating) setState({ toast: { text: `${l.device} input set to 100%`, at: Date.now() } });
        }}
      >
        {busy ? "Setting…" : "Set to 100%"}
      </button>
    </div>
  );
}

/** Audio settings changed but not applied yet. Kept while Settings is open, so switching tabs
 *  doesn't lose them; dropped when it closes (see SettingsDialog). */
let audioDraft: AudioConfig | null = null;

export function AudioPane({ close }: { close: () => void }) {
  const audio = useStore((s) => s.snap?.audio);
  const project = useStore((s) => s.snap?.project);
  const [devices, setDevices] = useState<DeviceList | null>(null);
  const [cfg, setCfgState] = useState<AudioConfig | null>(audioDraft);
  const [applied, setApplied] = useState<AudioConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [threads, setThreads] = useState<ThreadChoices | null>(null);
  const [memLock, setMemLock] = useState<MemoryLock | null>(null);
  const setCfg = (c: AudioConfig) => {
    audioDraft = c;
    setCfgState(c);
  };
  useEffect(() => {
    call<AudioConfig>("get_audio_config").then((c) => {
      setApplied(c);
      if (c && !audioDraft) setCfgState(c);
    });
    call<ThreadChoices>("thread_choices").then(setThreads);
    call<MemoryLock>("memory_lock").then(setMemLock);
  }, []);
  // New threads ask for real-time priority as they start, so what was left out is known a moment later.
  const refreshThreads = () => setTimeout(() => call<ThreadChoices>("thread_choices").then(setThreads), 500);
  useEffect(() => {
    if (cfg) call<DeviceList>("audio_devices", { host: cfg.host }).then(setDevices);
  }, [cfg?.host]);
  if (!cfg) return null;
  const rates = supportedRates(cfg, devices);
  const changed = applied !== null && JSON.stringify(cfg) !== JSON.stringify(applied);
  const bufferMs = ((cfg.buffer_size / cfg.sample_rate) * 1000).toFixed(1);
  const hostLabel = (h: { name: string; devices: number }) =>
    h.name === "JACK" && h.devices === 0 ? "JACK (no server running)" : h.name === "ALSA" ? "ALSA (direct hardware)" : h.name;
  const apply = async (): Promise<boolean> => {
    // The project runs at the device's rate, and its audio files play unconverted: warn as
    // Project Settings does before a rate change makes recorded audio play at the wrong speed.
    const sr = project?.sample_rate;
    if (sr && cfg.sample_rate !== sr && project.tracks.some((t) => t.regions.some((r) => r.content.type === "audio"))) {
      const ok = await ask(
        `This project's audio was recorded at ${(sr / 1000).toFixed(1)} kHz. Marbble plays audio files without converting them, so at ${(cfg.sample_rate / 1000).toFixed(1)} kHz they would play ${cfg.sample_rate > sr ? "faster and higher" : "slower and lower"}.`,
        { title: "Change Sample Rate", kind: "warning", okLabel: "Change Anyway", cancelLabel: "Cancel" },
      );
      if (!ok) return false;
    }
    setBusy(true);
    const snap = await act("set_audio_config", { config: cfg });
    setBusy(false);
    if (snap) {
      audioDraft = null;
      setApplied(cfg);
      refreshThreads();
      call<MemoryLock>("memory_lock").then(setMemLock);
    }
    return !!snap;
  };
  // Done applies what's still pending, like OK in a settings dialog.
  const done = async () => {
    if (!changed || (await apply())) close();
  };
  return (
    <>
      <div className="settings-grid">
        <label>Driver:</label>
        <Select
          value={cfg.host ?? devices?.host ?? ""}
          options={(devices?.hosts ?? []).map((h) => [h.name, hostLabel(h)] as [string, string])}
          onChange={(h) => setCfg({ ...cfg, host: h, input: null, output: null })}
        />
        <span />
        <span className="form-note">PipeWire is the recommended driver on Linux.</span>
        <label>Output Device:</label>
        <Select
          value={cfg.output ?? ""}
          options={[["", "System Default"] as [string, string], ...(devices?.outputs ?? []).map((d) => [d.id, `${d.name} (${d.channels} ch)`] as [string, string])]}
          onChange={(v) => setCfg({ ...cfg, output: v || null })}
        />
        <label>Input Device:</label>
        <Select
            value={cfg.input_enabled ? cfg.input ?? "" : "__off"}
            options={[
              ["__off", "None (playback only)"] as [string, string],
              ["", "System Default"] as [string, string],
              ...(devices?.inputs ?? []).map((d) => [d.id, `${d.name} (${d.channels} ch)`] as [string, string]),
            ]}
            onChange={(v) => setCfg({ ...cfg, input_enabled: v !== "__off", input: v === "__off" || v === "" ? null : v })}
          />
        <label>Sample Rate:</label>
        <Select value={cfg.sample_rate} options={rates.map((r) => [r, `${(r / 1000).toFixed(1)} kHz`] as [number, string])} onChange={(r) => setCfg({ ...cfg, sample_rate: r })} />
        <label>I/O Buffer Size:</label>
        <div className="row-inline">
          <Select value={cfg.buffer_size} options={[32, 64, 128, 256, 512, 1024, 2048].map((b) => [b, `${b} samples`] as [number, string])} onChange={(b) => setCfg({ ...cfg, buffer_size: b })} />
          <span className="dim">{bufferMs} ms per buffer</span>
        </div>
        <label>Processing Threads:</label>
        {threads ? <Select value={cfg.processing_threads} options={threadOptions(threads)} onChange={(n) => setCfg({ ...cfg, processing_threads: n })} /> : <span />}
        <span />
        <span className="form-note">
          How many threads play tracks at once. Automatic uses more only when a project needs them; 1 keeps everything on the audio thread. Changes take effect
          without restarting audio.
        </span>
        {threads && threadNote(threads) && (
          <>
            <span />
            <span className="warn">{threadNote(threads)}</span>
          </>
        )}
        <label>Monitoring:</label>
        <label className="check">
          <input type="checkbox" checked={cfg.auto_monitor} onChange={(e) => setCfg({ ...cfg, auto_monitor: e.target.checked })} />
          Hear record-enabled tracks without pressing I
        </label>
        <span />
        <span className="form-note">
          Leave this off if your interface or pedal monitors directly — otherwise you hear a second, delayed copy, which sounds hollow
          or phasey. Use a track's I button to hear its input through Marbble (and its plug-ins).
        </span>
        <label>Recording Delay:</label>
        <div className="row-inline">
          <NumberField value={cfg.recording_delay_ms} step={0.5} resolution={0.1} min={-500} max={500} onChange={(v) => setCfg({ ...cfg, recording_delay_ms: v })} />
          <span className="dim">ms, added to the measured latency</span>
        </div>
        {offersMemoryLock(memLock) && (
          <>
            <label>Memory:</label>
            <label className="check">
              <input type="checkbox" checked={cfg.lock_memory} onChange={(e) => setCfg({ ...cfg, lock_memory: e.target.checked })} />
              Lock audio memory (keeps the audio from dropping out when the system swaps)
            </label>
            <span />
            <span className={memoryLockNote(memLock).warn ? "warn" : "form-note"}>{memoryLockNote(memLock).text}</span>
          </>
        )}
        <PdcSettings />
      </div>
      <LatencyReadout />
      <InputLevelCheck />
      <div className="settings-status">
        {audio?.running ? (
          <>
            <span className="status-dot ok" />
            Running on {audio.host}: {audio.output_name} → {audio.output_channels} out
            {audio.input_name ? `, ${audio.input_name} → ${audio.input_channels} in` : ", no input"} @ {audio.sample_rate} Hz, {audio.buffer_size} samples
          </>
        ) : (
          <>
            <span className="status-dot bad" />
            Audio stopped{audio?.error ? `: ${audio.error}` : ""}
          </>
        )}
        {audio?.error && audio.running && <div className="warn">{audio.error}</div>}
      </div>
      {/* Done applies the changes too, so it stays on the right as in every pane. */}
      <Foot close={done} busy={busy} note="Changes apply with Apply Changes or Done.">
        <button
          className="push"
          disabled={busy}
          title="Close and reopen the audio devices, e.g. after an interface was replugged"
          onClick={async () => {
            setBusy(true);
            await act("restart_audio");
            setBusy(false);
            refreshThreads();
          }}
        >
          Reset Audio Engine
        </button>
        <button className="push" disabled={busy || !changed} onClick={apply}>
          {busy ? "Applying…" : "Apply Changes"}
        </button>
      </Foot>
    </>
  );
}

/** Forgets the unapplied audio settings: the Settings window is closing. */
export function forgetAudioDraft() {
  audioDraft = null;
}
