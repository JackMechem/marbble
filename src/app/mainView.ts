// The main window's two views, like Logic's Tracks and Mixer windows or Pro Tools' Edit and Mix
// windows: the Tracks area with the bottom section under it, or the Mixer filling the centre
// column. The control bar, the Inspector and the side panes stay in both. Pure, so it can be unit
// tested (tests/app/mainView.test.ts); App.tsx lays the views out.
import type { BottomPane } from "../state/store";

export type MainView = "tracks" | "mixer";

interface Views {
  mainView: MainView;
  bottom: BottomPane;
}

/**
 * What the bottom section shows in a view. Under the Tracks area, any pane; under the Mixer, only
 * the docked effect panels its slots open. The editors edit what the Tracks area shows, and a
 * Mixer pane under the Mixer would be the same strips twice, so they wait for the Tracks view
 * (`bottom` keeps them).
 */
export function bottomIn(view: MainView, bottom: BottomPane): BottomPane {
  return view === "tracks" || bottom === "fx" ? bottom : "none";
}

/** Whether `pane` shows in the bottom section now: it lights its button and ticks its menu item. */
export const paneShown = (s: Views, pane: BottomPane): boolean => bottomIn(s.mainView, s.bottom) === pane;

/** Toggling a bottom pane (X, E, D and their buttons) shows or hides it under the Tracks area. In
 *  the Mixer view the Tracks view comes back, with the pane showing. */
export function toggleBottom(s: Views, pane: BottomPane): Partial<Views> {
  if (s.mainView === "mixer" && pane !== "fx") return { mainView: "tracks", bottom: pane };
  return { bottom: s.bottom === pane ? "none" : pane };
}

/** The Mixer view's key command: the Mixer view, or back to the Tracks view from it. */
export const toggledView = (v: MainView): MainView => (v === "mixer" ? "tracks" : "mixer");

/** Whether a key command of `group` (keymap.ts) runs in `view`. The Mixer view shows no regions,
 *  so the region edits wait for the Tracks view rather than change what can't be seen. */
export const keyRunsIn = (view: MainView, group: string | undefined): boolean => view === "tracks" || group !== "Editing";

/** The view remembered for a project (localStorage text): the Tracks view unless it was the Mixer. */
export const parseMainView = (text: string | null): MainView => (text === "mixer" ? "mixer" : "tracks");
