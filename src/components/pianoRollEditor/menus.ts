// The Piano Roll's menus: Edit, Functions, View, the lane picker and the Tool menu.
import { undoRedo } from "../../files";
import * as me from "../../midiEdit";
import { fmt } from "../../keymap";
import { detachMenuItems } from "../../detach";
import { getState } from "../../store";
import { type MenuItem } from "../Menu";
import { canRamp, createRamps, CURVE_SHAPES, densityStep, DENSITIES, thinOut } from "../../ccCurves";
import type { Commands } from "./commands";
import { ccClipboard, noteClipboard } from "./commands";
import type { Core } from "./core";
import { setPrefs } from "./prefs";
import { LANES, laneKind, timeBase, TOOLS } from "../../pianoRollModel";

export type Menus = ReturnType<typeof editorMenus>;

export function editorMenus(ed: Core, c: Commands) {
  const { grid, musical, commit, setSelection, work, live } = ed;
  const { zoomX, zoomY, zoomToFit, notesNow, anySelected, playhead, canEdit, copy, deleteSelected, cut, paste, duplicate, selectAll, apply, quantizeNow, msToSamples, humanize, ask, exportRegion } = c;

  /** The Functions menu, also used by the note context menu. */
  const functionItems = (): MenuItem[] => {
    const { project, pr } = live.current;
    const any = anySelected();
    const scope = any ? "" : " (All)";
    return [
      { label: `Quantize${scope}`, hint: "Q", onClick: quantizeNow },
      {
        label: "Quantize to",
        children: me.GRID_VALUES.map((g) => ({
          label: g,
          checked: g === pr.qValue,
          onClick: () => {
            setPrefs({ qValue: g });
            quantizeNow();
          },
        })),
      },
      { label: `Scale Quantize to ${project.key}`, onClick: () => apply((n) => me.scaleQuantize(n, me.scaleOf(project.key)), "Scale Quantize") },
      { separator: true },
      {
        label: "Transpose",
        children: [
          ...(
            [
              ["Octave Up", 12],
              ["Semitone Up", 1],
              ["Semitone Down", -1],
              ["Octave Down", -12],
            ] as [string, number][]
          ).map(([label, d]) => ({ label, onClick: () => apply((n) => me.transpose(n, d), "Transpose") })),
          { separator: true },
          { label: "Other…", onClick: () => ask("Transpose", "Semitones:", "7", ([d]) => apply((n) => me.transpose(n, Math.round(d)), "Transpose")) },
        ],
      },
      {
        label: "Velocity",
        children: [
          ...[32, 64, 80, 100, 127].map((v) => ({ label: `Set to ${v}`, onClick: () => apply((n) => me.setVelocity(n, v), "Set Velocity") })),
          { separator: true },
          ...[50, 80, 90, 110, 120, 150].map((p) => ({ label: `Scale ${p}%`, onClick: () => apply((n) => me.scaleVelocity(n, p), "Scale Velocity") })),
          { separator: true },
          { label: "Set to…", onClick: () => ask("Set Velocity", "Velocity (1–127):", "100", ([v]) => apply((n) => me.setVelocity(n, v), "Set Velocity")) },
          { label: "Scale by…", onClick: () => ask("Scale Velocity", "Percent:", "110", ([p]) => apply((n) => me.scaleVelocity(n, p), "Scale Velocity")) },
        ],
      },
      {
        label: "Humanize",
        children: [
          { label: "Light (±5 ms, ±5 velocity)", onClick: () => humanize(5, 5) },
          { label: "Medium (±10 ms, ±10 velocity)", onClick: () => humanize(10, 10) },
          { label: "Strong (±20 ms, ±20 velocity)", onClick: () => humanize(20, 20) },
          { label: "Custom…", onClick: () => ask("Humanize", "Timing (ms), velocity:", "10, 10", ([ms, v]) => humanize(ms, v ?? 0)) },
        ],
      },
      { separator: true },
      {
        label: "Note Length",
        children: [
          { label: "Legato", onClick: () => apply(me.legato, "Legato") },
          { label: "Remove Overlaps", onClick: () => apply(me.removeOverlaps, "Remove Overlaps") },
          { label: `Set to Grid (${pr.grid})`, onClick: () => apply((n) => musical(n, (v) => me.setLength(v, grid().grid)), "Set Note Length") },
          { separator: true },
          ...(["1/1", "1/2", "1/4", "1/8", "1/16", "1/32"] as const).map((g) => ({ label: `Set to ${g}`, onClick: () => apply((n) => musical(n, (v) => me.setLength(v, me.gridLength(timeBase(project), g))), "Set Note Length") })),
        ],
      },
      { label: "Mute / Unmute Notes", hint: fmt("Ctrl+M"), onClick: () => apply(me.toggleMute, "Mute Notes") },
      { label: "Delete Duplicates", onClick: () => apply(me.deleteDuplicates, "Delete Duplicates") },
      { label: "Reverse", onClick: () => apply(me.reverse, "Reverse Notes") },
      { label: "Split at Playhead", onClick: () => apply((n) => me.splitAt(n, playhead(), new Set(me.targets(n))), "Split Notes") },
      { label: "Glue Same Pitches", disabled: !any, onClick: () => apply(me.glue, "Glue Notes") },
    ];
  };

  const editItems = (): MenuItem[] => {
    const snap = getState().snap;
    const any = anySelected();
    return [
      { label: snap?.undo ? `Undo ${snap.undo}` : "Undo", hint: fmt("Mod+Z"), disabled: !snap?.undo, onClick: () => undoRedo("undo") },
      { label: snap?.redo ? `Redo ${snap.redo}` : "Redo", hint: fmt("Mod+Shift+Z"), disabled: !snap?.redo, onClick: () => undoRedo("redo") },
      { separator: true },
      { label: "Cut", hint: fmt("Mod+X"), disabled: !canEdit(), onClick: cut },
      { label: "Copy", hint: fmt("Mod+C"), disabled: !canEdit(), onClick: copy },
      { label: "Paste at Playhead", hint: fmt("Mod+V"), disabled: !noteClipboard.length && !ccClipboard.length, onClick: paste },
      { label: "Duplicate", hint: fmt("Mod+D"), disabled: !any, onClick: duplicate },
      { label: "Delete", hint: fmt("Backspace"), disabled: !canEdit(), onClick: deleteSelected },
      { separator: true },
      { label: "Select All", hint: fmt("Mod+A"), onClick: selectAll },
      { label: "Deselect All", hint: "Esc", onClick: () => setSelection(me.selectNone(notesNow())) },
      { label: "Invert Selection", onClick: () => setSelection(me.invertSelection(notesNow())) },
      { label: "Select Same Pitch", disabled: !any, onClick: () => setSelection(me.selectSamePitch(notesNow())) },
      { label: "Select Highest Notes", onClick: () => setSelection(me.selectTop(notesNow(), true, msToSamples(15))) },
      { label: "Select Lowest Notes", onClick: () => setSelection(me.selectTop(notesNow(), false, msToSamples(15))) },
      {
        label: "Select Muted Notes",
        onClick: () => setSelection(notesNow().map((n) => ({ ...n, sel: n.muted }))),
      },
      {
        label: "Select Inside Locators",
        disabled: !(live.current.project.cycle.end > live.current.project.cycle.start),
        onClick: () => {
          const c = live.current.project.cycle;
          setSelection(notesNow().map((n) => ({ ...n, sel: n.start >= c.start && n.start < c.end })));
        },
      },
      { separator: true },
      { label: "Export Region as MIDI File…", disabled: !work.current.regions.length, onClick: exportRegion },
    ];
  };

  const laneItems = (): MenuItem[] => {
    const cc = work.current.cc;
    const used = (id: string) => {
      const k = laneKind(id);
      return k ? cc.some((c) => me.sameKind(c.kind, k)) : false;
    };
    const custom = cc.filter((c) => c.kind.type === "controller" && !LANES.some((l) => l.id === `cc:${(c.kind as { cc: number }).cc}`)).map((c) => `cc:${(c.kind as { cc: number }).cc}`);
    const ids = [...LANES.map((l) => l.id), ...new Set(custom)];
    const cur = live.current.pr.lane;
    if (!ids.includes(cur)) ids.push(cur);
    return [
      ...ids.map((id) => ({
        label: LANES.find((l) => l.id === id)?.label ?? `Controller ${id.slice(3)}`,
        checked: cur === id,
        hint: used(id) ? "●" : undefined,
        onClick: () => setPrefs({ lane: id, showLane: true }),
      })),
      { separator: true },
      {
        label: "Other Controller…",
        onClick: () => ask("Show Controller", "Controller (0–127):", "74", ([n]) => n >= 0 && n <= 127 && setPrefs({ lane: `cc:${Math.round(n)}`, showLane: true })),
      },
      ...curveItems(),
    ];
  };

  /** The controller lane's curve settings and ramp functions (ccCurves.ts). */
  const curveItems = (): MenuItem[] => {
    const { pr, project } = live.current;
    const kind = laneKind(pr.lane);
    if (!kind) return [];
    const cc = work.current.cc;
    const step = densityStep(me.quarterLength(timeBase(project)), pr.density);
    return [
      { separator: true },
      { label: "Line Curve", children: CURVE_SHAPES.map(([id, label]) => ({ label, checked: pr.curve === id, onClick: () => setPrefs({ curve: id }) })) },
      { label: "Ramp Density", children: DENSITIES.map((d) => ({ label: d, checked: pr.density === d, onClick: () => setPrefs({ density: d }) })) },
      { label: "Create Ramp", disabled: !canRamp(cc, kind), onClick: () => commit(work.current.notes, createRamps(work.current.cc, kind, pr.curve, step), "Create Ramp") },
      {
        label: "Thin Out",
        onClick: () => {
          const next = thinOut(work.current.cc, kind, Math.max(1, Math.round(me.ccMax(kind) / 127)));
          if (next !== work.current.cc) commit(work.current.notes, next, "Thin Out Controller Events");
        },
      },
    ];
  };

  const viewItems = (): MenuItem[] => {
    const p = live.current.pr;
    return [
      { label: "Color by Velocity", checked: p.color === "velocity", onClick: () => setPrefs({ color: "velocity" }) },
      { label: "Color by Region", checked: p.color === "region", onClick: () => setPrefs({ color: "region" }) },
      { label: `Highlight Scale (${live.current.project.key})`, checked: p.scale, onClick: () => setPrefs({ scale: !p.scale }) },
      { separator: true },
      { label: "Show Velocity / Controller Lane", checked: p.showLane, onClick: () => setPrefs({ showLane: !p.showLane }) },
      { label: "Lane", children: laneItems() },
      { separator: true },
      { label: "Catch Playhead", checked: p.catchPlayhead, onClick: () => setPrefs({ catchPlayhead: !p.catchPlayhead }) },
      { label: "MIDI Out (Hear Edited Notes)", checked: p.midiOut, onClick: () => setPrefs({ midiOut: !p.midiOut }) },
      { separator: true },
      { label: "Zoom In", hint: fmt("Mod+ArrowRight"), onClick: () => zoomX(1.4) },
      { label: "Zoom Out", hint: fmt("Mod+ArrowLeft"), onClick: () => zoomX(1 / 1.4) },
      { label: "Taller Rows", hint: fmt("Mod+ArrowDown"), onClick: () => zoomY(1.2) },
      { label: "Shorter Rows", hint: fmt("Mod+ArrowUp"), onClick: () => zoomY(1 / 1.2) },
      { label: "Zoom to Fit Region", hint: "Z", onClick: zoomToFit },
      ...detachMenuItems("editor"),
    ];
  };

  const toolItems = (): MenuItem[] =>
    TOOLS.map((t) => ({ label: t.label, hint: t.key, checked: live.current.pr.tool === t.id, onClick: () => setPrefs({ tool: t.id }) }));

  return { functionItems, editItems, laneItems, curveItems, viewItems, toolItems };
}
