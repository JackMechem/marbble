// Unit tests for src/controllers.ts, the controller assignment types and labels (MIDI Learn).
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  autoParamTarget,
  buttonHint,
  buttonOptions,
  channelLabel,
  controllerLabel,
  feedbackChoice,
  feedbackOptions,
  feedbackSetting,
  feedbackTitle,
  fromPercent,
  isButtonKind,
  offersLatching,
  rowEditor,
  stripTarget,
  toPercent,
  type Controller,
  type CtlKind,
} from "../src/controllers.ts";

test("controllers are labelled like Logic's assignment list", () => {
  const cc: Controller = { port: "nanoKONTROL2", channel: 0, kind: { type: "cc", number: 7 } };
  assert.equal(controllerLabel(cc), "CC 7");
  assert.equal(channelLabel(cc), "Ch 1");
  const bend: Controller = { port: "Keys", channel: 15, kind: { type: "pitch_bend" } };
  assert.equal(controllerLabel(bend), "Pitch Bend");
  assert.equal(channelLabel(bend), "Ch 16");
});

test("notes and program changes are labelled as Settings › MIDI shows them", () => {
  const pad: Controller = { port: "Matrix", channel: 9, kind: { type: "note", number: 36 } };
  assert.equal(controllerLabel(pad), "Note C1");
  assert.equal(channelLabel(pad), "Ch 10");
  assert.equal(controllerLabel({ ...pad, kind: { type: "note", number: 61 } }), "Note C♯3");
  assert.equal(controllerLabel({ ...pad, kind: { type: "program", number: 4 } }), "Program 5");
});

test("each row edits what its controller and target need", () => {
  const row = (kind: CtlKind, button: boolean) => ({ controller: { port: "P", channel: 0, kind }, button });
  assert.equal(rowEditor(row({ type: "cc", number: 7 }, false)), "takeover");
  assert.equal(rowEditor(row({ type: "pitch_bend" }, false)), "takeover");
  assert.equal(rowEditor(row({ type: "note", number: 36 }, false)), "button-range");
  assert.equal(rowEditor(row({ type: "program", number: 1 }, false)), "button-range");
  assert.equal(rowEditor(row({ type: "cc", number: 20 }, true)), "button");
  assert.equal(rowEditor(row({ type: "note", number: 36 }, true)), "button");
  assert.ok(isButtonKind({ type: "note", number: 1 }) && !isButtonKind({ type: "cc", number: 1 }));
  // A program change has no release to end a Momentary press.
  const modes = (kind: CtlKind) => buttonOptions(row(kind, true)).map(([m]) => m);
  assert.deepEqual(modes({ type: "note", number: 36 }), ["toggle", "momentary", "trigger"]);
  assert.deepEqual(modes({ type: "program", number: 2 }), ["toggle", "trigger"]);
  assert.match(buttonHint("momentary"), /held/);
  assert.equal(buttonHint("trigger", true), "Runs on press");
  assert.match(buttonHint("momentary", true), /release/);
  // A program change is never released, so it can't latch either.
  assert.ok(offersLatching(row({ type: "cc", number: 20 }, true)) && offersLatching(row({ type: "note", number: 36 }, true)));
  assert.ok(!offersLatching(row({ type: "program", number: 2 }, true)) && !offersLatching(row({ type: "note", number: 36 }, false)));
});

test("ranges show as whole percentages and come back clamped", () => {
  assert.equal(toPercent(0.254), 25);
  assert.equal(toPercent(1.2), 100);
  assert.equal(toPercent(-1), 0);
  assert.equal(fromPercent(75), 0.75);
  assert.equal(fromPercent(140), 1);
  assert.equal(fromPercent(-5), 0);
});

test("an automation lane's parameter becomes a target", () => {
  assert.deepEqual(autoParamTarget(3, { type: "volume" }), { type: "volume", track: 3 });
  assert.deepEqual(autoParamTarget(3, { type: "send", value: 2 }), { type: "send", track: 3, slot: 2 });
  assert.deepEqual(autoParamTarget(3, { type: "plugin", value: { uid: 9, param: 1234, name: "Synth: Cutoff" } }), {
    type: "plugin",
    track: 3,
    uid: 9,
    plugin: "",
    param: 1234,
  });
});

test("a right-click on a strip control finds its target", () => {
  const inside = (...sels: string[]) => (sel: string) => sels.includes(sel);
  assert.deepEqual(stripTarget(4, inside(".strip-fader")), { type: "volume", track: 4 });
  assert.deepEqual(stripTarget(4, inside(".strip-pan .knob")), { type: "pan", track: 4 });
  // The switches toggle on any button, so they aren't offered.
  assert.equal(stripTarget(4, inside(".tbtn.mute")), null);
  assert.equal(stripTarget(4, inside(".strip-name")), null);
  // Stereo Out: only its fader.
  assert.deepEqual(stripTarget(null, inside(".strip-fader")), { type: "master_volume" });
  assert.equal(stripTarget(null, inside(".strip-pan .knob")), null);
});

test("the Feedback checkbox says where values go, or what's missing", () => {
  const controller: Controller = { port: "nanoKONTROL2:nanoKONTROL2 MIDI 1", channel: 0, kind: { type: "cc", number: 7 } };
  assert.match(feedbackTitle({ controller, feedback: true, output: null }), /No MIDI output is paired with nanoKONTROL2:nanoKONTROL2 MIDI 1.*Settings › MIDI/);
  assert.match(feedbackTitle({ controller, feedback: true, output: "nanoKONTROL2:nanoKONTROL2 MIDI 1" }), /^Sends .* back to nanoKONTROL2/);
  assert.match(feedbackTitle({ controller, feedback: false, output: "X" }), /^Send .* back to X/);
});

test("an input's feedback pairing round-trips through its pop-up", () => {
  const chosen = { Keys: "", Pads: "Lights" };
  assert.equal(feedbackChoice(chosen, "Fader Box"), "auto");
  assert.equal(feedbackChoice(undefined, "Fader Box"), "auto");
  assert.equal(feedbackChoice(chosen, "Keys"), "none");
  assert.equal(feedbackChoice(chosen, "Pads"), "out:Lights");
  assert.equal(feedbackSetting("auto"), undefined);
  assert.equal(feedbackSetting("none"), "");
  assert.equal(feedbackSetting("out:Lights"), "Lights");
  // An output whose name starts like a choice still comes back.
  assert.equal(feedbackChoice({ In: feedbackSetting("out:out:odd") ?? "" }, "In"), "out:out:odd");
});

test("the pop-up names the automatic output and lists the others once", () => {
  const outputs = [{ key: "A" }, { key: "B" }, { key: "A" }];
  assert.deepEqual(feedbackOptions({ auto_output: "A" }, outputs, "auto"), [
    ["auto", "Automatic (A)"],
    ["none", "No feedback"],
    ["out:A", "A"],
    ["out:B", "B"],
  ]);
  assert.equal(feedbackOptions({ auto_output: null }, [], "auto")[0][1], "Automatic (none found)");
  // A chosen output that is unplugged stays listed.
  assert.deepEqual(feedbackOptions({ auto_output: null }, outputs, "out:Gone").at(-1), ["out:Gone", "Gone"]);
});
