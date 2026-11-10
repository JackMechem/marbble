// Marbble Sampler's panel, laid out left to right for the wide, short dock: the zones on a keyboard
// map, then the selected zone's waveform and settings, then the instrument's parameter sections in
// a row (the parts are ZoneMap.tsx, ZoneWaveform.tsx and ZoneFields.tsx). It scrolls sideways when
// the dock is narrower than that, and works floated. Recording a sample into a zone is here too.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "../../../project/fileDialog";
import { Select } from "../../../ui/Form";
import { type Id, type InputSel } from "../../../engine/project";
import { type Snapshot } from "../../../engine/snapshot";
import { act, call } from "../../../engine/commands";
import type { FxPanelProps } from "../../builtinEffects";
import { findFxSlot } from "../../fxHost";
import { dropOnSampler } from "./samplerDrop";
import { memorySize, keyName, toZone, type SamplerView, type ZoneView } from "./samplerMap";
import { setState, showToast, useStore } from "../../../state/store";
import { AUDIO_EXTENSIONS } from "../../../project/fileDrop";
import { FxGroup } from "../../panels/panelControls";
import { InstSection, wheelAcross } from "../Instrument";
import { ZoneFields } from "./ZoneFields";
import { ZoneMap } from "./ZoneMap";
import { ZoneWaveform } from "./ZoneWaveform";

type Result<T> = { snapshot: Snapshot; result: T };

/** A command that returns a snapshot along with something to say. */
async function callSnap<T>(cmd: string, args: Record<string, unknown>): Promise<T | null> {
  const r = await call<Result<T>>(cmd, args);
  if (!r) return null;
  setState({ snap: r.snapshot });
  if (r.snapshot.warning) showToast(r.snapshot.warning, 8000);
  return r.result;
}

function inputOptions(channels: number): { label: string; value: InputSel }[] {
  const out: { label: string; value: InputSel }[] = [];
  for (let c = 0; c < channels; c++) out.push({ label: `Input ${c + 1}`, value: { type: "mono", ch: c } });
  for (let c = 0; c + 1 < channels; c += 2) out.push({ label: `Input ${c + 1}-${c + 2}`, value: { type: "stereo", ch: c } });
  return out;
}

function RecordControls({ track, onDone }: { track: Id; onDone: (zone: number) => void }) {
  const channels = useStore((s) => s.snap?.audio.input_channels ?? 0);
  const options = inputOptions(channels);
  const [choice, setChoice] = useState(0);
  const [trim, setTrim] = useState(true);
  const [status, setStatus] = useState<{ secs: number; peak: number } | null>(null);
  useEffect(() => {
    if (!status) return;
    const t = window.setInterval(async () => {
      const r = await call<{ secs: number; peak: number } | null>("sampler_record_status");
      if (r) setStatus((old) => ({ secs: r.secs, peak: Math.max(r.peak, (old?.peak ?? 0) * 0.8) }));
    }, 100);
    return () => window.clearInterval(t);
  }, [!!status]); // eslint-disable-line react-hooks/exhaustive-deps

  const start = async () => {
    const input = options[choice]?.value;
    if (!input) return showToast("No audio input is open. Choose one in Settings › Audio.", 5000);
    try {
      await invoke("sampler_record_start", { track, input });
      setStatus({ secs: 0, peak: 0 });
    } catch (e) {
      showToast(String(e), 5000);
    }
  };
  const stop = async () => {
    setStatus(null);
    const zone = await callSnap<number>("sampler_record_stop", { trim, atKey: null });
    if (zone !== null) onDone(zone);
  };
  return (
    <>
      {status ? (
        <>
          <button className="fx-toggle smp-rec on" title="Stop, and make what came in playable" onClick={stop}>
            ■ Stop
          </button>
          <button
            className="fx-toggle"
            title="Stop and throw it away"
            onClick={() => {
              setStatus(null);
              call("sampler_record_cancel");
            }}
          >
            Cancel
          </button>
          <span className="smp-rec-time">{status.secs.toFixed(1)} s</span>
          <span className="smp-rec-level">
            <span style={{ width: `${Math.min(100, status.peak * 100)}%` }} className={status.peak >= 0.99 ? "over" : ""} />
          </span>
        </>
      ) : (
        <>
          <button className="fx-toggle smp-rec" title="Record from the audio input into this sampler. The transport stays as it is." onClick={start}>
            ● Record
          </button>
          <Select className="smp-input" value={choice} options={options.length ? options.map((o, i) => [i, o.label]) : [[0, "No input"]]} onChange={setChoice} title="The input to record from" />
          <label className="smp-check" title="Move the start and end in past the silence. The file keeps everything.">
            <input type="checkbox" checked={trim} onChange={(e) => setTrim(e.target.checked)} /> Trim
          </label>
        </>
      )}
    </>
  );
}

