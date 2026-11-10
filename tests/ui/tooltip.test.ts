// Unit tests for src/ui/tooltip.ts: when a tooltip shows and where it goes.
import { test } from "node:test";
import assert from "node:assert/strict";
import { needsLabelWhileHovered, placeTip, TIP_IDLE, TIP_WARM, tipStep, type TipEvent, type TipState } from "../../src/ui/tooltip.ts";

const view = { width: 1280, height: 800 };
const size = { width: 120, height: 22 };
const button = { left: 600, top: 10, width: 30, height: 24 };

test("a tooltip goes centred below its control", () => {
  const at = placeTip(button, size, view, { x: 610, y: 20 });
  assert.equal(at.top, 10 + 24 + 6);
  assert.equal(at.left, 615 - 60);
});

test("near the bottom edge it goes above", () => {
  const low = { left: 600, top: 770, width: 30, height: 24 };
  const at = placeTip(low, size, view, { x: 610, y: 780 });
  assert.equal(at.top, 770 - 6 - 22);
});

test("it stays inside the window at the sides", () => {
  assert.equal(placeTip({ left: 0, top: 10, width: 24, height: 24 }, size, view, null).left, 4);
  assert.equal(placeTip({ left: 1260, top: 10, width: 20, height: 24 }, size, view, null).left, 1280 - 4 - 120);
  // Wider than the window: at the left margin.
  assert.equal(placeTip(button, { width: 2000, height: 22 }, view, null).left, 4);
});

test("on a large element it goes by the pointer, below its arrow or above it", () => {
  const canvas = { left: 200, top: 100, width: 900, height: 500 };
  const at = placeTip(canvas, size, view, { x: 500, y: 300 });
  assert.equal(at.top, 300 + 20, "below the arrow, not below the canvas");
  assert.equal(at.left, 500 - 60);
  const low = placeTip({ left: 0, top: 0, width: 1280, height: 800 }, size, view, { x: 500, y: 790 });
  assert.equal(low.top, 790 - 6 - 22);
  // From the keyboard there's no pointer: under the element.
  assert.equal(placeTip({ ...canvas, height: 300 }, size, view, null).top, 406);
});

test("it never covers the pointer", () => {
  const short = { width: 1280, height: 60 };
  const tall = { width: 200, height: 50 };
  const p = { x: 600, y: 30 };
  const at = placeTip({ left: 560, top: 20, width: 100, height: 24 }, tall, short, p);
  const coversArrow = at.left < p.x + 14 && at.left + tall.width > p.x && at.top < p.y + 20 && at.top + tall.height > p.y;
  assert.equal(coversArrow, false);
  assert.ok(at.top >= 4 && at.left >= 4 && at.left + tall.width <= 1280 - 4);
});

// ---- the state machine ----

function run<K>(events: [TipEvent<K>, number][], from: TipState<K> = TIP_IDLE) {
  let s = from;
  const actions: string[] = [];
  for (const [e, now] of events) {
    const r = tipStep(s, e, now);
    s = r.state;
    actions.push(r.action);
  }
  return { s, actions };
}

const enter = (anchor: string, held = false): TipEvent<string> => ({ kind: "enter", anchor, held });

test("a tooltip waits for the pointer to rest, then shows", () => {
  const { s, actions } = run([
    [enter("a"), 0],
    [{ kind: "move" }, 100],
    [{ kind: "elapsed" }, 700],
  ]);
  assert.deepEqual(actions, ["wait", "wait", "show"]);
  assert.equal(s.phase, "shown");
  // Leaving before the delay: nothing shows, and the next one waits again.
  const r = run([
    [enter("a"), 0],
    [{ kind: "leave" }, 200],
    [enter("b"), 250],
  ]);
  assert.deepEqual(r.actions, ["wait", "hide", "wait"]);
});

test("moving to a neighbour shows its tooltip at once, and so does one just after a close", () => {
  const shown = run([
    [enter("a"), 0],
    [{ kind: "elapsed" }, 600],
  ]).s;
  assert.deepEqual(run([[enter("b"), 1000]], shown).actions, ["show"]);
  const r = run(
    [
      [{ kind: "leave" }, 1000],
      [enter("b"), 1000 + TIP_WARM - 50],
    ],
    shown,
  );
  assert.deepEqual(r.actions, ["hide", "show"]);
  const late = run(
    [
      [{ kind: "leave" }, 1000],
      [enter("b"), 1000 + TIP_WARM + 50],
    ],
    shown,
  );
  assert.deepEqual(late.actions, ["hide", "wait"]);
});

test("a press, wheel or key hides it until the pointer leaves the element", () => {
  const shown = run([
    [enter("a"), 0],
    [{ kind: "elapsed" }, 600],
  ]).s;
  const r = run(
    [
      [{ kind: "dismiss" }, 700],
      [enter("a"), 800],
      [{ kind: "move" }, 900],
      [{ kind: "elapsed" }, 1400],
    ],
    shown,
  );
  assert.deepEqual(r.actions, ["hide", "none", "none", "none"]);
  // A dismissal leaves no instant next one either.
  const next = run(
    [
      [{ kind: "leave" }, 1500],
      [enter("b"), 1510],
    ],
    r.s,
  );
  assert.deepEqual(next.actions, ["none", "wait"]);
});

test("no tooltip while a mouse button is down", () => {
  const r = run([
    [enter("a", true), 0],
    [{ kind: "elapsed" }, 600],
    [enter("b", true), 700],
  ]);
  assert.deepEqual(r.actions, ["none", "none", "none"]);
  assert.equal(r.s.phase, "idle");
  // A drag that ends on an element: nothing until the pointer has left it.
  assert.equal(tipStep(r.s, enter("b"), 800).action, "none");
});

test("leaving the window forgets it all", () => {
  const shown = run([
    [enter("a"), 0],
    [{ kind: "elapsed" }, 600],
  ]).s;
  const r = run(
    [
      [{ kind: "reset" }, 700],
      [enter("b"), 710],
    ],
    shown,
  );
  assert.deepEqual(r.actions, ["hide", "wait"]);
});

test("an element named only by its title keeps the name while it's emptied", () => {
  const el = (tagName: string, attrs: string[], text: string) => ({ tagName, hasAttribute: (n: string) => attrs.includes(n), textContent: text });
  assert.equal(needsLabelWhileHovered(el("BUTTON", [], "")), true, "an icon button");
  assert.equal(needsLabelWhileHovered(el("BUTTON", ["aria-label"], "")), false);
  assert.equal(needsLabelWhileHovered(el("DIV", ["aria-labelledby"], "")), false);
  assert.equal(needsLabelWhileHovered(el("SPAN", [], " MIDI: none ")), false, "the title only described it");
  assert.equal(needsLabelWhileHovered(el("INPUT", [], "")), false, "a field's <label> names it");
  assert.equal(needsLabelWhileHovered(el("svg", [], "")), true);
});
