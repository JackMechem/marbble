// A plug-in panel's content and header controls, wherever the panel shows: docked in the bottom
// section (FxDock), floating in a window of its own (FxWindows), or anywhere else that renders
// it. The header itself, which places these controls, is PanelHeader.tsx. Everything here works from the panel key alone (fxDockModel.ts), fills the box it is given and
// scrolls when the box is smaller than the panel.
import { useEffect, useRef, useState } from "react";
import { MASTER_ID, type Id, type Project } from "../../engine/project";
import { act, api, call } from "../../engine/commands";
import { FX_PANELS, type FxInfo, type FxPanelProps } from "../builtinEffects";
import { parseKey, type PanelKey } from "./fxDockModel";
import { valuesOf } from "../fxFormat";
import { CHANNEL_EQ, findFxSlot, fxInfo } from "../fxHost";
import { onTick, useStore } from "../../state/store";
import { FxControl, FxMeter, FxSideChain, isReductionMeter } from "./panelControls";
import { InstrumentPanel } from "../instruments/Instrument";
import { PresetButton, type Preset } from "./Presets";
import { FxLearnContext } from "../../controllers/learn";
import { CompareButton } from "./settings/CompareButton";
import { settingsMenuItems } from "./settings/settingsMenuModel";
import { copySettings, pasteSettings, rememberSaved } from "./settings/pluginSettings";

/** Values set from this panel within this long win over the tick's (which lag a little). */
const LOCAL_MS = 500;

const stripName = (project: Project | undefined, track: Id) => (track === MASTER_ID ? "Stereo Out" : project?.tracks.find((t) => t.id === track)?.name ?? "");

/**
 * What a panel is called: the effect and its strip, and the type its size is remembered under.
 * Null once the instance or strip is gone.
 */
export function usePanelTitle(key: PanelKey): { name: string; track: string; type: string } | null {
  const project = useStore((s) => s.snap?.project);
  useStore((s) => s.fxList);
  useStore((s) => s.instList);
  const ref = parseKey(key);
  if (!ref || !project) return null;
  const at = findFxSlot(project, ref.uid);
  const info = fxInfo(at?.plugin.plugin_id);
  if (!at || !info) return null;
  return { name: info.name, track: stripName(project, at.track), type: info.id };
}

/** Whether a panel's instance or strip still exists in `project`. */
export function panelExists(project: Project, key: PanelKey): boolean {
  const ref = parseKey(key);
  return !!ref && !!findFxSlot(project, ref.uid);
}

/** The panel itself: an effect's controls and meters. */
export function PanelContent({ panelKey }: { panelKey: PanelKey }) {
  const ref = parseKey(panelKey);
  return ref ? <FxPanelContent uid={ref.uid} /> : null;
}

/** The header control before the name (PanelHeader.tsx): the power button. */
export function PanelLead({ panelKey }: { panelKey: PanelKey }) {
  const ref = parseKey(panelKey);
  return ref ? <FxPower uid={ref.uid} /> : null;
}

/** The header controls after the name (PanelHeader.tsx): presets, Compare, and the side chain for
 *  effects with one. */
export function PanelTools({ panelKey }: { panelKey: PanelKey }) {
  const ref = parseKey(panelKey);
  return ref ? <FxTools uid={ref.uid} /> : null;
}

function useFx(uid: Id): { at: NonNullable<ReturnType<typeof findFxSlot>>; info: FxInfo } | null {
  const project = useStore((s) => s.snap?.project);
  useStore((s) => s.fxList);
  useStore((s) => s.instList);
  const at = findFxSlot(project, uid);
  const info = fxInfo(at?.plugin.plugin_id);
  return at && info ? { at, info } : null;
}

/** The power (bypass) button. */
function FxPower({ uid }: { uid: Id }) {
  const fx = useFx(uid);
  if (!fx) return null;
  const { at } = fx;
  const label = at.plugin.bypass ? "Turn On" : "Bypass";
  return (
    <button className={"fx-power" + (at.plugin.bypass ? "" : " on")} title={label} aria-label="Plug-in On" aria-pressed={!at.plugin.bypass} onClick={() => api.setBypass(at.track, at.slot, !at.plugin.bypass)}>
      ⏻
    </button>
  );
}

/** The preset pop-up (with Copy and Paste Settings), Compare and, for effects with a side-chain,
 *  the Side Chain pop-up. */
