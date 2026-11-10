// The Piano Roll's local bar: Edit, Functions and View, the tool, Quantize, grid, velocity and zoom controls.
// It keeps to one row: short of room, its captions go, it tightens, and its least used controls
// move into the View menu (FIT_STEPS).
import { Icon } from "../../ui/icons";
import * as me from "../midiEdit";
import { fmt, keyHint } from "../../keyboard/keymap";
import { setState } from "../../state/store";
import { ownPanel } from "../../app/detach/detach";
import { DetachButton } from "../../app/detach/DetachButton";
import { EditorTabs } from "../EditorTabs";
import { openMenuAt, type MenuItem } from "../../ui/Menu";
import { useBarFit } from "../../ui/useBarFit";
import type { Commands } from "./commands";
import type { Menus } from "./menus";
import { setPrefs, type Prefs } from "./prefs";
import { sliderToZoom } from "./model";
import { ToolIcon } from "./ToolIcon";
import { onPress } from "../../keyboard/keyNav";

/** What the bar shows and does; everything else it reaches through the editor's parts. */
export interface PianoRollBarProps {
  pr: Prefs;
  title: string;
  toolBtn: React.RefObject<HTMLButtonElement | null>;
  qBtn: React.RefObject<HTMLButtonElement | null>;
  qOpen: boolean;
  setQOpen: (open: boolean) => void;
  selNotes: { length: number };
  shownVel: number | null;
  showStepInput: boolean;
  velPreview: (val: number) => void;
  velCommit: () => void;
  hZoom: number;
  commands: Pick<Commands, "quantizeNow" | "zoomX" | "zoomY">;
  menus: Pick<Menus, "editItems" | "functionItems" | "viewItems" | "toolItems">;
}

/** What gives way, in this order, when the bar is short of room (barFit.ts). The tool and the
 *  menus never do; what moves out of the bar is in the View menu, Quantize in Functions. */
const FIT_STEPS = ["captions", "tight", "height", "tabs", "detach", "catch", "midi-out", "step-input", "zoom", "grid", "title", "quantize", "velocity"] as const;

