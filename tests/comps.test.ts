// Unit tests for src/comps.ts: take folder comp names and the folder's name bar label.
import { test } from "node:test";
import assert from "node:assert/strict";
import { compNames, currentComp, edgeNear, folderLabel, isMidiFolder, movedEdge, neighbourTake, sectionAt } from "../src/comps.ts";
import type { Region, TakeFolder } from "../src/api.ts";

const take = (id: number, name: string) => ({ id, name, clip: id, offset: 0, start: 0, end: 1000, gain_db: 0 });

function folder(f: Partial<TakeFolder>, type: "audio" | "midi" = "audio"): Region {
  const content =
    type === "audio" ? { type: "audio" as const, clip: 1, offset: 0, gain_db: 0, fade_in: 0, fade_out: 0 } : { type: "midi" as const, notes: [], cc: [] };
  return { id: 9, name: "Vocal", start: 0, length: 1000, muted: false, content, takes: { takes: [take(1, "Take 1"), take(2, "Take 2")], comp: [], ...f } } as Region;
}

test("a folder without saved comps has Comp 1", () => {
  const f = folder({}).takes!;
  assert.deepEqual(compNames(f), ["Comp 1"]);
  assert.equal(currentComp(f), 0);
});

test("the label names the whole take heard, or the comp", () => {
  assert.equal(folderLabel(folder({ comp: [{ start: 0, end: 1000, take: 2 }] })), "Take 2");
  const comps = [
    { name: "Comp 1", comp: [] },
    { name: "Lead", comp: [] },
  ];
  const r = folder({ comp: [{ start: 0, end: 500, take: 1 }, { start: 500, end: 1000, take: 2 }], comps, current: 1 });
  assert.equal(folderLabel(r), "Lead");
  assert.equal(currentComp({ ...r.takes!, current: 7 }), 1, "clamped");
  assert.equal(folderLabel({ ...r, takes: null }), null);
});

test("a MIDI folder names its take, the newest when none is chosen", () => {
  const r = folder({}, "midi");
  assert.ok(isMidiFolder(r) && !isMidiFolder(folder({})));
  assert.equal(folderLabel(r), "Take 2");
  assert.equal(folderLabel(folder({ comp: [{ start: 0, end: 1000, take: 1 }] }, "midi")), "Take 1");
});

// Three takes; take 3 has audio over 200..800 only. The comp: take 1, take 2, a gap, take 1.
const sections = (): TakeFolder => ({
  takes: [take(1, "Take 1"), take(2, "Take 2"), { ...take(3, "Take 3"), start: 200, end: 800 }],
  comp: [
    { start: 0, end: 300, take: 1 },
    { start: 300, end: 500, take: 2 },
    { start: 700, end: 1000, take: 1 },
  ],
});

test("a press near a section's end in its take's row grabs the edge", () => {
  const f = sections();
  assert.equal(edgeNear(f, 2, 297, 5), 300);
  assert.equal(edgeNear(f, 2, 503, 5), 500);
  assert.equal(edgeNear(f, 1, 400, 5), null, "not one of take 1's edges");
  assert.equal(edgeNear(f, 3, 300, 5), null, "take 3 isn't heard");
  assert.deepEqual(sectionAt(f, 300), { start: 300, end: 500, take: 2 });
  assert.equal(sectionAt(f, 600), null, "the gap");
});

test("dragging a border moves both sections, an outer edge only its own", () => {
  const f = sections();
  assert.deepEqual(movedEdge(f, 300, 250).slice(0, 2), [
    { start: 0, end: 250, take: 1 },
    { start: 250, end: 500, take: 2 },
  ]);
  // A border can't swallow either side.
  assert.equal(movedEdge(f, 300, 900)[1].start, 499);
  assert.equal(movedEdge(f, 300, -50)[0].end, 1);
  // The outer end of take 2's section grows into the gap, up to the next section.
  assert.deepEqual(movedEdge(f, 500, 650)[1], { start: 300, end: 650, take: 2 });
  assert.equal(movedEdge(f, 500, 900)[1].end, 700);
  assert.equal(movedEdge(f, 700, 100)[2].start, 500, "the last section's start stops at the one before");
  assert.deepEqual(movedEdge(f, 600, 650), f.comp, "no edge there");
});

test("a section's take ends where that take's audio does", () => {
  const f: TakeFolder = { ...sections(), comp: [{ start: 300, end: 600, take: 3 }] };
  assert.equal(movedEdge(f, 600, 950)[0].end, 800);
  assert.equal(movedEdge(f, 300, 0)[0].start, 200);
});

test("moving a section between takes skips takes with no audio there", () => {
  const f = sections();
  const first = f.comp[0];
  assert.equal(neighbourTake(f, first, 1), 2);
  assert.equal(neighbourTake(f, first, -1), null, "already the top take");
  const late = { start: 850, end: 1000, take: 1 };
  assert.equal(neighbourTake(f, { ...late, take: 2 }, 1), null, "take 3 has nothing after 800");
  assert.equal(neighbourTake(f, { start: 300, end: 500, take: 1 }, 1), 2);
  assert.equal(neighbourTake(f, { start: 300, end: 500, take: 2 }, 1), 3);
});
