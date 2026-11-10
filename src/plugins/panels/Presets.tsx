// The preset pop-up in built-in effect and instrument panels: factory and user
// presets (shared by every project, see crates/engine/src/library/presets.rs), saving, renaming and
// deleting user presets, and stepping through them. Which preset is loaded comes from the
// project (`PluginSlot.preset`): the engine sets it when a preset is
// loaded or saved, so it is saved with the project and follows undo.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { type Id, type PresetRef } from "../../engine/project";
import { type Snapshot } from "../../engine/snapshot";
import { act, call } from "../../engine/commands";
import { presetBoxLabel } from "./presetBox";
import { setState } from "../../state/store";
import { openMenuAt, type MenuItem } from "../../ui/Menu";
import { onPress } from "../../keyboard/keyNav";

/** A preset as the engine stores it. */
export interface Preset {
  name: string;
  effect: string;
  version: number;
  values?: [number, number][];
}

interface PresetList {
  factory: string[];
  user: string[];
}

/** What saving or renaming a preset returns: its name, and the project showing it. */
interface NamedPreset {
  snapshot: Snapshot;
  name: string;
}

// Bumped when user presets are saved, renamed or deleted, so every box reads its preset again.
let revision = 0;
const listeners = new Set<() => void>();

function presetsChanged() {
  revision++;
  listeners.forEach((l) => l());
}

type Sheet = { mode: "save" | "rename"; text: string; error?: string; replace?: boolean } | { mode: "delete" } | null;

/**
 * The preset pop-up.
 * - `effect`: the preset id ("builtin:deesser", "builtin:channel-eq").
 * - `track`/`slot`: where presets load to and save from.
 * - `loaded`: the preset the project has as loaded there.
 * - `current` and `normalize`: the settings now and a preset's, in one comparable form, which
 *   tells "(edited)" apart.
 * - `defaultName`: the factory preset that resets it ("Default", "Flat" for the Channel EQ).
 * - `extra`: items for the end of the menu.
 */
