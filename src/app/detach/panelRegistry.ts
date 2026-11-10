// The kinds of panel that can be detached into a window of their own, by the kind in a panel key
// ("eq" in "eq:12"). panelKinds.tsx registers them; detach.ts and panelMemory.ts look them up.
// The bottom section's panes (the Mixer, the Editors) are kinds made by `bottomPanel`.
import type { ReactNode } from "react";
import { bottomAfterHide, parsePanelKey } from "./detachModel";
import { dockedPanels } from "../../plugins/panels/fxDockModel";
import { getState, type BottomPane, type State } from "../../state/store";

/** One kind of detachable panel; `arg` is the part of the key after the kind ("12" in "eq:12"). */
export interface PanelKind {
  /** The window title. */
  title(arg: string, s: State): string;
  /** What the panel's window shows. */
  render(arg: string): ReactNode;
  /** Whether this window's state shows the panel (in the bottom section, a floating panel…). */
  shown(arg: string, s: State): boolean;
  /** The state change that takes the panel out of this window. */
  hide(arg: string, s: State): Partial<State>;
  /** The state change that shows it in this window (attached again, or in its own window). */
  show(arg: string, s: State): Partial<State>;
  /** False once what the panel shows is gone (its track, its effect): the main window closes it. */
  alive?(arg: string, s: State): boolean;
  /** In the panel's own window: a change that keeps its state fit to show it, or null. */
  own?(arg: string, s: State): Partial<State> | null;
  /** Reopened at launch if it was detached when Marbble quit. */
  restore?: boolean;
  /** A first window's size, in logical pixels. */
  size: { w: number; h: number };
  /** Where its window's geometry is remembered, if not per kind (one per type of effect). */
  memoryKey?(arg: string, s: State): string;
  /** A first window's size when it depends on the panel (an effect's natural size). */
  sizeOf?(arg: string, s: State): { w: number; h: number };
}

const kinds = new Map<string, PanelKind>();

export function registerPanelKind(kind: string, k: PanelKind) {
  kinds.set(kind, k);
}

export function panelKind(kind: string): PanelKind | undefined {
  return kinds.get(kind);
}

/** A panel key's kind (`k`, if registered), its kind's name and the argument after it. */
export function kindOf(key: string) {
  const { kind, arg } = parsePanelKey(key);
  return { k: kinds.get(kind), kind, arg };
}

// ---- the bottom section's panes ----------------------------------------------------------------

/** Kinds that are bottom-section panes ("mixer", "editor"). */
const bottomKinds = new Set<string>();
/** What the bottom section showed before its latest change (where a leaving pane goes back to). */
let bottomBefore: BottomPane = getState().bottom;
let lastBottom: BottomPane = getState().bottom;

/** Keeps track of the bottom section's previous pane; detach.ts calls it on every state change. */
export function followBottom(bottom: BottomPane) {
  if (bottom === lastBottom) return;
  bottomBefore = lastBottom;
  lastBottom = bottom;
}

/**
 * A bottom-section pane as a panel kind: shown while the section shows it, and when it leaves,
 * the section goes back to what it showed before. Its own window keeps `bottom` on it.
 */
export function bottomPanel(pane: BottomPane, k: Pick<PanelKind, "title" | "render" | "size"> & Pick<PanelKind, "own">): PanelKind {
  bottomKinds.add(pane);
  return {
    ...k,
    restore: true,
    shown: (_, s) => s.bottom === pane,
    hide: (_, s) => {
      const detached = s.detached.filter((d) => bottomKinds.has(parsePanelKey(d).kind));
      let bottom = bottomAfterHide(pane, bottomBefore, detached) as BottomPane;
      // The docked effect panels' pane is only worth going back to while it has panels.
      if (bottom === "fx" && !dockedPanels(s.fxDock).length) bottom = "none";
      // Nor should closing the last docked panel bring this one back (fxDockModel.ts `ret`).
      return s.fxDock.ret === pane ? { bottom, fxDock: { ...s.fxDock, ret: "none" } } : { bottom };
    },
    show: () => ({ bottom: pane }),
    own: k.own ?? ((_, s) => (s.bottom === pane ? null : { bottom: pane })),
  };
}
