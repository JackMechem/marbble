// Settings › MIDI › Control Surfaces: a Mackie Control and its extenders, or a HUI, each on an
// input and an output port, with its first strip and (on a main unit) F1–F8's key commands.
import { useEffect, useState } from "react";
import { call } from "../../engine/commands";
import type { MidiPort } from "../../controllers/controllers";
import { midiCommands } from "../../keyboard/keymap";
import { useSettings } from "../appSettings";
import {
  FUNCTION_KEYS,
  hasFunctionKeys,
  kindName,
  newSurface,
  portOptions,
  setFunction,
  STRIPS,
  statusText,
  SURFACE_KINDS,
  type SurfaceConfig,
  type SurfaceStatus,
} from "../../controllers/surfaces";
import { NumberField, Select } from "../../ui/Form";
import { Row, Section, setPrefs } from "./parts";

const COMMANDS: [string, string][] = [["", "None"], ...midiCommands().map((c): [string, string] => [c.id, `${c.group} › ${c.label}`])];

export function ControlSurfaces({ inputs, outputs }: { inputs: MidiPort[]; outputs: MidiPort[] }) {
  const units = useSettings((s) => s.prefs.midi.surfaces) ?? [];
  const [status, setStatus] = useState<SurfaceStatus[]>([]);
  useEffect(() => {
    const load = () => call<SurfaceStatus[]>("control_surfaces").then((s) => s && setStatus(s));
    load();
    const t = window.setInterval(load, 1000);
    return () => window.clearInterval(t);
  }, [units]);
  const save = (next: SurfaceConfig[]) => setPrefs("midi", { surfaces: next });
  const change = (i: number, patch: Partial<SurfaceConfig>) => save(units.map((u, k) => (k === i ? { ...u, ...patch } : u)));
  const add = () =>
    save([
      ...units,
      newSurface(
        units,
        inputs.map((p) => p.key),
        outputs.map((p) => p.key),
      ),
    ]);
  return (
    <Section title="Control Surfaces">
      <Row
        label="Surfaces:"
        note="A Mackie Control (with its extenders) or a HUI. Its eight strips show eight tracks, from the first strip on: Bank and Channel move them along the track list. Its ports play no instrument and take no assignments."
      >
        <div className="surfaces">
          {units.map((u, i) => {
            const s = status[i];
            return (
              <div key={i} className="surface-unit">
                <div className="surface-head">
                  <Select value={u.kind} options={SURFACE_KINDS} onChange={(kind) => change(i, { kind })} width={210} title="Surface type" />
                  <span className="surface-strips">
                    First strip
                    <NumberField value={u.offset + 1} min={1} max={121} width={52} onChange={(v) => change(i, { offset: Math.max(0, Math.round(v) - 1) })} title="The track (from the bank's first) its first strip shows" />
                    <span className="dim">
                      to {u.offset + STRIPS}
                    </span>
                  </span>
                  <button type="button" className="small-btn" onClick={() => save(units.filter((_, k) => k !== i))} title={`Remove this ${kindName(u.kind)}`}>
                    Remove
                  </button>
                </div>
                <div className="surface-ports">
                  <span className="dim">Input</span>
                  <Select value={u.input} options={portOptions(inputs, u.input)} onChange={(input) => change(i, { input })} width={250} title={`${kindName(u.kind)} input`} />
                  <span className="dim">Output</span>
                  <Select value={u.output} options={portOptions(outputs, u.output)} onChange={(output) => change(i, { output })} width={250} title={`${kindName(u.kind)} output`} />
                </div>
                <div className="surface-status">
                  <span className={"status-dot " + (s?.online ? "ok" : "off")} />
                  <span>{statusText(s, u)}</span>
                </div>
                {hasFunctionKeys(u.kind) && (
                  <details className="surface-functions">
                    <summary>Function keys F1–F8</summary>
                    <div className="surface-function-grid">
                      {Array.from({ length: FUNCTION_KEYS }, (_, k) => (
                        <div key={k} className="surface-function">
                          <span className="dim">F{k + 1}</span>
                          <Select value={u.functions[k] ?? ""} options={COMMANDS} onChange={(id) => change(i, { functions: setFunction(u.functions, k, id) })} width={230} title={`F${k + 1}: the key command it runs`} />
                        </div>
                      ))}
                    </div>
                  </details>
                )}
              </div>
            );
          })}
          <div>
            <button type="button" className="small-btn" onClick={add}>
              Add Surface
            </button>
          </div>
        </div>
      </Row>
    </Section>
  );
}
