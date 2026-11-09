// Marbble's own tooltips, in place of the webview's. Controls still say what they are with a
// `title` attribute, as before; one set of listeners for the whole window (installTooltips, from
// main.tsx, so a detached panel's window has them too) finds the `title` under the pointer,
// empties it while the pointer is there so the webview shows no tooltip of its own, and shows
// the text in a themed box (tooltip.css) once the pointer rests. The pure parts, where the box
// goes and when it shows, are unit tested (tests/tooltip.test.ts).
//
// Why an empty title rather than none: the webview takes the nearest element with a `title`
// attribute, so an empty one also keeps an outer element's title from showing; and React, which
// only sets the attribute when its value changes and removes it when it goes, still reaches a
// title that's there: a MutationObserver on the element sees the new text (a level that changed,
// the CPU meter's figures) or its removal.

import { keyboardFocus } from "./focusRing.ts";

/** How long the pointer rests on a control before its tooltip shows, in ms. */
export const TIP_DELAY = 600;
/** For this long after a tooltip closes, the next one shows at once, so moving along a row of
 *  buttons reads each without waiting again. */
export const TIP_WARM = 300;

/** Space between the box and what it describes, and between the box and the window's edges. */
const GAP = 6;
const MARGIN = 4;
/** The pointer's arrow, below and to the right of where it points, which the box keeps clear of. */
const CURSOR_W = 14;
const CURSOR_H = 20;
/** Past this size (a canvas, a fader, a list row across the window) the box goes by the pointer
 *  rather than under the element, which could be far from where the pointer is. */
