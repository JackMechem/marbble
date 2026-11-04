// Unit tests for src/rulerMenu.ts, the ruler's right-click menu.
import { test } from "node:test";
import assert from "node:assert/strict";
import { rulerMenu, type RulerCommands, type RulerMenuState } from "../src/rulerMenu.ts";

const state = (patch: Partial<RulerMenuState> = {}): RulerMenuState => ({ cycle: false, autopunch: false, regions: false, snap: "smart", hints: { cycle: "C", setLocators: "U" }, ...patch });

function commands(log: string[]): RulerCommands {
  return {
    locate: () => log.push("locate"),
    createMarker: () => log.push("marker"),
    toggleCycle: () => log.push("cycle"),
    toggleAutopunch: () => log.push("punch"),
    setLocatorsByRegions: () => log.push("locators"),
    setSnap: (s) => log.push(`snap ${s}`),
  };
}

const find = (items: ReturnType<typeof rulerMenu>, label: string) => {
  const it = items.find((i) => i.label === label);
  assert.ok(it, `${label} is in the menu`);
  return it;
};

test("the ruler menu offers the playhead, a marker, Cycle, Autopunch, the locators and Snap", () => {
  const items = rulerMenu(state(), commands([]));
  assert.deepEqual(
    items.map((i) => (i.separator ? "-" : i.label)),
    ["Move Playhead Here", "Create Marker Here", "-", "Cycle", "Autopunch", "Set Locators by Regions", "-", "Snap"],
  );
  assert.equal(find(items, "Cycle").hint, "C");
  assert.equal(find(items, "Set Locators by Regions").hint, "U");
});

test("toggles show their state, and Set Locators by Regions needs a selection", () => {
  const off = rulerMenu(state(), commands([]));
  assert.equal(find(off, "Cycle").checked, false);
  assert.equal(find(off, "Autopunch").checked, false);
  assert.equal(find(off, "Set Locators by Regions").disabled, true);
  const on = rulerMenu(state({ cycle: true, autopunch: true, regions: true, snap: "bar" }), commands([]));
  assert.equal(find(on, "Cycle").checked, true);
  assert.equal(find(on, "Autopunch").checked, true);
  assert.equal(find(on, "Set Locators by Regions").disabled, false);
  assert.deepEqual(
    find(on, "Snap").children!.map((c) => [c.label, !!c.checked]),
    [
      ["Smart", false],
      ["Bar", true],
      ["Beat", false],
      ["Division", false],
      ["Ticks", false],
      ["Off", false],
    ],
  );
});

test("each item runs its command", () => {
  const log: string[] = [];
  const items = rulerMenu(state(), commands(log));
  for (const label of ["Move Playhead Here", "Create Marker Here", "Cycle", "Autopunch", "Set Locators by Regions"]) find(items, label).onClick!();
  find(items, "Snap").children!.find((c) => c.label === "Off")!.onClick!();
  assert.deepEqual(log, ["locate", "marker", "cycle", "punch", "locators", "snap off"]);
});
