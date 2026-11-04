// Settings › Key Commands: listing, rebinding and conflicts.
import { useEffect, useRef, useState } from "react";
import { formatKey, KEY_COMMANDS, KEY_GROUPS, eventKey, keyConflict, keysFor, MENU_SHORTCUTS, normalizeKey } from "../../keymap";
import { getSettings, useSettings } from "../../prefs";
import { grabKeys } from "../../sheetKeys";
import { Foot, setUi } from "./parts";

export function KeysPane({ close }: { close: () => void }) {
  const overrides = useSettings((s) => s.ui.keymap);
  const [q, setQ] = useState("");
  const [capture, setCapture] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const captureRef = useRef(capture);
  captureRef.current = capture;

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

  const match = (label: string, keys: string[]) => {
    if (!q) return true;
    const t = q.toLowerCase();
    return label.toLowerCase().includes(t) || keys.some((k) => formatKey(k).toLowerCase().includes(t) || k.toLowerCase().includes(t));
  };
  const customized = Object.keys(overrides).length > 0;

  return (
    <>
      <div className="pm-toolbar">
        <input className="search pm-search" placeholder="Search commands or keys" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
        <span className="dim">{message ?? "Click + and press a key to add a shortcut."}</span>
      </div>
      <div className="key-table">
        {KEY_GROUPS.map((g) => {
          const rows = KEY_COMMANDS.filter((c) => c.group === g && match(c.label, keysFor(c.id, overrides)));
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
        {MENU_SHORTCUTS.some((m) => match(m.label, [m.key])) && <div className="key-group">Menu Bar (fixed)</div>}
        {MENU_SHORTCUTS.filter((m) => match(m.label, [m.key])).map((m) => (
          <div key={m.label} className="key-row fixed">
            <span className="key-label">
              {m.label} <span className="dim">— {m.menu}</span>
            </span>
            <span className="key-keys">
              <span className="key-chip">{formatKey(m.key)}</span>
            </span>
            <span />
          </div>
        ))}
      </div>
      <div className="pref-hint">Shortcuts work in the main window, not while typing in a text field.</div>
      <Foot close={close}>
        <button className="push" disabled={!customized} onClick={() => setKeys({})}>
          Reset All
        </button>
      </Foot>
    </>
  );
}
