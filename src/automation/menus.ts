// Choosing what the automation view shows: the parameter and mode menus, extra lanes, Show Used Automation.
import { act, BUILTIN_SYNTH, call, type AutoParam, type Id, type ParamInfo, type Track } from "../api";
import { nextParam, sameParam, usedLayout } from "../autoLanes";
import type { MenuItem } from "../components/Menu";
import { getState, setState } from "../store";
import { baseParams, isOutput, laneOf, outputTrack, paramLabel, stripOf, MODE_LABELS, MODES, VOLUME } from "./params";
import { shownParam } from "./values";

const paramCache = new Map<Id, Promise<ParamInfo[]>>();

/** A plug-in instance's automatable parameters (fetched once per instance). */
export function pluginParams(uid: Id): Promise<ParamInfo[]> {
  let p = paramCache.get(uid);
  if (!p) {
    p = call<ParamInfo[]>("list_plugin_params", { uid }).then((r) => r ?? []);
    paramCache.set(uid, p);
  }
  return p;
}

function showParam(t: Track, p: AutoParam) {
  setState({ autoLane: { ...getState().autoLane, [t.id]: p } });
}

function setSubLanes(t: Track, lanes: AutoParam[]) {
  act("set_auto_lanes", { lanes: [[t.id, lanes]] });
}

/** Shows another lane below the track's lanes (the "+" on a lane header): the next lane with
 *  automation that isn't shown, else the next of volume, pan, mute and sends. */
export function addLane(t: Track) {
  const subs = t.auto_lanes ?? [];
  const p = nextParam([shownParam(t), ...subs], t.automation, baseParams(t));
  if (p) setSubLanes(t, [...subs, p]);
}

/** Closes sub-lane `k`. */
export function closeLane(t: Track, k: number) {
  setSubLanes(t, (t.auto_lanes ?? []).filter((_, i) => i !== k));
}

/** Show Used Automation: every lane holding points on the selected tracks (all tracks when
 *  none is selected) gets shown, the main lane first. Turns the automation view on. */
export function showUsedAutomation() {
  const s = getState();
  const project = s.snap?.project;
  if (!project) return;
  const sel = s.selectedTracks.length ? s.selectedTracks : s.snap?.selected != null ? [s.snap.selected] : project.tracks.map((t) => t.id);
  const strips = sel.map((id) => stripOf(project, id)).filter((t): t is Track => !!t);
  if (s.showOutput && !strips.some(isOutput)) strips.push(outputTrack(project));
  const autoLane = { ...s.autoLane };
  const lanes: [Id, AutoParam[]][] = [];
  for (const t of strips) {
    const { main, subs } = usedLayout(t.automation, shownParam(t));
    autoLane[t.id] = main;
    lanes.push([t.id, subs]);
  }
  setState({ autoLane, showAutomation: true });
  act("set_auto_lanes", { lanes });
}

/** The parameter picker: Volume, Pan, Mute and the parameters of the track's plug-ins. Lanes
 *  that hold automation are marked. `sub` picks for that sub-lane instead of the main lane. */
export async function paramMenu(t: Track, sub = -1): Promise<MenuItem[]> {
  const cur = sub < 0 ? shownParam(t) : (t.auto_lanes?.[sub] ?? VOLUME);
  const pick = (p: AutoParam) => {
    if (sub < 0) return showParam(t, p);
    const lanes = [...(t.auto_lanes ?? [])];
    lanes[sub] = p;
    setSubLanes(t, lanes);
  };
  const item = (p: AutoParam, label = paramLabel(p)): MenuItem => ({
    label,
    checked: sameParam(cur, p),
    hint: laneOf(t, p)?.points.length ? "●" : undefined,
    onClick: () => pick(p),
  });
  const project = getState().snap?.project;
  const items: MenuItem[] = baseParams(t).map((p) => {
    if (p.type !== "send") return item(p);
    const aux = project?.tracks.find((x) => x.id === t.sends[p.value]?.bus);
    return item(p, `Send ${p.value + 1} → ${aux?.name ?? "Bus"}`);
  });
  // The built-in synth has no parameters; built-in effects list theirs.
  const slots = [t.instrument, ...t.inserts].filter((s) => s && s.plugin_id !== BUILTIN_SYNTH);
  for (const s of slots) {
    const params = await pluginParams(s!.uid);
    if (!params.length) continue;
    const uid = s!.uid;
    items.push({
      label: s!.name,
      hint: t.automation.some((l) => l.param.type === "plugin" && l.param.value.uid === uid && l.points.length) ? "●" : undefined,
      children: params.map((pi) => item({ type: "plugin", value: { uid, param: pi.id, name: `${s!.name}: ${pi.name}` } }, pi.name)),
    });
  }
  items.push(
    { separator: true },
    { label: `Delete ${paramLabel(cur)} Automation`, disabled: !laneOf(t, cur)?.points.length, onClick: () => act("clear_automation", { track: t.id, param: cur }) },
    { label: "Delete All Automation on Track", disabled: !t.automation.some((l) => l.points.length), onClick: () => act("clear_automation", { track: t.id, param: null }) },
  );
  return items;
}

export function modeMenu(t: Track): MenuItem[] {
  return MODES.map((m) => ({ label: MODE_LABELS[m], checked: t.auto_mode === m, onClick: () => act("set_auto_mode", { track: t.id, mode: m }) }));
}
