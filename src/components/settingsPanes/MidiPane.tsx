// Settings › MIDI: inputs, the velocity curve and the input monitor.
import { useEffect, useState } from "react";
import { call } from "../../api";
import { useSettings, type VelocityCurve } from "../../prefs";
import { useStore } from "../../store";
import { Check, NumberField, Select } from "../Form";
import { Foot, Row, Section, setPrefs } from "./parts";
import { describeMidi } from "../../settingsText";

interface MidiPort {
  name: string;
  enabled: boolean;
  connected: boolean;
}

interface MidiLogEntry {
  seq: number;
  ms: number;
  port: string;
  data: [number, number, number];
}

function VelocityGraph({ curve, fixed }: { curve: VelocityCurve; fixed: number }) {
  const pts: string[] = [];
  for (let i = 0; i <= 32; i++) {
    const x = i / 32;
    const y = curve === "fixed" ? fixed / 127 : curve === "soft" ? Math.pow(x, 0.6) : curve === "hard" ? Math.pow(x, 1.7) : x;
    pts.push(`${(2 + x * 40).toFixed(1)},${(42 - y * 40).toFixed(1)}`);
  }
  return (
    <svg className="velocity-graph" width={44} height={44} viewBox="0 0 44 44">
      <rect x="2" y="2" width="40" height="40" rx="3" />
      <polyline points={pts.join(" ")} />
    </svg>
  );
}

export function MidiPane({ close }: { close: () => void }) {
  const m = useSettings((s) => s.prefs.midi);
  const target = useStore((s) => s.snap?.midi_target);
  const [ports, setPorts] = useState<MidiPort[] | null>(null);
  const [log, setLog] = useState<MidiLogEntry[]>([]);
  const loadPorts = () => call<MidiPort[]>("midi_ports").then((p) => p && setPorts(p));
  useEffect(() => {
    loadPorts();
    const t = window.setInterval(loadPorts, 3000);
    return () => window.clearInterval(t);
  }, [m.disabled_inputs]);
  useEffect(() => {
    let last = -1;
    const t = window.setInterval(async () => {
      const l = await call<MidiLogEntry[]>("midi_monitor");
      const top = l?.length ? l[l.length - 1].seq : -1;
      if (l && top !== last) {
        last = top;
        setLog(l.slice(-10).reverse());
      }
    }, 120);
    return () => window.clearInterval(t);
  }, []);
  const toggle = (name: string, on: boolean) =>
    setPrefs("midi", { disabled_inputs: on ? m.disabled_inputs.filter((n) => n !== name) : [...m.disabled_inputs, name] });
  return (
    <>
      <Section title="Inputs">
        <Row label="MIDI Inputs:" note="Every enabled input plays the selected software instrument track. New devices are picked up within a few seconds.">
          <div className="midi-ports">
            {ports === null ? null : ports.length === 0 ? (
              <span className="dim">No MIDI inputs found</span>
            ) : (
              ports.map((p) => (
                <div key={p.name} className="midi-port">
                  <Check checked={p.enabled} onChange={(v) => toggle(p.name, v)}>
                    {p.name}
                  </Check>
                  <span className={"status-dot " + (p.connected ? "ok" : "off")} title={p.connected ? "Connected" : "Not connected"} />
                </div>
              ))
            )}
          </div>
        </Row>
        <Row label="MIDI Thru:" note={m.thru ? `Live MIDI now plays ${target ? `“${target}”` : "the selected instrument track"}.` : "Live MIDI is recorded on record-enabled tracks but not heard while you play — for keyboards that make their own sound."}>
          <Check checked={m.thru} onChange={(v) => setPrefs("midi", { thru: v })}>
            Play the selected instrument track live
          </Check>
        </Row>
        <Row label="Velocity:" note="Shapes how hard you need to play. Applies to MIDI inputs, not to Musical Typing.">
          <div className="row-inline">
            <Select
              value={m.velocity}
              options={[
                ["linear", "As played"],
                ["soft", "Soft (light touch plays louder)"],
                ["hard", "Hard (needs a firmer touch)"],
                ["fixed", "Fixed velocity"],
              ]}
              onChange={(v) => setPrefs("midi", { velocity: v })}
            />
            {m.velocity === "fixed" && <NumberField value={m.fixed_velocity} min={1} max={127} onChange={(v) => setPrefs("midi", { fixed_velocity: Math.round(v) })} />}
            <VelocityGraph curve={m.velocity} fixed={m.fixed_velocity} />
          </div>
        </Row>
        <Row label="Chase:" note="When playback starts, jumps or cycles back in the middle of a region, notes already held there start, and the controllers (sustain, modulation, pitch bend…) are set as they were.">
          <div className="row-inline">
            <Check checked={m.chase_notes ?? true} onChange={(v) => setPrefs("midi", { chase_notes: v })}>
              Notes
            </Check>
            <Check checked={m.chase_controllers ?? true} onChange={(v) => setPrefs("midi", { chase_controllers: v })}>
              Controllers
            </Check>
          </div>
        </Row>
      </Section>
      <Section title="Activity">
        <Row label="Monitor:">
          <div className="midi-monitor">
            {log.length === 0 ? (
              <span className="dim">Play something on a MIDI input to see its messages here.</span>
            ) : (
              log.map((e, i) => {
                const [ch, what] = describeMidi(e.data);
                return (
                  <div key={e.seq} className={"midi-msg" + (i === 0 ? " fresh" : "")}>
                    <span className="midi-port-name">{e.port}</span>
                    <span>{ch}</span>
                    <span className="midi-what">{what}</span>
                  </div>
                );
              })
            )}
          </div>
        </Row>
      </Section>
      <Foot close={close} />
    </>
  );
}
