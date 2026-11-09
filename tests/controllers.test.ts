// Unit tests for src/controllers.ts, the controller assignment types and labels (MIDI Learn).
import { test } from "node:test";
import assert from "node:assert/strict";
import { autoParamTarget, channelLabel, controllerLabel, feedbackChoice, feedbackOptions, feedbackSetting, feedbackTitle, fromPercent, stripTarget, toPercent, type Controller } from "../src/controllers.ts";

test("controllers are labelled like Logic's assignment list", () => {
  const cc: Controller = { port: "nanoKONTROL2", channel: 0, kind: { type: "cc", number: 7 } };
  assert.equal(controllerLabel(cc), "CC 7");
  assert.equal(channelLabel(cc), "Ch 1");
  const bend: Controller = { port: "Keys", channel: 15, kind: { type: "pitch_bend" } };
  assert.equal(controllerLabel(bend), "Pitch Bend");
  assert.equal(channelLabel(bend), "Ch 16");
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
