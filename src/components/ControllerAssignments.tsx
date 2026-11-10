// Logic's Controller Assignments window (Control Surfaces › Controller Assignments): every
// assignment with its controller, parameter, takeover or button mode, range and
// feedback, and the banner shown while an assignment is being learned. Assignments are global
// settings, saved by the engine in the config folder, so editing them isn't an undo step.
import { useEffect, useState } from "react";
import { call } from "../api";
import {
  buttonHint,
  buttonOptions,
  canBeRelative,
  CC_MODE_OPTIONS,
  ccMode,
  ccModePatch,
  channelLabel,
  controllerLabel,
  feedbackTitle,
  fromPercent,
  MODE_OPTIONS,
  noFeedback,
  offersLatching,
  rowEditor,
  selectedTargets,
  toPercent,
  TRANSPORT_TARGETS,
  type AssignmentPatch,
  type AssignmentRow,
} from "../controllers";
import { commandLabel, keyHint, midiCommands } from "../keymap";
import { cancelLearn, learn, learnCommand } from "../learn";
import type { MenuItem } from "./Menu";
import { onTick, setState, useStore } from "../store";
import { Sheet } from "./Dialogs";
import { Check, NumberField, Select } from "./Form";
import { openMenuAt } from "./Menu";
import { onPress } from "../keyNav";

function Row({ row, index, onChange }: { row: AssignmentRow; index: number; onChange: (rows: AssignmentRow[] | null) => void }) {
  const update = (patch: AssignmentPatch) => call<AssignmentRow[]>("update_controller_assignment", { index, patch }).then(onChange);
  const c = row.controller;
  const editor = rowEditor(row);
  const none = noFeedback(row);
  // Key commands are named here; the engine only knows their ids.
  const command = row.target.type === "command" ? commandLabel(row.target.id) : undefined;
  const name = row.target.type === "command" ? `Key Command › ${command ?? row.target.id}` : row.name;
  const available = row.available && command !== null;
  return (
    <div className={"ctl-row" + (available ? "" : " unavailable")} title={available ? undefined : "Not in this project: the assignment does nothing here"}>
      <span className="ctl-controller">{controllerLabel(c)}</span>
      <span className="ctl-channel">{channelLabel(c)}</span>
      <span className="ctl-port" title={c.port}>
        {c.port}
      </span>
      <span className="ctl-param" title={name}>
        {name}
      </span>
      {editor === "takeover" && canBeRelative(c.kind) ? (
        <Select value={ccMode(row)} options={CC_MODE_OPTIONS} onChange={(m) => update(ccModePatch(m))} width={120} />
      ) : editor === "takeover" ? (
        <Select value={row.mode} options={MODE_OPTIONS} onChange={(mode) => update({ mode })} width={120} />
      ) : (
        <Select value={row.button_mode} options={buttonOptions(row)} onChange={(button_mode) => update({ button_mode })} width={120} />
      )}
      {editor === "button" ? (
        <>
          <span className="ctl-button dim">{buttonHint(row.button_mode, row.target.type === "command")}</span>
          {offersLatching(row) ? (
            <Check
              checked={row.latching}
              onChange={(latching) => update({ latching })}
              title="The controller's button latches by itself: on at one press, off at the next (found when learning, if no release followed)"
            >
              Latching
            </Check>
          ) : (
            <span />
          )}
        </>
      ) : (
        <>
          <NumberField value={toPercent(row.min)} min={0} max={100} width={64} onChange={(v) => update({ min: fromPercent(v) })} />
          <NumberField value={toPercent(row.max)} min={0} max={100} width={64} onChange={(v) => update({ max: fromPercent(v) })} />
          {editor === "button-range" && c.kind.type === "note" ? (
            <Check checked={row.velocity} onChange={(velocity) => update({ velocity })} title="The note's velocity picks the value in the range; off, a press goes to Max">
              Velocity
            </Check>
          ) : editor === "takeover" && row.encoding !== "absolute" ? (
            <NumberField
              value={row.sensitivity}
              min={0.1}
              max={20}
              step={0.1}
              width={64}
              title="Sensitivity: how far one step of the encoder moves the parameter (1 is 1/127 of the range)"
              onChange={(sensitivity) => update({ sensitivity })}
            />
          ) : (
            <span />
          )}
        </>
      )}
      {/* Without a paired output the setting is kept for when there is one. A row with nothing
          to send (a program change, a command, an encoder) has the box off and disabled. */}
      <label className={"check ctl-feedback" + (row.output && !none ? "" : " dim")} title={none ?? feedbackTitle(row)}>
        <input
          type="checkbox"
          aria-label="Feedback"
          disabled={none !== null}
          checked={row.feedback && none === null}
          onChange={(e) => call<AssignmentRow[]>("set_controller_feedback", { index, on: e.target.checked }).then(onChange)}
        />
      </label>
      <button className="push" title="Delete this assignment" onClick={() => call<AssignmentRow[]>("delete_controller_assignment", { index }).then(onChange)}>
        Delete
      </button>
    </div>
  );
}

