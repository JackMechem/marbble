// The Piano Roll's local bar: Edit, Functions and View, the tool, Quantize, grid, velocity and zoom controls.
import { Icon } from "../../icons";
import * as me from "../../midiEdit";
import { fmt, keyHint } from "../../keymap";
import { setState } from "../../store";
import { ownPanel } from "../../detach";
import { DetachButton } from "../DetachButton";
import { EditorTabs } from "../EventList";
import { openMenuAt } from "../Menu";
import type { Commands } from "./commands";
import type { Menus } from "./menus";
import { setPrefs, type Prefs } from "./prefs";
import { ToolIcon } from "./ToolIcon";

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

export function PianoRollBar({ pr, title, toolBtn, qBtn, qOpen, setQOpen, selNotes, shownVel, showStepInput, velPreview, velCommit, hZoom, commands, menus }: PianoRollBarProps) {
  const { quantizeNow, zoomX, zoomY } = commands;
  const { editItems, functionItems, viewItems, toolItems } = menus;
  return (
    <div className="local-bar editor-bar pr-bar">
      <EditorTabs current="editor" />
      <div className="local-menus">
        <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, editItems())}>
          Edit
        </button>
        <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, functionItems())}>
          Functions
        </button>
        <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, viewItems())}>
          View
        </button>
        <span className="local-title">{title}</span>
      </div>
      <div className="local-right">
        <button ref={toolBtn} className="popup pr-tool" title="Tool (T)" aria-label="Tool (T)" onMouseDown={(e) => openMenuAt(e.currentTarget, toolItems())}>
          <ToolIcon tool={pr.tool} /> <Icon.chevron />
        </button>
        <span className="lb-label">Quantize</span>
        <span className="pr-qgroup">
          <button ref={qBtn} className="popup" title="Time Quantize settings" onMouseDown={() => setQOpen(!qOpen)}>
            {pr.qValue}
            {pr.qSwing ? ` · ${pr.qSwing}%` : ""} <Icon.chevron />
          </button>
          <button className="lb-btn pr-q" title={`Quantize ${selNotes.length ? "selected notes" : "all notes"} (Q)`} aria-label={`Quantize ${selNotes.length ? "selected notes" : "all notes"} (Q)`} onClick={quantizeNow}>
            Q
          </button>
        </span>
        <span className="lb-label">Grid</span>
        <button
          className="popup"
          title="Grid and note length for the Pencil"
          onMouseDown={(e) => openMenuAt(e.currentTarget, me.GRID_VALUES.map((g) => ({ label: g, checked: g === pr.grid, onClick: () => setPrefs({ grid: g }) })))}
        >
          {pr.grid} <Icon.chevron />
        </button>
        <button className={"lb-btn" + (pr.snap ? " on" : "")} title={`Snap to grid (hold ${fmt("Mod")} while dragging to move freely)`} aria-label={`Snap to grid (hold ${fmt("Mod")} while dragging to move freely)`} onClick={() => setPrefs({ snap: !pr.snap })}>
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
            <path d="M3 2v5a4 4 0 0 0 8 0V2M3 4.5h2.4M8.6 4.5H11M5.4 2v5a1.6 1.6 0 0 0 3.2 0V2" />
          </svg>
        </button>
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
        <span className={"pr-vel-val" + (shownVel === null ? " none" : "")} title={shownVel === null ? "Select notes to set their velocity" : undefined}>
          {shownVel ?? "No notes"}
        </span>
        <button className={"lb-btn" + (pr.midiOut ? " on" : "")} title="MIDI Out: hear notes as you edit them" aria-label="MIDI Out: hear notes as you edit them" onClick={() => setPrefs({ midiOut: !pr.midiOut })}>
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
            <path d="M2 5.2h2.4L7.6 2.5v9L4.4 8.8H2zM9.8 4.6a3.4 3.4 0 0 1 0 4.8M11.4 3a5.6 5.6 0 0 1 0 8" />
          </svg>
        </button>
        {/* The Step Input Keyboard lives in the main window. */}
        {!ownPanel && (
          <button className={"lb-btn" + (showStepInput ? " on" : "")} title={`Step Input Keyboard (${keyHint("step-input")})`} aria-label={`Step Input Keyboard (${keyHint("step-input")})`} onClick={() => setState({ showStepInput: !showStepInput })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M4.5 10.5V2.5h5" />
              <ellipse cx="3" cy="10.5" rx="1.8" ry="1.4" fill="currentColor" stroke="none" />
              <path d="M8 7.5h4.5M10.5 5.5l2 2-2 2" />
            </svg>
          </button>
        )}
        <button className={"lb-btn catch" + (pr.catchPlayhead ? " on" : "")} title="Catch Playhead" aria-label="Catch Playhead" onClick={() => setPrefs({ catchPlayhead: !pr.catchPlayhead })}>
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
            <path d="M3 2v10M3 7h8M8 4l3 3-3 3" />
          </svg>
        </button>
        <span className="lb-label" title="Row height">
          Height
        </span>
        <input className="zoom vzoom" type="range" min={5} max={32} step={0.5} value={pr.noteH} title={`Row height (${fmt("Alt")}-scroll, ${fmt("Mod+ArrowUp")} / ${fmt("Mod+ArrowDown")})`} onChange={(e) => zoomY(parseFloat(e.target.value) / pr.noteH)} />
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
          onChange={(e) => zoomX((6 * Math.pow(1600 / 6, parseFloat(e.target.value))) / pr.pxPerBeat)}
        />
        <DetachButton panel="editor" />
      </div>
    </div>
  );
}
