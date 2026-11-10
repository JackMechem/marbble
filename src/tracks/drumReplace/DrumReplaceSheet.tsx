// The Drum Replacement/Doubling sheet (Logic's Track › Drum Replacement/Doubling): instrument,
// mode, threshold and timing, with the hits it will use marked on the audio track's regions
// while it's open. Create makes a sampler track below playing a note at each hit
// (crates/engine/src/session/drum_replace.rs).
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { type Id } from "../../engine/project";
import { actMade, api, call } from "../../engine/commands";
import { getState, useStore } from "../../state/store";
import { DRUM_DEFAULTS, DRUM_KINDS, hitSummary, keptHits, notePos, setDrumMarks, type DrumHit, type DrumKind, type DrumMode, type DrumSettings, type DrumTiming } from "./drumReplace";
import { noteName } from "../../editors/eventList/model";
import { Sheet } from "../../ui/Sheet";
import { NumberField, Select, Slider } from "../../ui/Form";

const PREFS = "marbble.drumReplace";

function loadSettings(): DrumSettings {
  try {
    return { ...DRUM_DEFAULTS, ...JSON.parse(localStorage.getItem(PREFS) ?? "{}") };
  } catch {
    return DRUM_DEFAULTS;
  }
}

function saveSettings(s: DrumSettings) {
  try {
    localStorage.setItem(PREFS, JSON.stringify(s));
  } catch {
    // Storage blocked: the settings just aren't remembered.
  }
}

/** The audio track, and the selected regions on it (none: all of it). */
interface Target {
  track: Id;
  regions: Id[];
}

let current: Target | null = null;
const subs = new Set<() => void>();
const emit = () => subs.forEach((f) => f());

/** Opens the sheet for an audio track: its selected regions, or every region on it. */
export function openDrumReplace(track: Id) {
  const s = getState();
  const t = s.snap?.project.tracks.find((x) => x.id === track);
  if (!t || t.kind !== "audio") return;
  const regions = t.regions.filter((r) => s.selectedRegions.includes(r.id)).map((r) => r.id);
  current = { track, regions };
  emit();
}

function close() {
  current = null;
  emit();
}

export function DrumReplaceHost() {
  const target = useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => current,
  );
  return target ? <DrumReplaceSheet target={target} /> : null;
}

const INSTRUMENTS: [DrumKind, string][] = DRUM_KINDS.map((k) => [k.id, `${k.name} (${noteName(k.note)})`]);
const MODES: [DrumMode, string][] = [
  ["replace", "Replace"],
  ["double", "Doubling"],
];
const TIMINGS: [DrumTiming, string][] = [
  ["attack", "Attack Start"],
  ["peak", "Peak"],
];

function DrumReplaceSheet({ target }: { target: Target }) {
  const [settings, setSettings] = useState<DrumSettings>(loadSettings);
  const [hits, setHits] = useState<DrumHit[] | null>(null);
  const [busy, setBusy] = useState(false);
  const track = useStore((s) => s.snap?.project.tracks.find((t) => t.id === target.track));
  const rate = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  const set = (p: Partial<DrumSettings>) => {
    const next = { ...settings, ...p };
    setSettings(next);
    saveSettings(next);
  };

  // Every candidate, found again when the track's audio changes under the sheet.
  const audioKey = JSON.stringify(track?.regions.map((r) => [r.id, r.start, r.length, r.muted, r.loop_end, r.content.type === "audio" ? [r.content.offset, r.content.gain_db] : 0, r.takes?.comp]));
  useEffect(() => {
    let live = true;
    call<DrumHit[]>("drum_hits", { track: target.track, regions: target.regions }).then((h) => live && setHits(h ?? []));
    return () => {
      live = false;
    };
  }, [target, audioKey]);

  const kept = useMemo(() => keptHits(hits ?? [], settings.threshold_db), [hits, settings.threshold_db]);
  // The markers, where the notes will go.
  useEffect(() => {
    setDrumMarks({ track: target.track, at: kept.map((h) => notePos(h, settings, rate)) });
  }, [kept, settings.timing, settings.offset_ms, rate, target.track]);
  useEffect(() => () => setDrumMarks(null), []);

  const create = async () => {
    setBusy(true);
    const made = await actMade("drum_replace", { track: target.track, regions: target.regions, settings });
    setBusy(false);
    if (!made?.length) return;
    api.selectTrack(made[0]);
    close();
  };

  const scope = target.regions.length ? `${target.regions.length} selected region${target.regions.length === 1 ? "" : "s"} on “${track?.name ?? ""}”` : `“${track?.name ?? ""}”, all of it`;
  return (
    <Sheet title="Drum Replacement/Doubling" onClose={close} width={480}>
      <div className="drum-replace-summary dim">{track ? scope : "The track is gone"}</div>
      <div className="settings-grid drum-replace-grid">
        <label>Instrument:</label>
        <Select value={settings.instrument} options={INSTRUMENTS} onChange={(v) => set({ instrument: v })} width={180} />
        <label>Mode:</label>
        <div className="row-inline">
          <Select value={settings.mode} options={MODES} onChange={(v) => set({ mode: v })} width={180} />
          <span className="dim">{settings.mode === "replace" ? "mutes the recorded track" : "keeps both playing"}</span>
        </div>
        <label>Threshold:</label>
        <Slider value={settings.threshold_db} min={-60} max={0} step={0.5} width={200} onChange={(v) => set({ threshold_db: v })} format={(v) => `${v.toFixed(1)} dB`} />
        <label>Notes At:</label>
        <Select value={settings.timing} options={TIMINGS} onChange={(v) => set({ timing: v })} width={180} />
        <label>Timing Offset:</label>
        <div className="row-inline">
          <NumberField value={settings.offset_ms} min={-50} max={50} step={0.5} onChange={(v) => set({ offset_ms: v })} />
          <span className="dim">ms (negative is earlier)</span>
        </div>
      </div>
      <div className="drum-replace-summary dim">{hits === null ? "Finding the hits…" : hitSummary(hits, kept)}</div>
      <div className="sheet-foot">
        <button className="push" onClick={() => set(DRUM_DEFAULTS)}>
          Defaults
        </button>
        <div className="sheet-buttons">
          <button className="push" onClick={close}>
            Cancel
          </button>
          <button className="push default" disabled={busy || !track || !kept.length} onClick={create}>
            Create Track
          </button>
        </div>
      </div>
    </Sheet>
  );
}