export function PianoRollBar({ pr, title, toolBtn, qBtn, qOpen, setQOpen, selNotes, shownVel, showStepInput, velPreview, velCommit, hZoom, commands, menus }: PianoRollBarProps) {
  const { quantizeNow, zoomX, zoomY } = commands;
  const { editItems, functionItems, viewItems, toolItems } = menus;
  const fit = useBarFit(FIT_STEPS, `${title}|${pr.qValue}|${pr.qSwing}|${pr.grid}`);
  const gone = fit.has;
  // The controls that left the bar and aren't in the View menu already.
  const moved = (): MenuItem[] => [
    ...(gone("step-input") && !ownPanel ? [{ label: "Step Input Keyboard", hint: keyHint("step-input"), checked: showStepInput, onClick: () => setState({ showStepInput: !showStepInput }) }] : []),
    ...(gone("grid")
      ? [
          { label: "Snap to Grid", checked: pr.snap, onClick: () => setPrefs({ snap: !pr.snap }) },
          { label: "Grid", children: me.GRID_VALUES.map((g) => ({ label: g, checked: g === pr.grid, onClick: () => setPrefs({ grid: g }) })) },
        ]
      : []),
  ];
  const view = (): MenuItem[] => {
    const extra = moved();
    return extra.length ? [...viewItems(), { separator: true }, ...extra] : viewItems();
  };
  return (
    <div ref={fit.ref} className={"local-bar editor-bar fit-bar pr-bar" + fit.className}>
      <EditorTabs current="editor" short={gone("tabs")} />
      <div className="local-menus">
        <button className="local-menu" {...onPress((e) => openMenuAt(e.currentTarget, editItems()))}>
          Edit
        </button>
        <button className="local-menu" {...onPress((e) => openMenuAt(e.currentTarget, functionItems()))}>
          Functions
        </button>
        <button className="local-menu" {...onPress((e) => openMenuAt(e.currentTarget, view()))}>
          View
        </button>
        {!gone("title") && <span className="local-title">{title}</span>}
      </div>
      <div className="local-right">
        <button ref={toolBtn} className="popup pr-tool" title="Tool (T)" aria-label="Tool (T)" {...onPress((e) => openMenuAt(e.currentTarget, toolItems()))}>
          <ToolIcon tool={pr.tool} /> <Icon.chevron />
        </button>
        {!gone("quantize") && (
          <>
            <span className="lb-label">Quantize</span>
            <span className="pr-qgroup">
              <button ref={qBtn} className="popup" title="Time Quantize settings" aria-expanded={qOpen} {...onPress(() => setQOpen(!qOpen))}>
                {pr.qValue}
                {pr.qSwing ? ` · ${pr.qSwing}%` : ""} <Icon.chevron />
              </button>
              <button className="lb-btn pr-q" title={`Quantize ${selNotes.length ? "selected notes" : "all notes"} (Q)`} aria-label={`Quantize ${selNotes.length ? "selected notes" : "all notes"} (Q)`} onClick={quantizeNow}>
                Q
              </button>
            </span>
          </>
        )}
        {!gone("grid") && (
          <>
            <span className="lb-label">Grid</span>
            <button
              className="popup"
              title="Grid and note length for the Pencil"
              {...onPress((e) => openMenuAt(e.currentTarget, me.GRID_VALUES.map((g) => ({ label: g, checked: g === pr.grid, onClick: () => setPrefs({ grid: g }) }))))}
            >
              {pr.grid} <Icon.chevron />
            </button>
            <button className={"lb-btn" + (pr.snap ? " on" : "")} aria-pressed={pr.snap} title={`Snap to grid (hold ${fmt("Mod")} while dragging to move freely)`} aria-label={`Snap to grid (hold ${fmt("Mod")} while dragging to move freely)`} onClick={() => setPrefs({ snap: !pr.snap })}>
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
                <path d="M3 2v5a4 4 0 0 0 8 0V2M3 4.5h2.4M8.6 4.5H11M5.4 2v5a1.6 1.6 0 0 0 3.2 0V2" />
              </svg>
            </button>
          </>
        )}
        {!gone("velocity") && (
          <>
            <span className="lb-label" title="Velocity of the selected notes">
              Vel
            </span>
            <input
              className="zoom pr-vel"
              type="range"
              min={1}
              max={127}
              step={1}
              disabled={shownVel === null}
              value={shownVel ?? 100}
              title="Velocity of the selected notes"
              onChange={(e) => velPreview(parseInt(e.target.value))}
              onPointerUp={velCommit}
              onKeyUp={velCommit}
            />
            {/* Tight, "No notes" is a dash; the tooltip says what it means. */}
            <span className={"pr-vel-val" + (shownVel === null ? " none" : "")} title={shownVel === null ? "Select notes to set their velocity" : undefined}>
              {shownVel ?? (gone("tight") ? "–" : "No notes")}
            </span>
          </>
        )}
        {!gone("midi-out") && (
          <button className={"lb-btn" + (pr.midiOut ? " on" : "")} aria-pressed={pr.midiOut} title="MIDI Out: hear notes as you edit them" aria-label="MIDI Out: hear notes as you edit them" onClick={() => setPrefs({ midiOut: !pr.midiOut })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M2 5.2h2.4L7.6 2.5v9L4.4 8.8H2zM9.8 4.6a3.4 3.4 0 0 1 0 4.8M11.4 3a5.6 5.6 0 0 1 0 8" />
            </svg>
          </button>
        )}
        {/* The Step Input Keyboard lives in the main window. */}
        {!ownPanel && !gone("step-input") && (
          <button className={"lb-btn" + (showStepInput ? " on" : "")} aria-pressed={showStepInput} title={`Step Input Keyboard (${keyHint("step-input")})`} aria-label={`Step Input Keyboard (${keyHint("step-input")})`} onClick={() => setState({ showStepInput: !showStepInput })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M4.5 10.5V2.5h5" />
              <ellipse cx="3" cy="10.5" rx="1.8" ry="1.4" fill="currentColor" stroke="none" />
              <path d="M8 7.5h4.5M10.5 5.5l2 2-2 2" />
            </svg>
          </button>
        )}
        {!gone("catch") && (
          <button className={"lb-btn catch" + (pr.catchPlayhead ? " on" : "")} aria-pressed={pr.catchPlayhead} title="Catch Playhead" aria-label="Catch Playhead" onClick={() => setPrefs({ catchPlayhead: !pr.catchPlayhead })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M3 2v10M3 7h8M8 4l3 3-3 3" />
            </svg>
          </button>
        )}
        {!gone("height") && (
          <>
            <span className="lb-label" title="Row height">
              Height
            </span>
            <input className="zoom vzoom" type="range" min={5} max={32} step={0.5} value={pr.noteH} title={`Row height (${fmt("Alt")}-scroll, ${fmt("Mod+ArrowUp")} / ${fmt("Mod+ArrowDown")})`} onChange={(e) => zoomY(parseFloat(e.target.value) / pr.noteH)} />
          </>
        )}
        {!gone("zoom") && (
          <>
            <span className="lb-label" title="Horizontal zoom">
              Zoom
            </span>
            <input
              className="zoom"
              type="range"
              min={0}
              max={1}
              step={0.001}
              value={hZoom}
              title={`Horizontal zoom (${fmt("Mod")}-scroll, ${fmt("Mod+ArrowLeft")} / ${fmt("Mod+ArrowRight")})`}
              onChange={(e) => zoomX(sliderToZoom(parseFloat(e.target.value)) / pr.pxPerBeat)}
            />
          </>
        )}
        {!gone("detach") && <DetachButton panel="editor" />}
      </div>
    </div>
  );
}
