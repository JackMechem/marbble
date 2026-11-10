// The one header every plug-in panel has, wherever it shows: a built-in effect, an instrument,
// the Sampler or a Channel EQ, docked in the bottom section (FxDock), floating in the main window
// (FxWindows) or in a window of its own (panelKinds.tsx). Left to right, always in this order:
//
//   power · name · track · ‹ preset ▾ › · the panel's own pop-ups (Side Chain) · · · window buttons · ×
//
// Docked, the tabs stand in for the name and track (the front tab is the panel shown). The window
// buttons keep their places too: Float in Main Window or Dock in Bottom Section first, then Open in
// Separate Window or Attach to Main Window, then Close. A panel's own toggles (the Channel EQ's
// Analyzer, the Linear Phase EQ's Quality) are in its toolbar, under the header.
import type { ReactNode } from "react";
import { isMain } from "../../app/detach/detach";
import type { PanelKey } from "./fxDockModel";
import { closeDocked, closePanel, dockPanel, floatPanel } from "./fxDockState";
import { DetachButton } from "../../app/detach/DetachButton";
import { PanelLead, PanelTools, usePanelTitle } from "./FxPanel";

/** Where a panel shows: the bottom section, an in-app window, or an OS window of its own. */
export type HeaderPlace = "docked" | "floating" | "window";

export function PanelHeader({ panelKey, place, tabs, onMouseDown }: { panelKey: PanelKey; place: HeaderPlace; tabs?: ReactNode; onMouseDown?: (e: React.MouseEvent) => void }) {
  const title = usePanelTitle(panelKey);
  if (!title) return null;
  return (
    <div className={`fx-header ${place} ` + (place === "floating" ? "eq-titlebar" : "local-bar fx-dock-bar")} onMouseDown={onMouseDown}>
      {place === "docked" && tabs}
      <div className="fx-header-main">
        <PanelLead panelKey={panelKey} />
        {place !== "docked" && (
          <>
            <span className="fx-header-name">{title.name}</span>
            <span className="fx-header-track">{title.track}</span>
          </>
        )}
        <PanelTools panelKey={panelKey} />
      </div>
      <div className="fx-header-buttons">
        {place === "docked" && (
          <button className="fx-mode" title="Float in Main Window" aria-label="Float in Main Window" onClick={() => floatPanel(panelKey)}>
            <FloatGlyph />
          </button>
        )}
        {/* A detached Mixer's window has no bottom section to dock in. */}
        {place === "floating" && isMain && (
          <button className="fx-mode" title="Dock in Bottom Section" aria-label="Dock in Bottom Section" onClick={() => dockPanel(panelKey)}>
            <DockGlyph />
          </button>
        )}
        <DetachButton panel={panelKey} className="fx-mode" />
        <CloseButton panelKey={panelKey} name={title.name} docked={place === "docked"} />
      </div>
    </div>
  );
}

/**
 * Closes the panel this header belongs to: docked, the front tab. Shift- or Alt-click there
 * closes every docked panel (each tab's own × and its right-click menu close one or the others).
 */
function CloseButton({ panelKey, name, docked }: { panelKey: PanelKey; name: string; docked: boolean }) {
  const label = `Close ${name} (Esc)`;
  return (
    <button
      className="eq-close"
      title={docked ? `${label}\nShift-click closes every docked panel` : label}
      aria-label={label}
      onClick={(e) => (docked && (e.shiftKey || e.altKey) ? closeDocked() : closePanel(panelKey))}
    >
      ×
    </button>
  );
}

/** The button that puts a floating panel in the bottom section: a window with its lower part filled. */
function DockGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
      <rect x="1.5" y="2" width="11" height="10" rx="1.5" />
      <rect x="1.5" y="8" width="11" height="4" rx="1" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** The button that takes a docked panel out to float over the main window: a window over another. */
function FloatGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
      <rect x="1.5" y="4.5" width="8" height="8" rx="1.3" />
      <path d="M5 3V2.8A1.3 1.3 0 0 1 6.3 1.5h5A1.3 1.3 0 0 1 12.5 2.8v5A1.3 1.3 0 0 1 11.2 9.1H11" />
    </svg>
  );
}