/** Learn Key Command…'s menu: every command a controller can fire, a submenu per group. */
function commandMenu(): MenuItem[] {
  const all = midiCommands();
  const groups = [...new Set(all.map((c) => c.group))];
  return groups.map((g) => ({ label: g, children: all.filter((c) => c.group === g).map((c) => ({ label: c.label, onClick: () => learnCommand(c.id) })) }));
}

/** Learn Selected Track…'s menu. */
function selectedMenu(): MenuItem[] {
  return selectedTargets().map(({ label, target, children }) =>
    children ? { label, children: children.map((c) => ({ label: c.label, onClick: () => learn(c.target) })) } : { label, onClick: () => learn(target!) },
  );
}

/** The Controller Assignments sheet. */
export function ControllerAssignmentsSheet() {
  const [rows, setRows] = useState<AssignmentRow[]>([]);
  const project = useStore((s) => s.snap?.project);
  const close = () => setState({ showControllerAssignments: false });
  const apply = (r: AssignmentRow[] | null) => r && setRows(r);
  // Names follow the project (a renamed or deleted track), and a learn adds a row.
  useEffect(() => {
    call<AssignmentRow[]>("controller_assignments").then(apply);
  }, [project]);
  useEffect(
    () =>
      onTick((t) => {
        if (t.learned) call<AssignmentRow[]>("controller_assignments").then(apply);
      }),
    [],
  );
  const learnKey = keyHint("learn-assignment");
  return (
    // Non-modal, like Logic's window: Learn Assignment and the transport keys still work.
    <Sheet title="Controller Assignments" onClose={close} width={960} mode="non-modal">
      <div className="ctl-head">
        <span>Controller</span>
        <span>Channel</span>
        <span>Port</span>
        <span>Parameter</span>
        <span>Mode</span>
        <span>Min %</span>
        <span>Max %</span>
        <span />
        <span title="Send the parameter's value back to the controller (Settings › MIDI pairs inputs with outputs)">Feedback</span>
        <span />
      </div>
      <div className="ctl-list">
        {rows.length === 0 && (
          <div className="pref-hint">
            No assignments yet. Move a fader, knob or plug-in parameter, choose Learn Assignment{learnKey ? ` (${learnKey})` : ""}, then move a control or press a
            pad on your MIDI controller. Right-click a control to learn it directly.
          </div>
        )}
        {rows.map((r, i) => (
          <Row key={`${i}:${r.controller.port}:${r.controller.channel}:${JSON.stringify(r.controller.kind)}`} row={r} index={i} onChange={apply} />
        ))}
      </div>
      <div className="sheet-foot">
        <div className="sheet-buttons">
          <button className="push" {...onPress((e) => openMenuAt(e.currentTarget, TRANSPORT_TARGETS.map(({ label, target }) => ({ label, onClick: () => learn(target) }))))}>
            Learn Transport Button…
          </button>
          <button className="push" title="Assign a pad or button to any key command or menu item" {...onPress((e) => openMenuAt(e.currentTarget, commandMenu()))}>
            Learn Key Command…
          </button>
          <button className="push" title="Assign a controller to a control of whichever track is selected" {...onPress((e) => openMenuAt(e.currentTarget, selectedMenu()))}>
            Learn Selected Track…
          </button>
        </div>
        <div className="sheet-buttons">
          <button className="push default" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </Sheet>
  );
}

/** "Move a controller to assign it to …" with Cancel, while a learn waits; Esc cancels too. */
export function LearnBanner() {
  const [name, setName] = useState<string | null>(null);
  useEffect(
    () =>
      onTick((t) => {
        setName(t.learn ?? null);
        if (t.learned) setState({ toast: { text: t.learned, at: Date.now(), ms: 3000 } });
      }),
    [],
  );
  useEffect(() => {
    if (!name) return;
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      cancelLearn();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [name]);
  if (!name) return null;
  return (
    <div className="learn-banner">
      <span>
        Move a controller or press a pad to assign it to <b>{name}</b>…
      </span>
      <button className="push" onClick={cancelLearn}>
        Cancel
      </button>
    </div>
  );
}
