// Settings › MIDI: inputs, outputs and controller feedback, the velocity curve and the input
// monitor.
import { useEffect, useState } from "react";
import { call } from "../../api";
import { feedbackChoice, feedbackOptions, feedbackSetting, switchPort, type MidiPort } from "../../controllers";
import { useSettings, type VelocityCurve } from "../../prefs";
import { useStore } from "../../store";
import { Check, NumberField, Select } from "../Form";
import { Foot, Row, Section, setPrefs } from "./parts";
import { describeMidi } from "../../settingsText";

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
  const [outputs, setOutputs] = useState<MidiPort[] | null>(null);
  const [log, setLog] = useState<MidiLogEntry[]>([]);
  const loadPorts = () => {
    call<MidiPort[]>("midi_ports").then((p) => p && setPorts(p));
    call<MidiPort[]>("midi_outputs").then((p) => p && setOutputs(p));
  };
  useEffect(() => {
    loadPorts();
    const t = window.setInterval(loadPorts, 3000);
    return () => window.clearInterval(t);
  }, [m.disabled_inputs, m.disabled_outputs]);
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
  // Ports are switched off by key, so they stay off when replugged under new ALSA numbers.
  const toggle = (key: string, on: boolean) => setPrefs("midi", { disabled_inputs: switchPort(m.disabled_inputs, key, on) });
  const toggleOutput = (key: string, on: boolean) => setPrefs("midi", { disabled_outputs: switchPort(m.disabled_outputs ?? [], key, on) });
  const connectedOutputs = (outputs ?? []).filter((o) => o.connected);
  // One row per input device (ALSA lists a port again under a new number when replugged).
  const inputs = (ports ?? []).filter((p, i, all) => all.findIndex((q) => q.key === p.key) === i);
  const pair = (input: string, choice: string) => {
    const next = { ...(m.feedback_outputs ?? {}) };
    const v = feedbackSetting(choice);
    if (v === undefined) delete next[input];
    else next[input] = v;
    setPrefs("midi", { feedback_outputs: next });
  };
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
                  <Check checked={p.enabled} onChange={(v) => toggle(p.key, v)}>
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
      <Section title="Outputs">
        <Row label="MIDI Outputs:" note="Controller feedback (button lights, motor faders) goes out through these. New devices are picked up within a few seconds.">
          <div className="midi-ports">
            {outputs === null ? null : outputs.length === 0 ? (
              <span className="dim">No MIDI outputs found</span>
            ) : (
              outputs.map((p) => (
                <div key={p.name} className="midi-port">
                  <Check checked={p.enabled} onChange={(v) => toggleOutput(p.key, v)}>
                    {p.name}
                  </Check>
                  <span className={"status-dot " + (p.connected ? "ok" : "off")} title={p.connected ? "Connected" : "Not connected"} />
                </div>
              ))
            )}
          </div>
        </Row>
        <Row
          label="Feedback:"
          note="Where each input's controller assignments send the values back, so lights and motor faders follow Marbble. Automatic picks the output of the same device. Each assignment can turn it off in Controller Assignments."
        >
          <div className="midi-feedback">
            {inputs.length === 0 ? (
              <span className="dim">No MIDI inputs found</span>
            ) : (
              inputs.map((p) => {
                const choice = feedbackChoice(m.feedback_outputs, p.key);
                return (
                  <div key={p.key} className="midi-feedback-row">
                    <span className="midi-port-name" title={p.key}>
                      {p.key}
                    </span>
                    <Select value={choice} options={feedbackOptions(p, connectedOutputs, choice)} onChange={(c) => pair(p.key, c)} width={240} title={`Feedback for ${p.key}`} />
                  </div>
                );
              })
            )}
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
