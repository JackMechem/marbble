// Settings › Key Commands: listing, rebinding and conflicts, and each command's MIDI triggers
// (Learn MIDI): controller assignments to the command, kept with the others by the engine.
import { useEffect, useRef, useState } from "react";
import { call } from "../../engine/commands";
import { channelLabel, controllerLabel, type AssignmentRow } from "../../controllers/controllers";
import { formatKey, KEY_COMMANDS, KEY_GROUPS, eventKey, keyConflict, keysFor, MENU_SHORTCUTS, normalizeKey } from "../../keyboard/keymap";
import { cancelLearn, learnCommand } from "../../controllers/learn";
import { getSettings, useSettings } from "../appSettings";
import { grabKeys } from "../../keyboard/sheetKeys";
import { onTick } from "../../state/store";
import { Foot, setUi } from "./parts";

/** A command's MIDI triggers: the assignments to it, with their place in the list. */
const triggersOf = (rows: AssignmentRow[], id: string) =>
  rows.flatMap((r, index) => (r.target.type === "command" && r.target.id === id ? [{ index, label: `${controllerLabel(r.controller)} · ${channelLabel(r.controller)}`, port: r.controller.port }] : []));

/** The MIDI part of a command's row: its triggers, and Learn MIDI. */
function MidiTriggers({ id, rows, learning, onRows, onLearn }: { id: string; rows: AssignmentRow[]; learning: boolean; onRows: (r: AssignmentRow[]) => void; onLearn: () => void }) {
  return (
    <>
      {triggersOf(rows, id).map((t) => (
        <span key={t.index} className="key-chip midi" title={`MIDI: ${t.label}, ${t.port}`}>
          {t.label}
          <button
            title="Remove this MIDI trigger"
            aria-label="Remove this MIDI trigger"
            onClick={() => call<AssignmentRow[]>("delete_controller_assignment", { index: t.index }).then((r) => r && onRows(r))}
          >
            ×
          </button>
        </span>
      ))}
      {learning ? (
        <span className="key-chip capturing">Press a pad or button on your MIDI controller… (Esc cancels)</span>
      ) : (
        <button className="key-add key-midi" title="Learn MIDI: the next pad, button or program change you press runs this command" onClick={onLearn}>
          MIDI
        </button>
      )}
    </>
  );
}

