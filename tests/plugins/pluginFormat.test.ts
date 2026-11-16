// Unit tests for src/plugins/pluginFormat.ts: how the lists show a plug-in's format.
import { test } from "node:test";
import assert from "node:assert/strict";
import { formatName, formatsHeading, formatTag } from "../../src/plugins/pluginFormat.ts";

test("only CLAP plug-ins are tagged; one listed without a format is a VST3", () => {
  assert.equal(formatName({ format: "clap" }), "CLAP");
  assert.equal(formatName({ format: "vst3" }), "VST3");
  assert.equal(formatName({} as { format: "vst3" }), "VST3");
  assert.equal(formatTag({ format: "clap" }), "CLAP");
  assert.equal(formatTag({ format: "vst3" }), undefined);
});

test("the heading names the formats there, VST3 first", () => {
  assert.equal(formatsHeading([{ format: "clap" }, { format: "vst3" }]), "VST3 & CLAP");
  assert.equal(formatsHeading([{ format: "vst3" }]), "VST3");
  assert.equal(formatsHeading([{ format: "clap" }]), "CLAP");
  assert.equal(formatsHeading([]), "VST3");
});
