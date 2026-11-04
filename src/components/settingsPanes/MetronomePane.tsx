// Settings › Metronome, and the fields Project Settings shows too.
import { act, type ClickSettings, type ClickSound } from "../../api";
import { keyHint } from "../../keymap";
import { DEFAULT_CLICK, useSettings } from "../../prefs";
import { useStore } from "../../store";
import { Check, NumberField, Select, Slider } from "../Form";
import { Foot, Row, Section, setPrefs, toast } from "./parts";

const SOUNDS: { id: ClickSound; name: string; description: string }[] = [
  { id: "classic", name: "Classic", description: "A short sine tick" },
  { id: "click", name: "Click", description: "A sharp transient that cuts through a dense mix" },
  { id: "woodblock", name: "Woodblock", description: "A warm, woody knock" },
  { id: "beep", name: "Beep", description: "A longer electronic tone" },
];

/** The metronome form, shared by Settings and Project Settings. */
export function MetronomeFields() {
  const click = useStore((s) => s.snap?.project.click) ?? DEFAULT_CLICK;
  const on = useStore((s) => s.snap?.project.metronome ?? false);
  const set = (patch: Partial<ClickSettings>) => act("set_click", { click: { ...click, ...patch } });
  const st = (n: number) => (n > 0 ? `+${n}` : String(n));
  return (
    <>
      <Section title="Sound">
        <Row label="Click:">
          <Check checked={on} onChange={(v) => act("set_metronome", { on: v })}>
            Metronome on ({keyHint("metronome") || "K"})
          </Check>
        </Row>
        <Row label="Sound:" note={SOUNDS.find((s) => s.id === click.sound)?.description}>
          <div className="segmented">
            {SOUNDS.map((s) => (
              <button key={s.id} className={s.id === click.sound ? "on" : ""} onClick={() => set({ sound: s.id })}>
                {s.name}
              </button>
            ))}
          </div>
        </Row>
        <Row label="Volume:">
          <Slider value={click.volume_db} min={-40} max={6} step={0.5} onChange={(v) => set({ volume_db: v })} format={(v) => `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`} />
        </Row>
        <Row label="Accent:">
          <Check checked={click.accent} onChange={(v) => set({ accent: v })}>
            Higher and louder on the first beat of each bar
          </Check>
        </Row>
        <Row label="Pitch:" note="In semitones, relative to the sound's own pitch.">
          <div className="row-inline">
            <span className="inline-label">Bar</span>
            <NumberField value={click.bar_pitch} min={-12} max={12} onChange={(v) => set({ bar_pitch: Math.round(v) })} />
            <span className="inline-label">Beat</span>
            <NumberField value={click.beat_pitch} min={-12} max={12} onChange={(v) => set({ beat_pitch: Math.round(v) })} />
            <span className="dim">{click.bar_pitch || click.beat_pitch ? `${st(click.bar_pitch)} / ${st(click.beat_pitch)}` : ""}</span>
          </div>
        </Row>
        <Row label="Subdivision:">
          <Select
            value={click.division}
            options={[
              [1, "Off (beats only)"],
              [2, "Eighth notes"],
              [3, "Triplets"],
              [4, "Sixteenth notes"],
            ]}
            onChange={(v) => set({ division: v })}
          />
        </Row>
      </Section>
      <Section title="When">
        <Row label="Playing:">
          <Check checked={click.while_playing} onChange={(v) => set({ while_playing: v })}>
            Click while playing
          </Check>
        </Row>
        <Row label="Recording:" note="The count-in always clicks, even with the metronome off.">
          <div className="pref-checks">
            <Check checked={click.while_recording} onChange={(v) => set({ while_recording: v })}>
              Click while recording
            </Check>
            <Check checked={click.only_count_in} onChange={(v) => set({ only_count_in: v })}>
              Only during the count-in
            </Check>
          </div>
        </Row>
      </Section>
    </>
  );
}

export function MetronomePane({ close }: { close: () => void }) {
  const click = useStore((s) => s.snap?.project.click) ?? DEFAULT_CLICK;
  const deflt = useSettings((s) => s.prefs.click);
  const isDefault = JSON.stringify(click) === JSON.stringify(deflt);
  return (
    <>
      <MetronomeFields />
      <Foot close={close} note="Saved with the project.">
        <button className="push" title="The settings new projects start with" onClick={() => act("set_click", { click: deflt })} disabled={isDefault}>
          Revert to Default
        </button>
        <button className="push" title="Marbble's original metronome settings" onClick={() => act("set_click", { click: DEFAULT_CLICK })} disabled={JSON.stringify(click) === JSON.stringify(DEFAULT_CLICK)}>
          Factory Settings
        </button>
        <button
          className="push"
          disabled={isDefault}
          onClick={() => {
            setPrefs("click", click);
            toast("New projects will use these metronome settings");
          }}
        >
          Use for New Projects
        </button>
      </Foot>
    </>
  );
}