export function PresetButton({
  effect,
  track,
  slot,
  loaded,
  current,
  normalize,
  defaultName = "Default",
  extra,
}: {
  effect: string;
  track: Id;
  slot: number;
  loaded: PresetRef | null | undefined;
  current: string;
  normalize: (p: Preset) => string;
  defaultName?: string;
  extra?: MenuItem[];
}) {
  const [rev, setRev] = useState(revision);
  const [sheet, setSheet] = useState<Sheet>(null);
  const [defaultNorm, setDefaultNorm] = useState<string | null>(null);
  // The loaded preset's settings, by which preset they are ("f:Name" or "u:Name").
  const [presetNorm, setPresetNorm] = useState<{ key: string; norm: string | null } | null>(null);
  const state = loaded ?? null;
  const key = state ? `${state.factory ? "f" : "u"}:${state.name}` : null;
  useEffect(() => {
    const l = () => setRev(revision);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  useEffect(() => {
    call<Preset>("get_preset", { effect, name: defaultName, factory: true }).then((p) => p && setDefaultNorm(normalize(p)));
    // `normalize` only depends on the effect.
  }, [effect, defaultName]);
  useEffect(() => {
    if (!state || !key) return;
    let live = true;
    // Quietly: a project can name a user preset that isn't on this computer.
    invoke<Preset>("get_preset", { effect, name: state.name, factory: state.factory })
      .then((p) => live && setPresetNorm({ key, norm: normalize(p) }))
      .catch(() => live && setPresetNorm({ key, norm: null }));
    return () => {
      live = false;
    };
    // `normalize` only depends on the effect.
  }, [effect, key, rev]);

  const { label, edited } = presetBoxLabel(state, current, presetNorm?.key === key ? presetNorm.norm : null, defaultNorm, defaultName);

  const load = (name: string, factory: boolean) => act("load_preset", { track, slot, effect, name, factory });

  const save = async (name: string, overwrite: boolean): Promise<string | null> => {
    try {
      const res = await invoke<NamedPreset>("save_preset", { track, slot, name, overwrite });
      setState({ snap: res.snapshot });
      presetsChanged();
      return null;
    } catch (e) {
      return typeof e === "string" ? e : String(e);
    }
  };

  const step = async (dir: 1 | -1) => {
    const list = await call<PresetList>("list_presets", { effect });
    if (!list) return;
    const all = [...list.factory.map((n) => ({ n, f: true })), ...list.user.map((n) => ({ n, f: false }))];
    if (!all.length) return;
    const at = state ? all.findIndex((x) => x.n === state.name && x.f === state.factory) : -1;
    const next = all[(at + dir + all.length) % all.length];
    load(next.n, next.f);
  };

  const menu = async (el: HTMLElement) => {
    const list = (await call<PresetList>("list_presets", { effect })) ?? { factory: [], user: [] };
    const isUser = !!state && !state.factory;
    const items: MenuItem[] = [{ label: "Factory", header: true }];
    for (const n of list.factory.filter((n) => n !== defaultName)) items.push({ label: n, checked: state?.factory && state.name === n, onClick: () => load(n, true) });
    items.push({ separator: true }, { label: "User", header: true });
    if (!list.user.length) items.push({ label: "No User Presets", disabled: true });
    for (const n of list.user) items.push({ label: n, checked: isUser && state?.name === n, onClick: () => load(n, false) });
    items.push(
      { separator: true },
      {
        label: "Save Preset…",
        // Saving a user preset updates it; otherwise it needs a name.
        onClick: () => (isUser ? save(state!.name, true) : setSheet({ mode: "save", text: "" })),
      },
      { label: "Save Preset As…", onClick: () => setSheet({ mode: "save", text: state?.name ?? "" }) },
      { label: "Rename Preset…", disabled: !isUser, onClick: () => setSheet({ mode: "rename", text: state!.name }) },
      { label: "Delete Preset", disabled: !isUser, onClick: () => setSheet({ mode: "delete" }) },
      { separator: true },
      { label: state ? `Revert to “${state.name}”` : "Revert", disabled: !edited, onClick: () => state && load(state.name, state.factory) },
      { label: `Reset to “${defaultName}”`, onClick: () => load(defaultName, true) },
    );
    if (extra?.length) items.push({ separator: true }, ...extra);
    openMenuAt(el, items);
  };

  const submit = async () => {
    if (!sheet || sheet.mode === "delete") return;
    if (sheet.mode === "save") {
      const err = await save(sheet.text, !!sheet.replace);
      if (!err) setSheet(null);
      // A name already taken asks before replacing it.
      else setSheet({ ...sheet, error: err, replace: /already exists/.test(err) });
      return;
    }
    const renamed = await call<NamedPreset>("rename_preset", { effect, old: state!.name, new: sheet.text });
    if (renamed) {
      setState({ snap: renamed.snapshot });
      presetsChanged();
      setSheet(null);
    }
  };

  const remove = async () => {
    if (!state) return;
    if (await act("delete_preset", { effect, name: state.name })) presetsChanged();
    setSheet(null);
  };

  return (
    <span className="fx-presets">
      <button className="fx-step" title="Previous Preset" aria-label="Previous Preset" onClick={() => step(-1)}>
        ‹
      </button>
      <button className="fx-popup fx-preset" {...onPress((e) => menu(e.currentTarget))} title={`Presets: ${label}${edited ? " (edited)" : ""}`}>
        <span className="truncate">
          {label}
          {edited && <span className="dim"> (edited)</span>}
        </span>
      </button>
      <button className="fx-step" title="Next Preset" aria-label="Next Preset" onClick={() => step(1)}>
        ›
      </button>
      {sheet && (
        <div className="fx-sheet" onMouseDown={(e) => e.stopPropagation()}>
          {sheet.mode === "delete" ? (
            <>
              <div>Delete the preset “{state?.name}”? It is removed for every project.</div>
              <div className="fx-sheet-buttons">
                <button className="push" onClick={() => setSheet(null)}>
                  Cancel
                </button>
                <button className="push default" onClick={remove}>
                  Delete
                </button>
              </div>
            </>
          ) : (
            <>
              <div>{sheet.mode === "save" ? "Save the current settings as a preset named:" : "Rename the preset to:"}</div>
              <input
                className="fx-sheet-input"
                autoFocus
                value={sheet.text}
                onChange={(e) => setSheet({ ...sheet, text: e.target.value, error: undefined, replace: false })}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") submit();
                  if (e.key === "Escape") setSheet(null);
                }}
              />
              {sheet.error && <div className="fx-sheet-error">{sheet.replace ? `“${sheet.text.trim()}” already exists. Replace it?` : sheet.error}</div>}
              <div className="fx-sheet-buttons">
                <button className="push" onClick={() => setSheet(null)}>
                  Cancel
                </button>
                <button className="push default" disabled={!sheet.text.trim()} onClick={submit}>
                  {sheet.replace ? "Replace" : sheet.mode === "save" ? "Save" : "Rename"}
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </span>
  );
}
