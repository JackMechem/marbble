// Settings › Editing: snap, nudge, the mouse wheel, what a click does and Move Automation with Regions.
import { keyHint } from "../../keymap";
import { useSettings, type ClickLocates, type MoveAutomation, type Nudge } from "../../prefs";
import { setState, type Snap } from "../../store";
import { NumberField, Select } from "../Form";
import type { WheelScrolls } from "../../wheel";
import { Foot, Row, Section, setPrefs, setUi } from "./parts";

const SNAPS: [Snap, string][] = [
  ["smart", "Smart (follows the zoom)"],
  ["bar", "Bar"],
  ["beat", "Beat"],
  ["division", "Division (a quarter of a beat)"],
  ["ticks", "Ticks"],
  ["off", "Off"],
];

const CLICK_LOCATES: [ClickLocates, string][] = [
  ["anywhere", "Anywhere in the tracks"],
  ["empty", "Empty space only"],
  ["off", "Never"],
];

const WHEEL_SCROLLS: [WheelScrolls, string][] = [
  ["vertical", "Vertically (tracks)"],
  ["horizontal", "Horizontally (time)"],
];

const MOVE_AUTOMATION: [MoveAutomation, string][] = [
  ["never", "Never"],
  ["always", "Always"],
  ["ask", "Ask"],
];

const NUDGES: [Nudge, string][] = [
  ["bar", "Bar"],
  ["beat", "Beat"],
  ["8", "1/8 note"],
  ["16", "1/16 note"],
  ["32", "1/32 note"],
  ["ms10", "10 ms"],
  ["ms1", "1 ms"],
  ["smp50", "50 samples"],
  ["smp10", "10 samples"],
  ["smp5", "5 samples"],
  ["smp2", "2 samples"],
  ["smp1", "1 sample"],
];

export function EditingPane({ close }: { close: () => void }) {
  const ui = useSettings((s) => s.ui);
  const fade = useSettings((s) => s.prefs.editing.fade_ms);
  const moveAutomation = useSettings((s) => s.prefs.editing.move_automation) ?? "never";
  const xfadeMs = useSettings((s) => s.prefs.editing.crossfade_ms) ?? 20;
  const xfadeCurve = useSettings((s) => s.prefs.editing.crossfade_curve) ?? 0.5;
  return (
    <>
      <Section title="Regions">
        <Row label="Snap:" note="The snap setting new sessions start with; change it any time from the Snap pop-up above the tracks.">
          <Select
            value={ui.snap}
            options={SNAPS}
            onChange={(v) => {
              setUi({ snap: v });
              setState({ snapMode: v });
            }}
          />
        </Row>
        <Row label="Nudge Value:" note={nudgeNote()}>
          <Select value={ui.nudge} options={NUDGES} onChange={(v) => setUi({ nudge: v })} />
        </Row>
        <Row label="Click Moves Playhead:" note="Clicking without dragging puts the playhead there, snapped to the grid. Dragging still moves regions or draws a selection.">
          <Select value={ui.click_locates ?? "empty"} options={CLICK_LOCATES} onChange={(v) => setUi({ click_locates: v })} />
        </Row>
        <Row label="Anti-click Fades:" note="A short fade-in and fade-out on new recordings and imported audio, so region edges don't click. 0 = none.">
          <div className="row-inline">
            <NumberField value={fade} min={0} max={100} step={0.5} resolution={0.1} onChange={(v) => setPrefs("editing", { fade_ms: v })} />
            <span className="dim">ms</span>
          </div>
        </Row>
        <Row
          label="Move Automation with Regions:"
          note="Whether moving a region along its track takes the track's automation under it along, replacing the automation where it lands. Ask asks on each move that would."
        >
          <Select value={moveAutomation} options={MOVE_AUTOMATION} onChange={(v) => setPrefs("editing", { move_automation: v })} />
        </Row>
        <Row
          label="Crossfade Time:"
          note="The crossfades where one take hands over to the next in a take folder, those the X-Fade drag mode puts between regions that just touch, and the Crossfade menu's first choice."
        >
          <div className="row-inline">
            <NumberField value={xfadeMs} min={0} max={1000} step={1} resolution={1} onChange={(v) => setPrefs("editing", { crossfade_ms: v })} />
            <span className="dim">ms</span>
          </div>
        </Row>
        <Row label="Crossfade Curve:" note="Their shape, as in the Region inspector: 0 is a straight line, 50 (the default) a quarter sine, which keeps two different takes from dipping where they cross.">
          <NumberField value={Math.round(xfadeCurve * 100)} min={-100} max={100} step={10} resolution={1} onChange={(v) => setPrefs("editing", { crossfade_curve: Math.max(-1, Math.min(1, v / 100)) })} />
        </Row>
      </Section>
      <Section title="Scrolling">
        <Row
          label="Mouse Wheel Scrolls:"
          note="In the Tracks area and the Piano Roll. Horizontally suits a mouse with one wheel: it moves through time, and Shift+wheel scrolls the tracks instead. A trackpad's sideways swipe always scrolls in time."
        >
          <Select value={ui.wheel_scrolls ?? "vertical"} options={WHEEL_SCROLLS} onChange={(v) => setUi({ wheel_scrolls: v })} />
        </Row>
      </Section>
      <Foot close={close} />
    </>
  );
}

/** "How far ⌥← and ⌥→ move…", naming the commands instead when they have no keys. */
function nudgeNote(): string {
  const keys = [keyHint("nudge-left"), keyHint("nudge-right")].filter(Boolean);
  const what = keys.length ? keys.join(" and ") : "Nudge Regions Left / Right (no keys assigned)";
  const bySample = [keyHint("nudge-left-sample"), keyHint("nudge-right-sample")].filter(Boolean);
  const samples = bySample.length ? ` ${bySample.join(" and ")} always ${bySample.length === 1 ? "moves" : "move"} them by one sample.` : "";
  return `How far ${what} ${keys.length === 1 ? "moves" : "move"} the selected regions.${samples}`;
}
