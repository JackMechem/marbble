// The ruler's right-click menu (above the marker lane, which has its own). Pure: Ruler.tsx passes
// the state and the commands in, so tests can import it.
import type { MenuItem } from "./components/Menu";
import type { Snap } from "./store";

export const SNAP_LABELS: Record<Snap, string> = { smart: "Smart", bar: "Bar", beat: "Beat", off: "Off" };

export interface RulerMenuState {
  cycle: boolean;
  autopunch: boolean;
  /** Whether any region is selected (for Set Locators by Regions). */
  regions: boolean;
  snap: Snap;
  /** Key hints by keymap.ts id. */
  hints: { cycle?: string; autopunch?: string; setLocators?: string };
}

export interface RulerCommands {
  locate: () => void;
  createMarker: () => void;
  toggleCycle: () => void;
  toggleAutopunch: () => void;
  setLocatorsByRegions: () => void;
  setSnap: (s: Snap) => void;
}

/** The menu for a right-click on the cycle strip, the bar numbers or the Autopunch strip. The
 *  position-taking commands act at the click, snapped as the ruler snaps. */
export function rulerMenu(s: RulerMenuState, c: RulerCommands): MenuItem[] {
  return [
    { label: "Move Playhead Here", onClick: c.locate },
    { label: "Create Marker Here", onClick: c.createMarker },
    { separator: true },
    { label: "Cycle", hint: s.hints.cycle, checked: s.cycle, onClick: c.toggleCycle },
    { label: "Autopunch", hint: s.hints.autopunch, checked: s.autopunch, onClick: c.toggleAutopunch },
    { label: "Set Locators by Regions", hint: s.hints.setLocators, disabled: !s.regions, onClick: c.setLocatorsByRegions },
    { separator: true },
    { label: "Snap", children: (Object.keys(SNAP_LABELS) as Snap[]).map((k) => ({ label: SNAP_LABELS[k], checked: s.snap === k, onClick: () => c.setSnap(k) })) },
  ];
}
