// Draggable dividers between panes. Sizes live in the store and are remembered per machine;
// their limits and arithmetic are in paneSizes.ts.
import { clampPane, PANE_DEFAULTS, parsePaneSizes, type PaneKey } from "./paneSizes";
import { getState, setState, subscribeState, useStore } from "../state/store";

const KEY = "marbble.paneSizes";

export function loadPaneSizes() {
  let text: string | null = null;
  try {
    text = localStorage.getItem(KEY);
  } catch {
    // Defaults are fine.
  }
  setState({ paneSizes: parsePaneSizes(text) });
}

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(getState().paneSizes));
  } catch {
    // Not critical: sizes fall back to the defaults next time.
  }
}

/** The CSS variables the panes are laid out by. */
const PANE_VARS = { library: "--library-w", inspector: "--inspector-w", browser: "--browser-w", mixer: "--mixer-h", editor: "--editor-h", fx: "--fx-h" } as const;

/**
 * A ref callback that keeps the pane sizes on its element as CSS variables, straight from the
 * store, so dragging a divider lays the window out again without rendering it.
 */
export function paneVars(el: HTMLElement | null) {
  if (!el) return;
  let last: unknown = null;
  const apply = () => {
    const sizes = getState().paneSizes;
    if (sizes === last) return;
    last = sizes;
    for (const [k, name] of Object.entries(PANE_VARS)) el.style.setProperty(name, `${sizes[k as keyof typeof PANE_VARS]}px`);
  };
  apply();
  return subscribeState(apply);
}

/** Sets a pane's size within its limits and within `max`, the room there is for it. */
function setPaneSize(k: PaneKey, v: number, max = Infinity) {
  setState({ paneSizes: { ...getState().paneSizes, [k]: clampPane(k, v, max) } });
}

/**
 * A divider that resizes `pane`. `dir` is the drag axis; `sign` is +1 when dragging right/down
 * grows the pane (it sits on the pane's right/bottom edge) and -1 when it shrinks it.
 * `room`, when given, is the most the pane can take now (the bottom section's, which leaves
 * the Tracks area its minimum): a drag starts from the size the pane shows at and stays
 * within it. Double-click restores the default size.
 */
export function Splitter({ pane, dir, sign, room }: { pane: PaneKey; dir: "x" | "y"; sign: 1 | -1; room?: () => number }) {
  useStore((s) => s.paneSizes[pane]);
  const start = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const from = dir === "x" ? e.clientX : e.clientY;
    const max = room?.() ?? Infinity;
    const base = Math.min(getState().paneSizes[pane], max);
    document.body.style.cursor = dir === "x" ? "col-resize" : "row-resize";
    el.classList.add("dragging");
    const move = (ev: PointerEvent) => setPaneSize(pane, base + sign * ((dir === "x" ? ev.clientX : ev.clientY) - from), max);
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      el.classList.remove("dragging");
      document.body.style.cursor = "";
      save();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  return (
    <div
      className={`splitter splitter-${dir}`}
      onPointerDown={start}
      onDoubleClick={() => {
        setPaneSize(pane, PANE_DEFAULTS[pane], room?.());
        save();
      }}
      title="Drag to resize · double-click to reset"
    />
  );
}
