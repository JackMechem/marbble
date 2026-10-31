// An automation lane's values that take the store to read: the lane a track shows, a value in its own
// units (a built-in effect's come from its spec), and what an empty lane stands for.
import { type AutoParam, type Id, type Track } from "../api";
import { getState } from "../store";
import { formatDb } from "../time";
import type { FxParamSpec } from "../fx";
import { formatValue as fxFormat, fromNorm, toNorm } from "../fxFormat";
import { VOLUME } from "./params";

/** The lane a track shows in the automation view. */
export function shownParam(t: Track): AutoParam {
  return getState().autoLane[t.id] ?? VOLUME;
}

export function formatValue(p: AutoParam, v: number): string {
  switch (p.type) {
    case "volume":
    case "send":
      return `${formatDb(v)} dB`;
    case "pan": {
      const n = Math.round(v * 64);
      return n === 0 ? "Center" : n < 0 ? `${-n} L` : `${n} R`;
    }
    case "mute":
      return v >= 0.5 ? "Muted" : "On";
    case "plugin": {
      // A built-in effect's lane reads in the parameter's own units.
      const spec = builtinSpec(p.value.uid, p.value.param);
      return spec ? fxFormat(spec, fromNorm(spec, v)) : `${Math.round(v * 100)}%`;
    }
  }
}

function builtinSpec(uid: Id, param: number): FxParamSpec | undefined {
  const st = getState();
  const project = st.snap?.project;
  const slot = [...(project?.tracks.flatMap((t) => [t.instrument, ...t.inserts]) ?? []), ...(project?.master.inserts ?? [])].find((s) => s?.uid === uid);
  const specs = st.fxList.find((f) => f.id === slot?.plugin_id) ?? st.instList.find((f) => f.id === slot?.plugin_id);
  return specs?.params.find((p: FxParamSpec) => p.id === param);
}

/** The control's own value, what an empty lane stands for. */
export function liveValue(t: Track, p: AutoParam): number {
  switch (p.type) {
    case "volume":
      return t.volume_db;
    case "pan":
      return t.pan;
    case "mute":
      return t.mute ? 1 : 0;
    case "send":
      return t.sends?.[p.value]?.level_db ?? 0;
    case "plugin": {
      // A built-in effect's control is known here; a VST3's sits in its editor.
      const spec = builtinSpec(p.value.uid, p.value.param);
      const saved = t.inserts.find((s) => s?.uid === p.value.uid)?.params?.find((x) => x[0] === p.value.param)?.[1];
      return spec ? toNorm(spec, saved ?? spec.default) : 0.5;
    }
  }
}
