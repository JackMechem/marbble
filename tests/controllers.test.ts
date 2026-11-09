// Unit tests for src/controllers.ts, the controller assignment types and labels (MIDI Learn).
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  autoParamTarget,
  buttonHint,
  buttonOptions,
  canBeRelative,
  ccMode,
  ccModePatch,
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
  selectedTargets,
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

test("14-bit controllers and NRPNs are labelled by their numbers", () => {
  const c = (kind: CtlKind): Controller => ({ port: "P", channel: 0, kind });
  assert.equal(controllerLabel(c({ type: "cc14", number: 1 })), "CC 1/33");
  assert.equal(controllerLabel(c({ type: "nrpn", number: 389 })), "NRPN 389");
  assert.equal(rowEditor({ controller: c({ type: "nrpn", number: 389 }), button: false }), "takeover");
});

test("a control change's Mode pop-up holds the takeover mode and the relative encodings", () => {
  assert.equal(ccMode({ mode: "pickup", encoding: "absolute" }), "pickup");
  assert.equal(ccMode({ mode: "pickup", encoding: "binary_offset" }), "binary_offset");
  assert.deepEqual(ccModePatch("jump"), { mode: "jump", encoding: "absolute" });
  assert.deepEqual(ccModePatch("twos_complement"), { encoding: "twos_complement" });
  // Only 7-bit control changes send encoder steps.
  assert.ok(canBeRelative({ type: "cc", number: 16 }));
  assert.ok(!canBeRelative({ type: "cc14", number: 1 }) && !canBeRelative({ type: "pitch_bend" }) && !canBeRelative({ type: "nrpn", number: 1 }));
});

test("Learn Selected Track offers the track's controls, eight sends and inserts, and plug-in parameters", () => {
  const menu = selectedTargets(4);
  assert.deepEqual(menu[0].target, { type: "selected", param: { type: "volume" } });
  assert.deepEqual(
    menu.find((m) => m.label === "Send")?.children?.map((c) => c.target),
    [0, 1, 2, 3, 4, 5, 6, 7].map((slot) => ({ type: "selected", param: { type: "send", slot } })),
  );
  assert.equal(menu.find((m) => m.label === "Insert Bypass")?.children?.[7].label, "Insert 8 Bypass");
  assert.deepEqual(menu.find((m) => m.label === "Focused Plug-in")?.children?.[3].target, { type: "selected", param: { type: "plugin_param", index: 3 } });
  assert.ok(menu.some((m) => m.label === "Input Monitoring"));
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
