// Unit tests for src/app/paneSizes.ts: pane sizes within their limits and the room there is, and
// the stylesheet rules that keep the panes inside the window.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { bottomMax, bottomPaneSize, clampPane, LOCAL_BAR_H, MIN_TRACK_ROWS_H, PANE_DEFAULTS, PANE_LIMITS, parsePaneSizes, tracksAreaMin } from "../../src/app/paneSizes.ts";

test("a pane stays within its own limits", () => {
  assert.equal(clampPane("mixer", 50), PANE_LIMITS.mixer[0]);
  assert.equal(clampPane("mixer", 5000), PANE_LIMITS.mixer[1]);
  assert.equal(clampPane("mixer", 432.6), 433);
  assert.equal(clampPane("mixer", NaN), PANE_DEFAULTS.mixer);
});

test("the room there is wins over a pane's limits", () => {
  assert.equal(clampPane("mixer", 800, 500), 500);
  assert.equal(clampPane("mixer", 300, 500), 300, "a pane that fits keeps its size");
  // Less room than the pane's own minimum: the Tracks area still keeps its share.
  assert.equal(clampPane("mixer", 400, 120), 120);
  assert.equal(clampPane("editor", 400, -20), 0);
});

test("remembered sizes are read within their limits, and anything malformed is the default", () => {
  assert.deepEqual(parsePaneSizes(null), PANE_DEFAULTS);
  assert.deepEqual(parsePaneSizes("not json"), PANE_DEFAULTS);
  assert.deepEqual(parsePaneSizes("[1,2]"), PANE_DEFAULTS);
  assert.deepEqual(parsePaneSizes("7"), PANE_DEFAULTS);
  const s = parsePaneSizes(JSON.stringify({ mixer: 5000, editor: "tall", fx: null, library: 300, headers: 1 }));
  assert.equal(s.mixer, PANE_LIMITS.mixer[1]);
  assert.equal(s.editor, PANE_DEFAULTS.editor);
  assert.equal(s.fx, PANE_DEFAULTS.fx);
  assert.equal(s.library, 300);
  assert.equal(s.headers, PANE_LIMITS.headers[0]);
});

test("the Tracks area keeps its menu bar, its fixed rows and some track rows", () => {
  assert.equal(tracksAreaMin(52), LOCAL_BAR_H + 52 + MIN_TRACK_ROWS_H);
  assert.equal(tracksAreaMin(-10), LOCAL_BAR_H + MIN_TRACK_ROWS_H);
});

test("the bottom section takes what the Tracks area leaves", () => {
  const min = tracksAreaMin(52 + 41);
  assert.equal(bottomMax(756, min), 756 - min);
  assert.equal(bottomMax(100, min), 0, "never below nothing");
  // The case seen: a Mixer remembered 900 px tall in a 920 px window shows at the room there is,
  // and a drag of its divider starts from there, so it moves at once.
  const room = bottomMax(756, min);
  assert.equal(Math.min(900, room), room);
  assert.equal(clampPane("mixer", Math.min(900, room) + 40, room), room, "dragging up past the limit stays at it");
  assert.equal(clampPane("mixer", Math.min(900, room) - 40, room), room - 40, "dragging down shrinks it at once");
});

test("the Event List and the Step Sequencer share the Editors' height", () => {
  assert.equal(bottomPaneSize("mixer"), "mixer");
  assert.equal(bottomPaneSize("fx"), "fx");
  assert.equal(bottomPaneSize("editor"), "editor");
  assert.equal(bottomPaneSize("list"), "editor");
  assert.equal(bottomPaneSize("steps"), "editor");
  assert.equal(bottomPaneSize("none"), null);
});

// Guards on the stylesheet: the rules that keep the panes inside the window.
// Line endings are normalised: a Windows checkout has CRLF, and the selectors below span lines.
const css = (f: string) => readFileSync(new URL(`../../src/styles/${f}`, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const rule = (text: string, selector: string) => {
  const at = text.indexOf(`${selector} {`);
  assert.ok(at >= 0, `${selector} has a rule`);
  return text.slice(at, text.indexOf("}", at));
};

test("the window's layout is one column exactly as wide as the window", () => {
  // An implicit grid column is as wide as its widest content: a wide Mixer widened everything.
  assert.match(rule(css("layout.css"), ".app"), /grid-template-columns:\s*minmax\(0,\s*1fr\)/);
  assert.match(rule(css("layout.css"), ".center"), /min-width:\s*0/);
});

test("bottom panes shrink when the window has no room for their height", () => {
  for (const [file, selector] of [
    ["mixer-editor.css", ".mixer,\n.editor"],
    ["fx-dock.css", ".fx-dock"],
  ]) {
    const r = rule(css(file), selector);
    assert.match(r, /flex:\s*0 1 auto/, `${selector} shrinks`);
    assert.match(r, /min-height:\s*0/, `${selector} has no content minimum`);
  }
  assert.match(readFileSync(new URL("../../src/tracks/TracksArea.tsx", import.meta.url), "utf8"), /className="tracks-area" style=\{\{ minHeight \}\}/);
});
