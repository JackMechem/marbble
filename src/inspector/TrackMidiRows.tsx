// The Track inspector's MIDI rows on a software instrument track, as in Logic: Transpose,
// Velocity, Key Limit, Velocity Limit and Delay. They change what the track plays (its regions
// and live input), not the notes it holds; each change is one undo step. The values and ranges
// are in trackMidiModel.ts.
import type { Track, TrackMidiSettings } from "../engine/project";
import { act } from "../engine/commands";
import { NumberField } from "../ui/Form";
import { DELAY_MS_MAX, DELAY_TICKS_MAX } from "../tracks/regions/regionParams";
import { Choice, DELAY_UNITS } from "./Choice";
import { formatKey, limitWith, midiSettingsOf, parseKey, TRANSPOSE_MAX, VELOCITY_MAX } from "./trackMidiModel";

type Patch = Partial<TrackMidiSettings>;

function Row({ label, title, children }: { label: string; title: string; children: React.ReactNode }) {
  return (
    <div className="insp-row" title={title}>
      <span className="insp-label">{label}</span>
      <span className="insp-value">{children}</span>
    </div>
  );
}

/** A low–high pair of fields, low above high; moving one end past the other takes the other along. */
function LimitFields({ limit, min, format, parse, onChange }: { limit: [number, number]; min: number; format?: (v: number) => string; parse?: (t: string) => number | null; onChange: (l: [number, number]) => void }) {
  return (
    <span className="insp-limit">
      <NumberField value={limit[0]} min={min} max={127} width={70} format={format} parse={parse} onChange={(v) => onChange(limitWith(limit, "low", v))} />
      <NumberField value={limit[1]} min={min} max={127} width={70} format={format} parse={parse} onChange={(v) => onChange(limitWith(limit, "high", v))} />
    </span>
  );
}

export function TrackMidiRows({ track }: { track: Track }) {
  const m = midiSettingsOf(track);
  const set = (patch: Patch) => act("set_track_midi", { id: track.id, patch });
  return (
    <>
      <Row label="Transpose" title="Semitones the track's notes play higher or lower">
        <NumberField value={m.transpose} min={-TRANSPOSE_MAX} max={TRANSPOSE_MAX} width={70} onChange={(transpose) => set({ transpose })} />
      </Row>
      <Row label="Velocity" title="Added to every note's velocity">
        <NumberField value={m.velocity} min={-VELOCITY_MAX} max={VELOCITY_MAX} width={70} onChange={(velocity) => set({ velocity })} />
      </Row>
      <Row label="Key Limit" title="The lowest and highest note the track plays">
        <LimitFields limit={m.key_limit} min={0} format={formatKey} parse={parseKey} onChange={(key_limit) => set({ key_limit })} />
      </Row>
      <Row label="Vel Limit" title="Velocity Limit: the softest and hardest note the track plays">
        <LimitFields limit={m.velocity_limit} min={1} onChange={(velocity_limit) => set({ velocity_limit })} />
      </Row>
      <Row label="Delay" title="Plays the track earlier (negative) or later">
        <NumberField
          value={m.delay}
          step={m.delay_ms ? 1 : 10}
          resolution={m.delay_ms ? 0.1 : 1}
          min={m.delay_ms ? -DELAY_MS_MAX : -DELAY_TICKS_MAX}
          max={m.delay_ms ? DELAY_MS_MAX : DELAY_TICKS_MAX}
          width={70}
          onChange={(delay) => set({ delay })}
        />
        <Choice value={m.delay_ms ? 1 : 0} options={DELAY_UNITS} onChange={(v) => set({ delay_ms: v === 1 })} />
      </Row>
    </>
  );
}