const LARGE_W = 400;
const LARGE_H = 48;

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Point {
  x: number;
  y: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/**
 * Where a tooltip of `size` goes for an element at `anchor` in a window of `view`: centred below
 * it, or above it when there's no room below, and moved along to stay inside the window. For a
 * large element it goes below the pointer's arrow instead, or above the pointer. It never covers the pointer. `pointer` is null when the keyboard brought the
 * tooltip up (focus).
 */
export function placeTip(anchor: Box, size: { width: number; height: number }, view: { width: number; height: number }, pointer: Point | null): { left: number; top: number } {
  const p = anchor.width > LARGE_W || anchor.height > LARGE_H ? pointer : null;
  const cx = p ? p.x : anchor.left + anchor.width / 2;
  const below = p ? p.y + CURSOR_H : anchor.top + anchor.height + GAP;
  const above = (p ? p.y : anchor.top) - GAP - size.height;
  const lowest = view.height - MARGIN - size.height;
  let top: number;
  if (below <= lowest) top = below;
  else if (above >= MARGIN) top = above;
  // Room on neither side (a tall box in a short window): the side with more, as far as it goes.
  else top = view.height - below > above + size.height ? below : above;
  top = clamp(top, MARGIN, Math.max(MARGIN, lowest));
  let left = clamp(cx - size.width / 2, MARGIN, Math.max(MARGIN, view.width - MARGIN - size.width));
  // Squeezed over the pointer: beside it instead, on the side with room.
  if (pointer && covers({ left, top, width: size.width, height: size.height }, pointer)) {
    const right = pointer.x + CURSOR_W;
    left = right + size.width <= view.width - MARGIN ? right : Math.max(MARGIN, pointer.x - GAP - size.width);
  }
  return { left, top };
}

/** Whether a box at `b` lies over the pointer's arrow at `p`. */
function covers(b: Box, p: Point): boolean {
  return b.left < p.x + CURSOR_W && b.left + b.width > p.x && b.top < p.y + CURSOR_H && b.top + b.height > p.y;
}

// ---- When it shows -----------------------------------------------------------------------------

/** What happens to the tooltips. `K` is what names an element (the element itself in the app). */
export type TipEvent<K> =
  /** The pointer came onto an element with a tooltip, or Tab moved the focus to one. `held`:
   *  a mouse button is down (a drag), so it doesn't show. */
  | { kind: "enter"; anchor: K; held: boolean }
  /** The pointer moved, still over the same element. */
  | { kind: "move" }
  /** Off every element with a tooltip. */
  | { kind: "leave" }
  /** The delay ran out. */
  | { kind: "elapsed" }
  /** A press, wheel, scroll, drag or key: gone, and it stays gone until the pointer leaves. */
  | { kind: "dismiss" }
  /** The window lost the focus or the pointer: gone, with no instant next one. */
  | { kind: "reset" };

export interface TipState<K> {
  anchor: K | null;
  phase: "idle" | "waiting" | "shown";
  /** The tooltip was dismissed on this element: none until the pointer leaves it. */
  dismissed: boolean;
  /** Until when (ms) the next tooltip shows without the delay. */
  warmUntil: number;
}

/** "show": show (or update) the tooltip for the anchor now; "wait": (re)start the delay;
 *  "hide": hide it and stop any delay. */
export type TipAction = "show" | "wait" | "hide" | "none";

export const TIP_IDLE: TipState<never> = { anchor: null, phase: "idle", dismissed: false, warmUntil: 0 };

/** The next state and what to do, for event `e` at time `now` (ms). */
export function tipStep<K>(s: TipState<K>, e: TipEvent<K>, now: number): { state: TipState<K>; action: TipAction } {
  const hideIf = (st: TipState<K>) => ({ state: st, action: (s.phase === "idle" ? "none" : "hide") as TipAction });
  switch (e.kind) {
    case "enter": {
      if (e.anchor === s.anchor) {
        if (e.held && !s.dismissed) return hideIf({ ...s, phase: "idle", dismissed: true });
        return { state: s, action: "none" };
      }
      if (e.held) return hideIf({ ...s, anchor: e.anchor, phase: "idle", dismissed: true });
      // Straight from one tooltip to the next, or just after one closed: no new delay.
      if (s.phase === "shown" || now < s.warmUntil) return { state: { ...s, anchor: e.anchor, phase: "shown", dismissed: false }, action: "show" };
      return { state: { ...s, anchor: e.anchor, phase: "waiting", dismissed: false }, action: "wait" };
    }
    case "move":
      // The pointer rests before the tooltip shows: moving starts the delay again.
      return { state: s, action: s.phase === "waiting" ? "wait" : "none" };
    case "leave":
      return hideIf({ anchor: null, phase: "idle", dismissed: false, warmUntil: s.phase === "shown" ? now + TIP_WARM : s.warmUntil });
    case "elapsed":
      return s.phase === "waiting" ? { state: { ...s, phase: "shown" }, action: "show" } : { state: s, action: "none" };
    case "dismiss":
      if (s.anchor === null) return { state: s, action: "none" };
      return hideIf({ ...s, phase: "idle", dismissed: true, warmUntil: 0 });
    case "reset":
      return hideIf(TIP_IDLE);
  }
}

/** Whether an element whose title is emptied while the pointer is on it needs that text as its
 *  `aria-label` meanwhile: when the title was its only name (an icon button). A form field is
 *  usually named by its <label>, which an aria-label would override. */
export function needsLabelWhileHovered(el: { tagName: string; hasAttribute(n: string): boolean; textContent: string | null }): boolean {
  if (["INPUT", "SELECT", "TEXTAREA"].includes(el.tagName.toUpperCase())) return false;
  return !el.hasAttribute("aria-label") && !el.hasAttribute("aria-labelledby") && !(el.textContent ?? "").trim();
}

// ---- The listeners -----------------------------------------------------------------------------

/** Starts Marbble's tooltips in this window; returns a function that stops them. */
export function installTooltips(win: Window = window): () => void {
  const doc = win.document;
  let state: TipState<Element> = TIP_IDLE;
  let timer: number | undefined;
  let watch: number | undefined;
  let box: HTMLDivElement | null = null;
  let pointer: Point | null = null;
  /** Whether the anchor came from the keyboard (focus), so the box goes by the element. */
  let byFocus = false;
  /** The element whose title is emptied while the pointer is over it, and its text. */
  let held: { el: Element; text: string; labelled: boolean } | null = null;

  const observer = new MutationObserver(() => retitled());

  /** The text an element's tooltip shows. */
  const textOf = (el: Element) => (held?.el === el ? held.text : (el.getAttribute("title") ?? ""));

  /** The element whose tooltip the webview would show for `target`: the nearest with a title,
   *  or none when that title is empty. */
  function tipTarget(target: EventTarget | null): Element | null {
    for (let n = target instanceof Element ? target : null; n; n = n.parentElement) {
      if (n === held?.el) return n;
      if (n.hasAttribute("title")) return n.getAttribute("title") ? n : null;
    }
    return null;
  }

  /** Empties `el`'s title while the pointer is on it, keeping the text (and the name). */
  function hold(el: Element) {
    if (held?.el === el) return;
    release();
    const text = el.getAttribute("title") ?? "";
    const labelled = needsLabelWhileHovered(el);
    if (labelled) el.setAttribute("aria-label", text);
    el.setAttribute("title", "");
    held = { el, text, labelled };
    observer.observe(el, { attributes: true, attributeFilter: ["title"] });
  }

  /** Gives the held element its title back. */
  function release() {
    if (!held) return;
    if (observer.takeRecords().length) retitled();
    if (!held) return;
    const { el, text, labelled } = held;
    held = null;
    observer.disconnect();
    if (el.getAttribute("title") === "") el.setAttribute("title", text);
    if (labelled && el.getAttribute("aria-label") === text) el.removeAttribute("aria-label");
  }

  /** React set the held element's title: new text to show, or none. */
  function retitled() {
    if (!held) return;
    const v = held.el.getAttribute("title");
    if (v === "") return; // ours
    const el = held.el;
    if (v === null) {
      if (held.labelled && el.getAttribute("aria-label") === held.text) el.removeAttribute("aria-label");
      held = null;
      observer.disconnect();
      if (state.anchor === el) send({ kind: "leave" });
      return;
    }
    if (held.labelled) el.setAttribute("aria-label", v);
    held.text = v;
    el.setAttribute("title", "");
    if (state.anchor === el && state.phase === "shown") show();
  }

  function send(e: TipEvent<Element>) {
    const next = tipStep(state, e, win.performance.now());
    state = next.state;
    if (next.action === "wait") {
      win.clearTimeout(timer);
      timer = win.setTimeout(() => send({ kind: "elapsed" }), TIP_DELAY);
    } else if (next.action === "show") {
      win.clearTimeout(timer);
      show();
    } else if (next.action === "hide") {
      win.clearTimeout(timer);
      hide();
    }
  }

  function show() {
    const el = state.anchor;
    const text = el ? textOf(el) : "";
    if (!el || !el.isConnected || !text) {
      send({ kind: "leave" });
      return;
    }
    if (!box) {
      box = doc.createElement("div");
      box.className = "tooltip";
      // The element keeps its name and description (its title, or the aria-label above).
      box.setAttribute("aria-hidden", "true");
      doc.body.appendChild(box);
    }
    box.textContent = text;
    // Measured at the corner, where it has the whole width to wrap in (at the right edge a fixed
    // box would wrap to what's left of the window).
    box.style.left = "0px";
    box.style.top = "0px";
    const r = el.getBoundingClientRect();
    const b = box.getBoundingClientRect();
    const at = placeTip(r, b, { width: win.innerWidth, height: win.innerHeight }, byFocus ? null : pointer);
    box.style.left = `${Math.round(at.left)}px`;
    box.style.top = `${Math.round(at.top)}px`;
    box.classList.add("shown");
    // An element taken away under the pointer (a menu closing) sends no event: look now and then.
    win.clearInterval(watch);
    watch = win.setInterval(() => {
      if (!state.anchor?.isConnected) reset();
    }, 250);
  }

  function hide() {
    win.clearInterval(watch);
    box?.classList.remove("shown");
  }

  const over = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    pointer = { x: e.clientX, y: e.clientY };
    const el = tipTarget(e.target);
    if (!el) {
      release();
      if (state.anchor && !byFocus) send({ kind: "leave" });
      return;
    }
    hold(el);
    byFocus = false;
    send({ kind: "enter", anchor: el, held: e.buttons !== 0 });
  };
  const out = (e: PointerEvent) => {
    // Out of the window (or into another webview): nothing is under the pointer here.
    if (e.relatedTarget === null) {
      release();
      send({ kind: "reset" });
    }
  };
  const move = (e: PointerEvent) => {
    pointer = { x: e.clientX, y: e.clientY };
    if (state.phase === "waiting" && !byFocus) send({ kind: "move" });
  };
  const dismiss = () => send({ kind: "dismiss" });
  // Only a scroll that moves the element: the Tracks area following the playhead doesn't count.
  const scroll = (e: Event) => {
    const t = e.target;
    if (state.anchor && (t === doc || (t instanceof Node && t.contains(state.anchor)))) dismiss();
  };
  const reset = () => {
    release();
    send({ kind: "reset" });
  };
  // Tab onto a control shows its tooltip too (its title stays: the webview shows none for focus).
  const focusIn = (e: FocusEvent) => {
    if (!keyboardFocus(doc.documentElement)) return;
    const el = tipTarget(e.target);
    if (!el) return;
    byFocus = true;
    send({ kind: "enter", anchor: el, held: false });
  };
  const focusOut = (e: FocusEvent) => {
    if (byFocus && state.anchor && e.target instanceof Node && state.anchor.contains(e.target)) send({ kind: "leave" });
  };

  const capture = { capture: true, passive: true };
  const listeners: [EventTarget, string, EventListener][] = [
    [doc, "pointerover", over as EventListener],
    [doc, "pointerout", out as EventListener],
    [doc, "pointermove", move as EventListener],
    [doc, "pointerdown", dismiss],
    [doc, "wheel", dismiss],
    [doc, "dragstart", dismiss],
    [doc, "keydown", dismiss],
    [doc, "scroll", scroll],
    [doc, "focusin", focusIn as EventListener],
    [doc, "focusout", focusOut as EventListener],
    [win, "blur", reset],
  ];
  for (const [t, type, f] of listeners) t.addEventListener(type, f, capture);
  return () => {
    for (const [t, type, f] of listeners) t.removeEventListener(type, f, capture);
    release();
    win.clearTimeout(timer);
    hide();
    box?.remove();
  };
}
