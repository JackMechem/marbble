// The LCD's Tuner mode: the note the selected audio track's input plays, how far off it is on a
// needle across ±50 cents, and its frequency. The engine listens only while this shows.
import { useEffect, useState } from "react";
import { call } from "../../engine/commands";
import type { TunerReading } from "../../engine/snapshot";
import { onTick, useStore } from "../../state/store";
import { IN_TUNE, centsText, hzText, needlePercent } from "./lcdModes";
import { noteName, spellingFor } from "./noteNames";

const same = (a: TunerReading | null, b: TunerReading | null) =>
  a === b || (!!a && !!b && a.track === b.track && a.note === b.note && a.cents.toFixed(1) === b.cents.toFixed(1) && a.freq.toFixed(1) === b.freq.toFixed(1));

export function LcdTuner() {
  const [r, setR] = useState<TunerReading | null>(null);
  useEffect(() => {
    call("set_input_tuner", { on: true });
    const off = onTick((t) => setR((prev) => (same(prev, t.tuner ?? null) ? prev : (t.tuner ?? null))));
    return () => {
      off();
      call("set_input_tuner", { on: false });
    };
  }, []);
  const spelling = spellingFor(useStore((s) => s.snap?.project.key) ?? "");
  const trackName = useStore((s) => (r?.track != null ? s.snap?.project.tracks.find((t) => t.id === r.track)?.name : undefined));

  const heard = !!r && r.track != null && r.note >= 0 && r.freq > 0;
  const inTune = heard && Math.abs(r.cents) <= IN_TUNE;
  const name = heard ? noteName(r.note, spelling) : null;
  // The octave goes small beside the note's name, as on the Tuner plug-in.
  const [, pitch, octave] = name ? /^(.+?)(-?\d+)$/.exec(name)! : [];
  const noInput = !!r && r.track == null;

  return (
    <div className="lcd-body lcd-tuner" title={trackName ? `Tuning the input of “${trackName}”` : "Tunes the selected audio track's input"}>
      <div className={"lcd-field lcd-note" + (heard ? "" : " idle") + (inTune ? " in-tune" : "")}>
        <div className="lcd-digits">
          {pitch ?? "—"}
          {octave !== undefined && <span className="lcd-octave">{octave.replace("-", "−")}</span>}
        </div>
        <div className="lcd-labels">
          <span>NOTE</span>
        </div>
      </div>
      <div className="lcd-sep" />
      <div className="lcd-field lcd-cents">
        <div className={"lcd-scale" + (heard ? "" : " idle")} aria-hidden>
          <div className="lcd-scale-zone" />
          {[0, 25, 50, 75, 100].map((p) => (
            <div key={p} className={"lcd-scale-tick" + (p === 50 ? " centre" : "")} style={{ left: `${p}%` }} />
          ))}
          <div className={"lcd-needle" + (inTune ? " in-tune" : "")} style={{ left: `${needlePercent(heard ? r.cents : 0)}%` }} />
        </div>
        <div className="lcd-labels lcd-tuner-readout">
          {noInput ? (
            <span>SELECT AN AUDIO TRACK</span>
          ) : (
            <>
              <span>
                <b>{heard ? centsText(r.cents) : "—"}</b> CENTS
              </span>
              <span>
                <b>{heard ? hzText(r.freq) : "—"}</b> HZ
              </span>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