function FxTools({ uid }: { uid: Id }) {
  const fx = useFx(uid);
  const settingsClip = useStore((s) => s.settingsClip);
  if (!fx) return null;
  const { at, info } = fx;
  const norm = (vals: Record<number, number>) => JSON.stringify(info.params.map((p) => vals[p.id]));
  return (
    <>
      <PresetButton
        effect={info.id}
        track={at.track}
        slot={at.slot}
        loaded={at.plugin.preset}
        current={norm(valuesOf(info.params, at.plugin.params))}
        normalize={(p: Preset) => norm(valuesOf(info.params, p.values))}
        defaultName={info.id === CHANNEL_EQ ? "Flat" : undefined}
        extra={settingsMenuItems(info.id, settingsClip, () => copySettings(uid), () => pasteSettings(uid))}
        onStored={() => rememberSaved(uid)}
      />
      <CompareButton uid={uid} plugin={at.plugin} info={info} />
      {info.sidechain && <FxSideChain uid={uid} />}
    </>
  );
}

/**
 * An effect's panel from FX_PANELS (or the generic one), fed with its parameter values and
 * meters while it is on screen.
 */
export function FxPanelContent({ uid }: { uid: Id }) {
  const fx = useFx(uid);
  const [meters, setMeters] = useState<number[]>([]);
  const [live, setLive] = useState<Record<number, number>>({});
  const local = useRef(new Map<number, { v: number; at: number }>());

  useEffect(() => {
    call("watch_builtin", { uid, on: true });
    const off = onTick((t) => {
      const f = t.fx?.find((x) => x.uid === uid);
      if (!f) return;
      setMeters(f.meters);
      setLive(Object.fromEntries(f.values));
    });
    return () => {
      off();
      call("watch_builtin", { uid, on: false });
    };
  }, [uid]);

  if (!fx) return null;
  const { at, info } = fx;
  const track = at.track;
  const saved = valuesOf(info.params, at.plugin.params);
  // What the effect runs with (automation included), except what was just set here.
  const now = performance.now();
  const values: Record<number, number> = {};
  for (const p of info.params) {
    const l = local.current.get(p.id);
    values[p.id] = l && now - l.at < LOCAL_MS ? l.v : live[p.id] ?? saved[p.id];
  }

  const set = (id: number, value: number) => {
    local.current.set(id, { v: value, at: performance.now() });
    setLive((cur) => ({ ...cur, [id]: value }));
    act("set_builtin_param", { track, slot: at.slot, id, value });
  };
  const setMany = (values: [number, number][]) => {
    const now = performance.now();
    for (const [id, v] of values) local.current.set(id, { v, at: now });
    setLive((cur) => ({ ...cur, ...Object.fromEntries(values) }));
    act("set_builtin_params", { track, slot: at.slot, values });
  };
  const touch = (id: number, on: boolean) => {
    // Stereo Out isn't automated.
    if (track === MASTER_ID) return;
    const spec = info.params.find((p) => p.id === id);
    act("automation_touch", { track, param: { type: "plugin", value: { uid, param: id, name: `${at.plugin.name}: ${spec?.name ?? id}` } }, touching: on });
  };
  const props: FxPanelProps = { info, track, uid, values, set, touch, meters, setMany };
  const Panel = FX_PANELS[info.id] ?? (info.sections?.length ? InstrumentPanel : GenericPanel);
  return (
    <div className={"fx-body" + (at.plugin.bypass ? " bypassed" : "")}>
      {/* Lets every control offer Learn MIDI Assignment for its parameter. */}
      <FxLearnContext.Provider value={{ track, uid }}>
        <Panel {...props} />
      </FxLearnContext.Provider>
    </div>
  );
}

/** The panel for effects without a custom one: a control per parameter and a bar per meter. */
function GenericPanel({ info, values, set, touch, meters }: FxPanelProps) {
  const knobs = info.params.filter((p) => p.kind.type === "continuous");
  const switches = info.params.filter((p) => p.kind.type !== "continuous");
  return (
    <div className="fx-generic">
      <div className="fx-controls">
        <div className="fx-knobs">
          {knobs.map((p) => (
            <FxControl key={p.id} spec={p} value={values[p.id]} onChange={(v) => set(p.id, v)} onTouch={(on) => touch(p.id, on)} />
          ))}
        </div>
        {switches.length > 0 && (
          <div className="fx-switches">
            {switches.map((p) => (
              <FxControl key={p.id} spec={p} value={values[p.id]} onChange={(v) => set(p.id, v)} />
            ))}
          </div>
        )}
      </div>
      {info.meters.length > 0 && (
        <div className="fx-meters">
          {info.meters.map((m, i) => (
            <FxMeter key={m} label={m} value={meters[i] ?? NaN} kind={isReductionMeter(m) ? "reduction" : "level"} />
          ))}
        </div>
      )}
    </div>
  );
}
