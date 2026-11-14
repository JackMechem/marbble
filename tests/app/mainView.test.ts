// Unit tests for src/app/mainView.ts: what the bottom section shows in each view, the pane
// toggles and keys in the Mixer view, and the view remembered per project.
import { test } from "node:test";
import assert from "node:assert/strict";
import { bottomIn, keyRunsIn, paneShown, parseMainView, toggleBottom, toggledView } from "../../src/app/mainView.ts";

test("under the Tracks area any pane shows; under the Mixer only the docked effect panels", () => {
  for (const pane of ["none", "mixer", "editor", "list", "steps", "fx"] as const) assert.equal(bottomIn("tracks", pane), pane);
  assert.equal(bottomIn("mixer", "fx"), "fx");
  for (const pane of ["none", "mixer", "editor", "list", "steps"] as const) assert.equal(bottomIn("mixer", pane), "none", pane);
});

test("a pane's button is lit only while the pane shows", () => {
  assert.ok(paneShown({ mainView: "tracks", bottom: "editor" }, "editor"));
  assert.ok(!paneShown({ mainView: "mixer", bottom: "editor" }, "editor"), "kept for the Tracks view, not shown");
  assert.ok(paneShown({ mainView: "mixer", bottom: "fx" }, "fx"));
});

test("a pane's toggle shows or hides it under the Tracks area", () => {
  assert.deepEqual(toggleBottom({ mainView: "tracks", bottom: "none" }, "mixer"), { bottom: "mixer" });
  assert.deepEqual(toggleBottom({ mainView: "tracks", bottom: "mixer" }, "mixer"), { bottom: "none" });
  assert.deepEqual(toggleBottom({ mainView: "tracks", bottom: "mixer" }, "editor"), { bottom: "editor" });
});

test("in the Mixer view a pane's toggle brings the Tracks view back with the pane showing", () => {
  assert.deepEqual(toggleBottom({ mainView: "mixer", bottom: "none" }, "editor"), { mainView: "tracks", bottom: "editor" });
  // Already chosen but hidden: it shows rather than goes.
  assert.deepEqual(toggleBottom({ mainView: "mixer", bottom: "mixer" }, "mixer"), { mainView: "tracks", bottom: "mixer" });
  assert.deepEqual(toggleBottom({ mainView: "mixer", bottom: "list" }, "list"), { mainView: "tracks", bottom: "list" });
  // The docked effect panels show under the Mixer too.
  assert.deepEqual(toggleBottom({ mainView: "mixer", bottom: "fx" }, "fx"), { bottom: "none" });
});

test("the Mixer view's key goes there and back", () => {
  assert.equal(toggledView("tracks"), "mixer");
  assert.equal(toggledView("mixer"), "tracks");
});

test("the Mixer view holds back the region edits' keys, and only those", () => {
  assert.ok(!keyRunsIn("mixer", "Editing"));
  for (const group of ["Transport", "Navigation", "View", "Tracks", "Windows", "Control Surfaces", "Step Input", undefined]) assert.ok(keyRunsIn("mixer", group), String(group));
  assert.ok(keyRunsIn("tracks", "Editing"));
});

test("a project opens in the view it was left in, the Tracks view otherwise", () => {
  assert.equal(parseMainView("mixer"), "mixer");
  assert.equal(parseMainView("tracks"), "tracks");
  assert.equal(parseMainView(null), "tracks");
  assert.equal(parseMainView("\"mixer\""), "tracks");
});
