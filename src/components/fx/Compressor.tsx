// The Compressor's panel: Logic's layout of a gain-reduction meter over the circuit selector,
// big Threshold and Ratio controls, the timing and gain knobs, a transfer curve, and the side
// chain and output sections. Parameter ids and curve maths are in compressorModel.ts, which mirrors
// crates/engine/src/fx/compressor.rs; the small controls, shared with the Limiter, in dynControls.tsx.
import { useLayoutEffect, useRef, useState } from "react";
import type { FxPanelProps } from "../../fx";
import { Circuit, CIRCUITS, FET_ATTACK, FET_RELEASE, P, VARI_MU_TC, VCA_ATTACKS, VCA_RELEASES } from "../../compressorModel";
import { formatMs } from "../../dynScale";
import { GrBar, GrNeedle, LevelBar, ParamChoice, ParamKnob, ParamToggle, Section, DynKnob } from "./dynControls";
import { TransferGraph } from "./TransferGraph";

export function CompressorPanel(props: FxPanelProps) {
  const { info, values, meters } = props;
  const v = (id: number) => values[id] ?? info.params.find((p) => p.id === id)?.default ?? 0;
  const circuit = Math.round(v(P.circuit)) as Circuit;
  const meta = CIRCUITS[circuit] ?? CIRCUITS[0];
  const [mIn, mOut, mGr, mLevel] = [meters[0] ?? -120, meters[1] ?? -120, meters[2] ?? 0, meters[3] ?? -120];
  const setNow = (id: number, value: number) => {
    props.touch(id, true);
    props.set(id, value);
    props.touch(id, false);
  };

  const thrSpec = info.params.find((p) => p.id === P.threshold);
  const opto = circuit === Circuit.Opto;
  const fet = circuit === Circuit.Fet;
  const varimu = circuit === Circuit.VariMu;
  const digital = circuit === Circuit.Platinum || circuit === Circuit.Vca;
  const tc = VARI_MU_TC[Math.round(v(P.timeConstant))] ?? VARI_MU_TC[0];

  // The meter row takes the height a taller panel has to spare; the transfer curve grows with
  // it (square, up to two fifths of the row) while the GR meter takes the rest of the width.
  const top = useRef<HTMLDivElement>(null);
  const [graph, setGraph] = useState(136);
  useLayoutEffect(() => {
    const el = top.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => {
      const { width, height } = e.contentRect;
      setGraph(Math.max(136, Math.min(Math.floor(height) - 6, Math.round(0.4 * width))));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const ratioControl = fet ? (
    <div className="dyn-ratio-buttons">
      <div className="dyn-knob-label">Ratio</div>
      <ParamChoice props={props} id={P.fetRatio} labels={["4", "8", "12", "20", "All"]} vertical />
    </div>
  ) : opto ? (
    <div className="dyn-opto-switch">
      <div className="dyn-knob-label">Mode</div>
      <ParamChoice props={props} id={P.optoMode} vertical />
    </div>
  ) : (
    <ParamKnob props={props} id={P.ratio} size={74} />
  );

  // The opto face shows the threshold as the LA-2A's Peak Reduction: 0–100, clockwise for
  // more reduction.
  const threshold = opto ? (
    <DynKnob
      label="Peak Reduction"
      value={(-v(P.threshold) / 60) * 100}
      min={0}
      max={100}
      def={(-(thrSpec?.default ?? -20) / 60) * 100}
      format={(pr) => pr.toFixed(0)}
      onChange={(pr) => props.set(P.threshold, Math.round(-pr * 6) / 10)}
      onGrab={(on) => props.touch(P.threshold, on)}
      size={74}
    />
  ) : (
    <ParamKnob props={props} id={P.threshold} size={74} />
  );

  const attack =
    circuit === Circuit.Vca ? (
      <ParamKnob props={props} id={P.attack} steps={VCA_ATTACKS} range={[VCA_ATTACKS[0], VCA_ATTACKS[VCA_ATTACKS.length - 1]]} />
    ) : fet ? (
      <ParamKnob props={props} id={P.attack} range={FET_ATTACK} />
    ) : opto ? (
      <ParamKnob props={props} id={P.attack} disabled note="10 ms" title="The optical cell's attack is fixed (about 10 ms)" />
    ) : varimu ? (
      <ParamKnob props={props} id={P.attack} disabled note={formatMs(tc.attack)} title="Set by the Time Constant" />
    ) : (
      <ParamKnob props={props} id={P.attack} />
    );
  const autoRel = v(P.autoRelease) >= 0.5;
  const release =
    circuit === Circuit.Vca ? (
      autoRel ? (
        <ParamKnob props={props} id={P.release} disabled note="Auto" />
      ) : (
        <ParamKnob props={props} id={P.release} steps={VCA_RELEASES} range={[VCA_RELEASES[0], VCA_RELEASES[VCA_RELEASES.length - 1]]} />
      )
    ) : fet ? (
      <ParamKnob props={props} id={P.release} range={FET_RELEASE} title="Program-dependent: recovers more slowly after heavy compression" />
    ) : opto ? (
      <ParamKnob props={props} id={P.release} disabled note="T4 cell" title="Two-stage, program-dependent: ~60 ms, then 1–15 s" />
    ) : varimu ? (
      <ParamKnob props={props} id={P.release} disabled note={tc.release} title="Set by the Time Constant" />
    ) : (
      <ParamKnob props={props} id={P.release} />
    );

  return (
    <div className={`dyn-panel dyn-comp circuit-${meta.key}`}>
      <div className="dyn-circuits" role="tablist">
        {CIRCUITS.map((c, i) => (
          <button key={c.key} className={i === circuit ? "on" : ""} title={c.about} onClick={() => setNow(P.circuit, i)}>
            {c.short}
          </button>
        ))}
      </div>

      <div className="dyn-top" ref={top}>
        <LevelBar db={mIn} label="In" length={108} grow />
        <div className="dyn-meter-face">{circuit === Circuit.Platinum ? <GrBar gr={mGr} /> : <GrNeedle gr={mGr} />}</div>
        <LevelBar db={mOut} label="Out" length={108} grow />
        <TransferGraph v={v} level={mLevel} size={graph} />
      </div>

      <div className="dyn-main">
        <div className="dyn-big">
          {threshold}
          {ratioControl}
        </div>
        <div className="dyn-small">
          {attack}
          {release}
          {digital ? <ParamKnob props={props} id={P.knee} /> : <ParamKnob props={props} id={P.knee} disabled note={varimu ? "Wide" : opto ? "Soft" : "Fixed"} title="This circuit's knee is part of its character" />}
          <ParamKnob props={props} id={P.makeup} label={opto ? "Gain" : "Make Up"} />
          <ParamKnob props={props} id={P.mix} />
          <ParamKnob props={props} id={P.output} />
        </div>
      </div>

      <div className="dyn-sections">
        <Section title="Detection">
          {digital ? <ParamChoice props={props} id={P.detector} /> : <span className="dyn-note">Peak</span>}
          {digital && <ParamToggle props={props} id={P.autoRelease} label="Auto Release" />}
          {varimu && (
            <div className="dyn-field">
              <span className="dyn-field-label">Time Constant</span>
              <ParamChoice props={props} id={P.timeConstant} labels={["1", "2", "3", "4", "5", "6"]} />
            </div>
          )}
          <ParamKnob props={props} id={P.link} label="Link" size={30} />
          <ParamKnob props={props} id={P.lookahead} size={30} />
        </Section>
        <Section title="Side Chain">
          <ParamChoice props={props} id={P.scMode} />
          <ParamKnob props={props} id={P.scFreq} label="Freq" size={30} disabled={Math.round(v(P.scMode)) === 0} note="Off" />
          <ParamToggle props={props} id={P.listen} label="Listen" title="Hear what the detector hears" />
        </Section>
        <Section title="Output">
          <div className="dyn-field">
            <span className="dyn-field-label">Auto Gain</span>
            <ParamChoice props={props} id={P.autoGain} labels={["Off", "0", "−12", "−24"]} />
          </div>
          <div className="dyn-field">
            <span className="dyn-field-label">Distortion</span>
            <ParamChoice props={props} id={P.distortion} />
          </div>
        </Section>
      </div>
      <div className="dyn-footer">
        <span className="dyn-circuit-name">{meta.name}</span>
        <span className="dyn-circuit-about">{meta.about}</span>
        <span className="dyn-footer-gr" title="Gain reduction">
          {mGr > 0.05 ? `−${mGr.toFixed(1)} dB` : "0.0 dB"}
        </span>
      </div>
    </div>
  );
}

export default CompressorPanel;