export function KeysPane({ close }: { close: () => void }) {
  const overrides = useSettings((s) => s.ui.keymap);
  const [q, setQ] = useState("");
  const [capture, setCapture] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const captureRef = useRef(capture);
  captureRef.current = capture;
  // Controller assignments (for the MIDI triggers), and the command a MIDI trigger is being
  // learnt for.
  const [assigned, setAssigned] = useState<AssignmentRow[]>([]);
  const [midiFor, setMidiFor] = useState<string | null>(null);
  useEffect(() => {
    const load = () => call<AssignmentRow[]>("controller_assignments").then((r) => r && setAssigned(r));
    load();
    let was: string | null = null;
    return onTick((t) => {
      if (t.learned) load();
      // The learn ended: assigned, cancelled, or taken over by another learn.
      if (was && !t.learn) setMidiFor(null);
      was = t.learn ?? null;
    });
  }, []);
  useEffect(() => {
    if (!midiFor) return;
    return grabKeys((e) => {
      e.preventDefault();
      if (e.key !== "Escape") return;
      cancelLearn();
      setMidiFor(null);
    });
  }, [midiFor]);
  const startMidi = (id: string) => {
    setCapture(null);
    learnCommand(id).then((name) => name && setMidiFor(id));
  };
  const midi = (id: string) => <MidiTriggers id={id} rows={assigned} learning={midiFor === id} onRows={setAssigned} onLearn={() => startMidi(id)} />;

  const setKeys = (map: Record<string, string[]>) => setUi({ keymap: map });
  const withKeys = (map: Record<string, string[]>, id: string, keys: string[]) => {
    const def = (KEY_COMMANDS.find((c) => c.id === id)?.keys ?? []).map(normalizeKey);
    const next = { ...map };
    if (JSON.stringify(keys) === JSON.stringify(def)) delete next[id];
    else next[id] = keys;
    return next;
  };

  useEffect(() => {
    if (!capture) return;
    const k = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapture(null);
        return;
      }
      const key = eventKey(e);
      if (!key) return;
      const id = captureRef.current!;
      setCapture(null);
      const map = getSettings().ui.keymap;
      const clash = keyConflict(key, id, map);
      if (clash && !clash.id) {
        setMessage(`${formatKey(key)} belongs to ${clash.label} in the menu bar, which takes it first. Choose another key.`);
        return;
      }
      let next = map;
      if (clash?.id) {
        next = withKeys(next, clash.id, keysFor(clash.id, next).filter((x) => x !== key));
        setMessage(`${formatKey(key)} was moved from “${clash.label}”.`);
      } else setMessage(null);
      const mine = keysFor(id, next);
      if (!mine.includes(key)) next = withKeys(next, id, [...mine, key]);
      setKeys(next);
    };
    // Ahead of the sheet's Escape handling and the main window's shortcuts.
    return grabKeys(k);
  }, [capture]);

  /** Puts a command's default keys back, taking them from any command that has them now (a
   *  key runs one command only). */
  const restoreDefault = (id: string, defaults: string[]) => {
    const keys = defaults.map(normalizeKey);
    let next = withKeys(overrides, id, keys);
    const moved: string[] = [];
    for (const k of keys) {
      const clash = keyConflict(k, id, next);
      if (!clash?.id) continue;
      next = withKeys(next, clash.id, keysFor(clash.id, next).filter((x) => x !== k));
      moved.push(`${formatKey(k)} from “${clash.label}”`);
    }
    setKeys(next);
    setMessage(moved.length ? `Moved ${moved.join(", ")}.` : null);
  };

  const match = (label: string, keys: string[], id?: string) => {
    if (!q) return true;
    const t = q.toLowerCase();
    const midiLabels = id ? triggersOf(assigned, id).map((x) => x.label.toLowerCase()) : [];
    return label.toLowerCase().includes(t) || keys.some((k) => formatKey(k).toLowerCase().includes(t) || k.toLowerCase().includes(t)) || midiLabels.some((l) => l.includes(t));
  };
  const customized = Object.keys(overrides).length > 0;

  return (
    <>
      <div className="pm-toolbar">
        <input className="search pm-search" placeholder="Search commands or keys" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
        <span className="dim">{message ?? "Click + and press a key to add a shortcut, or MIDI and press a pad or button on a MIDI controller."}</span>
      </div>
      <div className="key-table">
        {KEY_GROUPS.map((g) => {
          const rows = KEY_COMMANDS.filter((c) => c.group === g && match(c.label, keysFor(c.id, overrides), c.id));
          if (!rows.length) return null;
          return (
            <div key={g}>
              <div className="key-group">{g}</div>
              {rows.map((c) => {
                const keys = keysFor(c.id, overrides);
                return (
                  <div key={c.id} className={"key-row" + (overrides[c.id] ? " custom" : "")}>
                    <span className="key-label">{c.label}</span>
                    <span className="key-keys">
                      {keys.map((k) => (
                        <span key={k} className="key-chip">
                          {formatKey(k)}
                          <button title="Remove this shortcut" aria-label="Remove this shortcut" onClick={() => setKeys(withKeys(overrides, c.id, keys.filter((x) => x !== k)))}>
                            ×
                          </button>
                        </span>
                      ))}
                      {capture === c.id ? (
                        <span className="key-chip capturing">Press a key… (Esc cancels)</span>
                      ) : (
                        <button className="key-add" title="Add a shortcut" aria-label="Add a shortcut" onClick={() => setCapture(c.id)}>
                          +
                        </button>
                      )}
                      {midi(c.id)}
                    </span>
                    {overrides[c.id] ? (
                      <button className="link-btn" onClick={() => restoreDefault(c.id, c.keys)}>
                        Default
                      </button>
                    ) : (
                      <span />
                    )}
                  </div>
                );
              })}
            </div>
          );
        })}
        {MENU_SHORTCUTS.some((m) => match(m.label, [m.key], m.id)) && <div className="key-group">Menu Bar (fixed)</div>}
        {MENU_SHORTCUTS.filter((m) => match(m.label, [m.key], m.id)).map((m) => (
          <div key={m.label} className="key-row fixed">
            <span className="key-label">
              {m.label} <span className="dim">— {m.menu}</span>
            </span>
            <span className="key-keys">
              <span className="key-chip">{formatKey(m.key)}</span>
              {/* Quit is left to the keyboard. */}
              {m.id !== "quit" && midi(m.id)}
            </span>
            <span />
          </div>
        ))}
      </div>
      <div className="pref-hint">Shortcuts and MIDI triggers work in the main window, not while typing in a text field.</div>
      <Foot close={close}>
        <button className="push" disabled={!customized} onClick={() => setKeys({})}>
          Reset All
        </button>
      </Foot>
    </>
  );
}