export function SamplerPanel(props: FxPanelProps) {
  const { info, track, uid, meters } = props;
  const fingerprint = useStore((s) => findFxSlot(s.snap?.project, uid)?.plugin.state);
  const [view, setView] = useState<SamplerView | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [notes, setNotes] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    call<SamplerView>("sampler_view", { track }).then((v) => {
      if (!live || !v) return;
      setView(v);
      setSelected((cur) => (v.zones.some((z) => z.id === cur) ? cur : v.zones[0]?.id ?? null));
    });
    return () => {
      live = false;
    };
  }, [track, fingerprint]);

  const zones = view?.zones ?? [];
  const zone = zones.find((z) => z.id === selected) ?? null;
  const update = (z: ZoneView, merge?: string) => {
    setView((v) => v && { ...v, zones: v.zones.map((x) => (x.id === z.id ? z : x)) });
    act("sampler_update_zones", { track, zones: [toZone(z)], merge: merge ?? null });
  };
  const addFiles = async () => {
    const picked = await open({ title: "Add Samples", multiple: true, filters: [{ name: "Audio or SFZ", extensions: [...AUDIO_EXTENSIONS, "sfz"] }] });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    const report = await dropOnSampler(track, paths, null);
    if (report) setNotes(report);
  };
  const replace = async () => {
    if (!zone) return;
    const picked = await open({ title: `Replace ${zone.name}`, filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }] });
    if (typeof picked === "string") act("sampler_replace_file", { track, zone: zone.id, path: picked });
  };
  const slice = async (sensitivity: number) => {
    if (!zone) return;
    const n = await callSnap<number>("sampler_slice", { track, zone: zone.id, sensitivity });
    if (n !== null) showToast(`${n} slice${n === 1 ? "" : "s"}, from C1 up`, 3000);
  };
  const broken = zones.filter((z) => z.error);
  const gb = (b: number) => (b / (1 << 30)).toFixed(b < 1 << 30 ? 2 : 1);
  return (
    <div className="fx-panel smp-panel" data-sampler-track={track}>
      <div className="smp-row" onWheel={wheelAcross}>
        <FxGroup title="Zones" className="smp-col smp-zones">
          <div className="smp-tools">
            <button className="fx-toggle" title="Add audio files, or load an SFZ in place of the zones. You can drop them here too." onClick={addFiles}>
              Add…
            </button>
            <RecordControls track={track} onDone={setSelected} />
          </div>
          <div className="smp-map-box" data-sampler-map="1">
            <ZoneMap zones={zones} selected={selected} playing={meters[2] ?? -1} track={track} onSelect={setSelected} onChange={(z) => update(z)} />
            {!zones.length && <div className="smp-empty">Drop audio or SFZ files here, choose Add…, or Record</div>}
          </div>
          {broken.length > 0 ? (
            <div className="smp-status bad" title={broken.map((z) => `${z.file}: ${z.error}`).join("\n")}>
              {broken.length} sample{broken.length > 1 ? "s" : ""} can't play: {broken[0].file} ({broken[0].error}){broken.length > 1 ? " and more" : ""}. The rest play.
            </div>
          ) : notes ? (
            <div className="smp-status" title={notes} onClick={() => setNotes(null)}>
              {notes}
            </div>
          ) : (
            <div className="smp-status">
              {zones.length} zone{zones.length === 1 ? "" : "s"}, {memorySize(view?.memory ?? 0)} of {gb(view?.limit ?? 0)} GB in memory, {meters[0] ?? 0} voices
            </div>
          )}
        </FxGroup>
        <FxGroup title={zone ? `Zone: ${zone.name}` : "Zone"} className="smp-col smp-wave">
          {zones.length > 1 && (
            <Select
              className="smp-zone-pick"
              value={selected ?? -1}
              options={zones.map((z) => [z.id, `${z.error ? `(can't play) ${z.name}` : z.name} ${keyName(z.key_lo)}–${keyName(z.key_hi)}`])}
              onChange={setSelected}
              title="Choose a zone, for those behind others on the map"
            />
          )}
          {zone ? (
            <>
              <ZoneWaveform track={track} zone={zone} onChange={(z) => update(z)} />
              <ZoneFields
                zone={zone}
                update={(patch) => update({ ...zone, ...patch })}
                remove={() => act("sampler_delete_zones", { track, ids: [zone.id] })}
                slice={slice}
                replace={replace}
              />
            </>
          ) : (
            <div className="smp-empty">No zone selected</div>
          )}
        </FxGroup>
        {(info.sections ?? []).map((s) => (
          <InstSection key={s.title} {...props} title={s.title} params={s.params} className="smp-col" />
        ))}
      </div>
    </div>
  );
}

