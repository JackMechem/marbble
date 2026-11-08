// Settings › Recording.
import { act, type BitDepth } from "../../api";
import { fmt, keyHint } from "../../keymap";
import { useSettings } from "../../prefs";
import { useStore } from "../../store";
import { Check, NumberField, Select } from "../Form";
import { Foot, Row, Section, setPrefs, FolderField } from "./parts";

export const BIT_DEPTHS: [BitDepth, string][] = [
  ["int24", "24-bit integer"],
  ["float32", "32-bit float"],
];

export function RecordingPane({ close }: { close: () => void }) {
  const r = useSettings((s) => s.prefs.recording);
  const countIn = useStore((s) => s.snap?.project.count_in ?? false);
  const override = useStore((s) => s.snap?.project.bit_depth ?? null);
  return (
    <>
      <Section title="Files">
        <Row
          label="Bit Depth:"
          note={
            <>
              32-bit float can't clip on disk however hot the input runs; 24-bit files are a quarter smaller and open everywhere.
              {override && <b> This project overrides it ({BIT_DEPTHS.find((b) => b[0] === override)?.[1]}) in Project Settings.</b>}
            </>
          }
        >
          <Select value={r.bit_depth} options={BIT_DEPTHS} onChange={(v) => setPrefs("recording", { bit_depth: v })} />
        </Row>
        <Row label="Recording Folder:" note="Recordings go into a folder named after the project here. Projects copy them in when you Save As.">
          <FolderField value={r.folder} placeholder="Project › Audio Files" title="Choose where recordings go" onChange={(v) => setPrefs("recording", { folder: v })} />
        </Row>
        <Row label="Take Names:">
          <Select
            value={r.take_names}
            options={[
              ["track_number", "Track name and take number (Vocals#03)"],
              ["track", "Track name (Vocals)"],
              ["project_track", "Project, track and take number (Song - Vocals#03)"],
            ]}
            onChange={(v) => setPrefs("recording", { take_names: v })}
          />
        </Row>
      </Section>
      <Section title="Starting to Record">
        <Row label="Count-in:" note={`Counts in with the click before recording when Count-in is on (${keyHint("count-in") || fmt("Shift+K")}).`}>
          <div className="row-inline">
            <Check checked={countIn} onChange={(on) => act("set_count_in", { on })}>
              On
            </Check>
            <Select
              value={r.count_in_bars}
              options={[1, 2, 3, 4].map((n) => [n, `${n} bar${n > 1 ? "s" : ""}`] as [number, string])}
              onChange={(v) => setPrefs("recording", { count_in_bars: v })}
              width={110}
            />
          </div>
        </Row>
        <Row label="Pre-roll:" note="With Count-in off, playback starts this far before the playhead so you hear the lead-in; recording still starts at the playhead.">
          <div className="row-inline">
            <NumberField value={r.pre_roll_secs} min={0} max={30} step={0.5} onChange={(v) => setPrefs("recording", { pre_roll_secs: v })} />
            <span className="dim">seconds</span>
          </div>
        </Row>
      </Section>
      <Section title="Overlapping Recordings">
        <Row
          label="Audio:"
          note={
            r.replace
              ? "Replace is on in the control bar: recordings replace what they overlap, audio and MIDI alike."
              : {
                  take_folders:
                    "Recording over regions packs them into a take folder with the new take, and recording over a take folder adds a take to it. The new take is heard; choose another from the folder's menu.",
                  on_top: "The new take plays; the older audio stays underneath and comes back if you move the take.",
                  new_track: "When the take overlaps a region, it goes on a new track below with the same settings and plug-ins.",
                  replace: "The parts of older regions under the take are removed. Their audio files are kept.",
                }[r.audio_overlap]
          }
        >
          <Select
            value={r.audio_overlap}
            options={[
              ["take_folders", "Create take folders"],
              ["on_top", "Put the new take on top"],
              ["new_track", "Create a new track"],
              ["replace", "Replace what's underneath"],
            ]}
            onChange={(v) => setPrefs("recording", { audio_overlap: v })}
          />
        </Row>
        <Row
          label="MIDI:"
          note={
            r.replace
              ? "Replace is on in the control bar: recordings replace what they overlap, audio and MIDI alike."
              : r.midi_overlap === "replace"
                ? "Notes and controllers from where recording starts (or the punch-in) to where it stops are replaced by what you play."
                : r.midi_overlap === "take_folders"
                  ? "Each cycle pass becomes a take, and recording over MIDI regions packs them into a take folder with the new take. Choose the take to hear from the folder's menu. The Replace button still replaces."
                  : undefined
          }
        >
          <Select
            value={r.midi_overlap}
            options={[
              ["new_region", "Create a new region"],
              ["merge", "Merge into the existing region"],
              ["replace", "Replace in the recorded range"],
              ["take_folders", "Create take folders"],
            ]}
            onChange={(v) => setPrefs("recording", { midi_overlap: v })}
          />
        </Row>
      </Section>
      <Section title="Takes">
        <Row label="Take Colors:" note="Each take in a take folder gets a colour of its own as it's recorded. Change one from the take folder menu.">
          <Check checked={!!r.color_takes} onChange={(on) => setPrefs("recording", { color_takes: on })}>
            Color takes automatically
          </Check>
        </Row>
      </Section>
      <Foot close={close} />
    </>
  );
}
