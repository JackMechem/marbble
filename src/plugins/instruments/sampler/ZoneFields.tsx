// The selected zone's settings in the Sampler panel: keys, tuning, velocities, gain, pan, round-robin
// group and loop crossfade typed in small boxes, the loop and playback switches, and Slice, Replace…
// and Delete.
import { useState } from "react";
import { keyName, parseKey, pointsOf, type ZoneView } from "./samplerMap";

/** A number typed in a small box, which lands on Enter or leaving the box. */
function Field({ label, value, parse, show, onCommit, title, width = 46 }: { label: string; value: number; parse?: (t: string) => number | null; show?: (v: number) => string; onCommit: (v: number) => void; title?: string; width?: number }) {
  const text = show ? show(value) : String(Math.round(value * 100) / 100);
  const [edit, setEdit] = useState<string | null>(null);
  const commit = () => {
    if (edit !== null) {
      const v = parse ? parse(edit) : Number(edit);
      if (v !== null && Number.isFinite(v) && v !== value) onCommit(v);
    }
    setEdit(null);
  };
  return (
    <label className="smp-field" title={title}>
      <span>{label}</span>
      <input
        style={{ width }}
        value={edit ?? text}
        onChange={(e) => setEdit(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") setEdit(null);
        }}
      />
    </label>
  );
}

export function ZoneFields({ zone, update, remove, slice, replace }: { zone: ZoneView; update: (patch: Partial<ZoneView>) => void; remove: () => void; slice: (sensitivity: number) => void; replace: () => void }) {
  const [sens, setSens] = useState(50);
  const pts = pointsOf(zone);
  const ms = (f: number) => (zone.rate ? `${Math.round((f / zone.rate) * 10000) / 10}` : String(f));
  const key = (t: string) => parseKey(t);
  return (
    <div className="smp-fields">
      <Field label="Low Key" value={zone.key_lo} parse={key} show={keyName} onCommit={(v) => update({ key_lo: v })} />
      <Field label="High Key" value={zone.key_hi} parse={key} show={keyName} onCommit={(v) => update({ key_hi: v })} />
      <Field label="Root" value={zone.root} parse={key} show={keyName} onCommit={(v) => update({ root: v })} title="The key that plays the sample at its own pitch" />
      <Field label="Fine" value={zone.fine} onCommit={(v) => update({ fine: v })} title="Fine tune, in cents" />
      <Field label="Low Vel" value={zone.vel_lo} onCommit={(v) => update({ vel_lo: v })} />
      <Field label="High Vel" value={zone.vel_hi} onCommit={(v) => update({ vel_hi: v })} />
      <Field label="Gain" value={zone.gain_db} onCommit={(v) => update({ gain_db: v })} title="dB" />
      <Field label="Pan" value={zone.pan * 100} onCommit={(v) => update({ pan: v / 100 })} title="-100 left to 100 right" />
      <Field label="Group" value={zone.rr_group} onCommit={(v) => update({ rr_group: Math.round(v) })} title="Round robin: zones of the same group on the same key and velocity take turns. 0 is none." />
      <Field label="Fade ms" value={zone.xfade} parse={(t) => (zone.rate ? Math.round((Number(t) / 1000) * zone.rate) : Number(t))} show={ms} onCommit={(v) => update({ xfade: Math.max(0, v) })} title="The loop's crossfade, from the audio before its start. A loop at the very start of a sample has no room for one." />
      <label className="smp-check" title="Loop while the key's held">
        <input type="checkbox" checked={zone.loop_on} onChange={(e) => update(e.target.checked && zone.loop_start === 0 && zone.loop_end === 0 ? { loop_on: true, loop_start: pts.start, loop_end: pts.end } : { loop_on: e.target.checked })} /> Loop
      </label>
      <label className="smp-check" title="Crossfade at equal power, for loops whose two sides don't line up">
        <input type="checkbox" checked={zone.equal_power} onChange={(e) => update({ equal_power: e.target.checked })} /> Equal Power
      </label>
      <label className="smp-check" title="Plays to its end whatever the key does">
        <input type="checkbox" checked={zone.one_shot} onChange={(e) => update({ one_shot: e.target.checked })} /> One-Shot
      </label>
      <label className="smp-check">
        <input type="checkbox" checked={zone.reverse} onChange={(e) => update({ reverse: e.target.checked })} /> Reverse
      </label>
      <span className="smp-buttons">
        <button className="fx-toggle" title="Cut this zone at its transients, one slice per key from C1" onClick={() => slice(sens / 100)}>
          Slice
        </button>
        <input className="smp-sens" type="range" min={0} max={100} value={sens} title={`Slice sensitivity: ${sens} %`} onChange={(e) => setSens(Number(e.target.value))} />
        <button className="fx-toggle" title="Point this zone at another file" onClick={replace}>
          Replace…
        </button>
        <button className="fx-toggle" onClick={remove}>
          Delete
        </button>
      </span>
    </div>
  );
}
