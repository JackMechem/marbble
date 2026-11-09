// A mixer channel strip, and the plug-in slot it shows for each insert.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { act, actMade, api, canRecord, INSTRUMENT_SLOT, isStack, MASTER_ID, SEND_SLOTS, type Id, type InputSel, type PluginSlot, type Track } from "../api";
import { Icon } from "../icons";
import { pluginMenu } from "../pluginMenu";
import { getState, onTick, peakHold, useStore } from "../store";
import { formatDb, inputLabel } from "../time";
import { Fader, GrMeter, Knob, Meter } from "./Controls";
import { EqThumb } from "./Eq";
import { automatable, outputTrack, useAutomation, VOLUME } from "../automation";
import { AutoModeButton } from "./AutomationHeader";
import { openMenu, openMenuAt, type MenuItem } from "./Menu";
import { OutputSlot, SendsSection } from "./Sends";
import { StripLatencyBadge, useSlotLatency } from "./Latency";
import { refuseFrozen } from "../freeze";
import { fxInfo, isBuiltinFx, loadPlugin, slotMenu, slotSideChainItems, toggleFxPanel } from "../fxHost";
import { keyName } from "../sideChain";
import { startSlotDrag } from "../slotDrag";
import { fxKey, usePanelShown } from "../fxDockState";
import { clipResetTargets, isClip, peakText, reducesGain } from "../stripMeters";
import { resetClipIndicators, stripSettingMenu } from "../stripSettings";
import { fmt } from "../keymap";
import { stripLearnMenu } from "../learn";
import { canAssignVca, vcaOffset } from "../vca";
import { isTrackOff, powerTitle } from "../trackOff";
import { auxInput, hasExtraOutputs, nextOutput } from "../multiOut";
import { keepRegionsOn } from "../ops";
import { useVcas, VcaSlot } from "./Vca";
import { slotName } from "../slotName";
import { slotProps } from "../slotKeys";
import { BlankButtons, BlankEq, BlankRow, BlankSlots, MoreSlot, PlateName, SlotBox } from "./StripSlots";
import { TrackIcon, TrackIconArt } from "../trackIconArt";
import { openTrackIconPicker } from "./TrackIconPicker";
import { slotLayout, stripRows, stripWants, type SlotRows } from "../stripRows";
import { METER_W, plateInk } from "../mixerLayout";
import { onPress } from "../keyNav";

const PAN = { type: "pan" } as const;
/** Stereo Out's name plate. */
const MASTER_PLATE = "#8d9099";

/** Opens a slot's plug-in: a built-in effect's panel, a VST3's editor. A frozen track's plug-ins
 *  are unloaded, and a missing VST3 or a built-in without a panel has nothing to open. */
function openSlot(trackId: Id, plugin: PluginSlot, title: string, missing: boolean) {
  if (isBuiltinFx(plugin.plugin_id)) {
    if (!refuseFrozen([trackId])) toggleFxPanel(plugin.uid);
    return;
  }
  if (missing || plugin.plugin_id.startsWith("builtin:") || refuseFrozen([trackId])) return;
  api.openEditor(plugin.uid, `${title} — ${plugin.name}`);
}

/** The menu behind an Audio FX "+N more": each plug-in in `slots` with what its slot offers,
 *  then a plug-in to add in the first free slot. */
function moreFxMenu(trackId: Id, inserts: (PluginSlot | null)[], slots: number[], title: string): MenuItem[] {
  const missing = getState().snap?.missing ?? [];
  const items: MenuItem[] = slots.map((i) => {
    const p = inserts[i]!;
    const change = (f: () => void) => () => {
      if (!refuseFrozen([trackId])) f();
    };
    return {
      label: slotName(p.plugin_id, p.name),
      hint: p.bypass ? "Off" : undefined,
      children: [
        { label: "Open", onClick: () => openSlot(trackId, p, title, missing.includes(p.uid)) },
        { label: "Bypass", checked: p.bypass, onClick: change(() => api.setBypass(trackId, i, !p.bypass)) },
        { separator: true },
        { label: "Remove", onClick: change(() => api.removePlugin(trackId, i)) },
      ],
    };
  });
  const free = inserts.findIndex((p) => !p);
  if (free >= 0) {
    // The slot menu's list without its "No Plug-in" and the line under it.
    const add = pluginMenu(getState().plugins, { instrument: false, pick: (id) => loadPlugin(trackId, free, id), remove: () => {} }).slice(2);
    items.push({ separator: true }, { label: "Add Plug-in", children: add });
  }
  return items;
}

export function PluginSlotView({
  trackId,
  slot,
  plugin,
  instrument,
  title,
}: {
  trackId: number;
  slot: number;
  plugin: PluginSlot | null;
  instrument?: boolean;
  title: string;
}) {
  const plugins = useStore((s) => s.plugins);
  const openEditors = useStore((s) => s.snap?.open_editors) ?? [];
  const fxOpen = usePanelShown(plugin ? fxKey(plugin.uid) : null);
  useStore((s) => s.fxList);
  useStore((s) => s.instList);
  // A frozen track's plug-ins are unloaded and can't change until it is unfrozen.
  const frozen = useStore((s) => !!s.snap?.project.tracks.find((t) => t.id === trackId)?.frozen);
  const latency = useSlotLatency(plugin?.uid);
  // A VST3 that isn't installed here: the slot keeps its settings for where it is.
  const missing = useStore((s) => !!plugin && !!s.snap?.missing?.includes(plugin.uid));
  const rate = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  // A VST3 keyed by another track: built-in effects show theirs in the panel.
  const keyedBy = useStore((s) => (plugin && s.snap?.key_inputs?.includes(plugin.uid) ? keyName(s.snap.project.tracks, plugin) : null));
  const remove = () => api.removePlugin(trackId, slot);
  // The plug-in menu, beside the slot `el` is in.
  const menuBy = (el: Element) => {
    if (refuseFrozen([trackId])) return;
    const items = pluginMenu(plugins, {
      instrument: !!instrument,
      current: plugin?.plugin_id,
      pick: (id) => loadPlugin(trackId, slot, id),
      remove,
    });
    const r = el.closest(".slot")!.getBoundingClientRect();
    openMenu(r.right - 4, r.top, [...slotSideChainItems(trackId, slot, plugin), ...items]);
  };
  const menu = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (e.button === 0) menuBy(e.currentTarget);
  };
  // Right-click: Copy and Paste Plug-in.
  const contextAt = (x: number, y: number) => {
    if (!refuseFrozen([trackId])) openMenu(x, y, slotMenu(trackId, slot, plugin));
  };
  const context = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    contextAt(e.clientX, e.clientY);
  };
  // From the keyboard (slotKeys.ts): the menus open at the slot, Delete empties it as No Plug-in does.
  const keyed = (open: (el: HTMLElement) => void) =>
    slotProps(
      `${instrument ? "Instrument" : `Audio FX ${slot + 1}`}: ${plugin ? slotName(plugin.plugin_id, plugin.name) : "empty"}`,
      {
        open,
        menu: menuBy,
        context: (el) => {
          const r = el.getBoundingClientRect();
          contextAt(r.left, r.bottom + 2);
        },
        remove: plugin ? () => !refuseFrozen([trackId]) && remove() : undefined,
      },
      { group: !!plugin, popup: !plugin },
    );
  // Audio FX slots are drop targets for dragged plug-ins (see slotDrag.ts).
  const target = instrument ? {} : { "data-track": trackId, "data-slot": slot };
  if (!plugin) {
    return (
      <div className={"slot empty" + (instrument ? " instrument" : "")} {...target} {...keyed(menuBy)} onMouseDown={menu} onContextMenu={context} title={instrument ? "Choose an instrument" : "Add an Audio FX plug-in"}>
        {instrument ? <span className="slot-name dim">Instrument</span> : <span className="slot-add" />}
      </div>
    );
  }
  const open = openEditors.includes(plugin.uid) || fxOpen;
  return (
    <div
      className={"slot filled" + (plugin.bypass ? " bypassed" : "") + (open ? " editing" : "") + (instrument ? " instrument" : "") + (frozen ? " frozen" : "") + (missing ? " missing" : "") + (keyedBy ? " keyed" : "")}
      {...target}
      {...keyed(() => openSlot(trackId, plugin, title, missing))}
      onContextMenu={context}
    >
      <button
        className="slot-power"
        title={plugin.bypass ? "Turn on" : "Bypass"}
        aria-label="Plug-in On"
        aria-pressed={!plugin.bypass}
        {...onPress((e) => {
          e.stopPropagation();
          if (!refuseFrozen([trackId])) api.setBypass(trackId, slot, !plugin.bypass);
        })}
      >
        <Icon.power />
      </button>
      <span
        className="slot-name"
        title={missing ? `${plugin.name} — not installed on this computer; its settings are kept with the project` : `${plugin.name}${keyedBy ? ` — side chain: ${keyedBy}` : ""}${latency ? ` — latency ${latency} samples (${((latency * 1000) / rate).toFixed(1)} ms)` : ""}${frozen ? " — frozen" : instrument ? " — click to open" : " — click to open, drag to move (Alt to copy)"}`}
        onMouseDown={(e) => {
          e.stopPropagation();
          if (e.button !== 0) return;
          const open = () => openSlot(trackId, plugin, title, missing);
          if (instrument || frozen) open();
          else startSlotDrag(e, trackId, slot, slotName(plugin.plugin_id, plugin.name), open);
        }}
      >
        {slotName(plugin.plugin_id, plugin.name)}
      </span>
      <button className="slot-arrow" tabIndex={-1} onMouseDown={menu} aria-label="Plug-in menu">
        <Icon.chevron />
      </button>
    </div>
  );
}

/** An audio track's Input menu (the strip's input slot and the Track inspector). */
export function inputMenu(track: Track, channels: number): MenuItem[] {
  const n = Math.max(channels, 2);
  const set = (input: InputSel) => api.updateTrack(track.id, { input });
  const items: MenuItem[] = [{ label: "No Input", checked: track.input.type === "none", onClick: () => set({ type: "none" }) }, { separator: true }];
  for (let c = 0; c < n; c++) {
    items.push({ label: `Input ${c + 1}`, checked: track.input.type === "mono" && track.input.ch === c, onClick: () => set({ type: "mono", ch: c }) });
  }
  items.push({ separator: true });
  for (let c = 0; c + 1 < n; c += 2) {
    items.push({
      label: `Input ${c + 1}–${c + 2}`,
      checked: track.input.type === "stereo" && track.input.ch === c,
      onClick: () => set({ type: "stereo", ch: c }),
    });
  }
  return items;
}

/** A strip's clip indicator: its highest peak since reset, held by the engine (meter key). */
function usePeakHold(key: Id): number {
  const [peak, setPeak] = useState(() => peakHold(key));
  useEffect(() => {
    const off = onTick(() => setPeak(peakHold(key)));
    return () => {
      off();
    };
  }, [key]);
  return peak;
}

/**
 * Logic-style channel strip. `track` is null for the Stereo Out strip. `rows` are the insert and
 * send rows shared by the strips beside it (stripRows.ts); without them it sizes to its own.
 */
export function ChannelStrip({ track, selected, rows }: { track: Track | null; selected?: boolean; rows?: SlotRows }) {
  // The fader takes whatever height is left, like Logic's strips.
  const faderBox = useRef<HTMLDivElement>(null);
  const [faderHeight, setFaderHeight] = useState(160);
  useLayoutEffect(() => {
    const el = faderBox.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setFaderHeight(Math.max(60, Math.floor(e.contentRect.height))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const master = useStore((s) => s.snap?.project.master);
  const inputs = useStore((s) => s.snap?.audio.input_channels ?? 2);
  const isMaster = track === null;
  const id = track?.id ?? MASTER_ID;
  const peak = usePeakHold(isMaster ? -1 : id);
  useStore((s) => s.fxList);
  const inserts = isMaster ? master?.inserts ?? [] : track.inserts;
  const volume = isMaster ? master?.volume_db ?? 0 : track.volume_db;
  const wants = stripWants(isMaster ? null : track.kind, inserts, isMaster ? undefined : track.sends, SEND_SLOTS);
  const rowsHere = rows ?? stripRows([wants]);
  // Every strip shows the shared rows (empty slots below its plug-ins); past them, "+N more".
  const fx = slotLayout(inserts, rowsHere.inserts, inserts.length);
  const name = isMaster ? "Stereo Out" : track.name;
  // A folder stack has no audio of its own: its strip is a fader, its buttons and a name, with
  // blank rows where the others have slots so its fader lines up with theirs.
  const folder = track?.kind === "folder";
  const summing = track?.kind === "summing";
  const tracks = useStore((s) => s.snap?.project.tracks);
  const setVolume = (db: number) => (isMaster ? act("set_master_volume", { db }) : api.updateTrack(id, { volume_db: db }));
  // While playing, the fader and pan knob follow the automation.
  // Stereo Out's fader follows its lane on the Output track.
  const project = useStore((s) => s.snap?.project);
  const strip = track ?? (project ? outputTrack(project) : null);
  const off = !!track && isTrackOff(tracks ?? [], track);
  const instrumentOutputs = useStore((s) => s.snap?.instrument_outputs);
  const extraOutputs = !!track && track.kind === "instrument" && hasExtraOutputs(instrumentOutputs, track.id);
  const nextOut = extraOutputs ? nextOutput(instrumentOutputs, tracks ?? [], id) : null;
  const auxIn = track?.kind === "aux" ? auxInput(track, tracks ?? [], instrumentOutputs) : null;
  const [autoVolume, volumeTouch] = useAutomation(strip, VOLUME, volume);
  const [autoPan, panTouch] = useAutomation(track, PAN, track?.pan ?? 0);
  // Like Logic, a strip with a dynamics effect shows its gain reduction beside the meter.
  const dynamics = !folder && !track?.frozen && inserts.some((p) => !!p && reducesGain(fxInfo(p.plugin_id)?.meters ?? []));
  // A strip on a VCA shows where it really plays on its fader.
  const vcas = useVcas();
  const vca = track && canAssignVca(track.kind) && track.vca != null ? vcaOffset(vcas, track.vca).db : undefined;
  // The name plate is filled with the track's colour, as in Logic; Stereo Out's is grey.
  const plate = isMaster ? MASTER_PLATE : track.color;
  const settingMenu = async (x: number, y: number, minWidth?: number) => {
    if (folder) return;
    openMenu(x, y, await stripSettingMenu(id, name), minWidth);
  };
  // The Setting slot's menu, under it.
  const settingUnder = (el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    settingMenu(r.left, r.bottom + 2, r.width);
  };

  return (
    <div className={"strip" + (folder ? " folder" : summing ? " summing" : "") + (selected ? " selected" : "") + (off ? " is-off" : "")} onMouseDown={() => {
        if (!track || selected) return;
        keepRegionsOn([track.id]);
        api.selectTrack(track.id);
      }} onContextMenu={(e) => stripLearnMenu(e, isMaster ? null : id)}>
      {folder ? (
        <BlankRow />
      ) : (
        <div className="strip-section">
          <div
            className="slot static output strip-setting"
            title="Setting: copy, paste, reset, save and load the channel strip's settings"
            {...slotProps("Channel strip setting", { open: settingUnder }, { popup: true })}
            onMouseDown={(e) => {
              e.stopPropagation();
              if (e.button === 0) settingUnder(e.currentTarget);
            }}
          >
            <span className="slot-text">Setting</span>
          </div>
        </div>
      )}

      {isMaster || folder ? (
        // Stereo Out and folders have no input: the row stays, empty, so the strips line up.
        <BlankRow label />
      ) : (
        <div className="strip-section">
          <div className="strip-label">Input</div>
          {isStack(track.kind) ? (
            <div className="slot static" title="Σ Subtracks: this summing stack's subtracks play through its strip">
              <span className="slot-text">Σ Subtracks</span>
            </div>
          ) : track.kind === "aux" ? (
            <div className={"slot static output" + (auxIn?.silent ? " no-output" : "")} title={auxIn?.title}>
              <span className="aux-input">{auxIn?.label}</span>
            </div>
          ) : track.kind === "instrument" ? (
            <PluginSlotView trackId={id} slot={INSTRUMENT_SLOT} plugin={track.instrument} instrument title={name} />
          ) : (
            <div className="slot input-slot" {...slotProps(`Input: ${inputLabel(track.input)}`, { open: (el) => openMenuAt(el, inputMenu(track, inputs)) }, { group: true })}>
              <button
                className="slot-format"
                title="Mono / Stereo"
                aria-label={track.input.type === "stereo" ? "Stereo input" : "Mono input"}
                {...onPress((e) => {
                  e.stopPropagation();
                  const ch = track.input.type === "none" ? 0 : track.input.ch;
                  api.updateTrack(id, { input: track.input.type === "stereo" ? { type: "mono", ch } : { type: "stereo", ch: ch - (ch % 2) } });
                })}
              >
                {track.input.type === "stereo" ? <Icon.stereo /> : <Icon.mono />}
              </button>
              <span
                className="slot-name"
                title={`Input: ${inputLabel(track.input)}`}
                onMouseDown={(e) => {
                  e.stopPropagation();
                  openMenuAt(e.currentTarget.parentElement as HTMLElement, inputMenu(track, inputs));
                }}
              >
                {inputLabel(track.input)}
              </span>
            </div>
          )}
        </div>
      )}

      {folder ? (
        <div className="strip-folder-blank" title="A folder stack: its fader, mute and solo act on its subtracks">
          <BlankEq />
          <BlankSlots rows={rowsHere.inserts} />
          <BlankSlots rows={rowsHere.sends} />
          <div className="strip-folder-note">Folder</div>
        </div>
      ) : (
        <>
          <div className="strip-section">
            <EqThumb trackId={id} name={name} />
          </div>

          <div className="strip-section">
            {/* The latency comes first so a narrow strip can drop "Audio FX" for it (CSS puts it after). */}
            <div className="strip-label with-badge">
              <StripLatencyBadge id={id} />
              <span className="strip-label-text">Audio FX</span>
            </div>
            <SlotBox rows={rowsHere.inserts}>
              {inserts.slice(0, fx.shown).map((p, i) => (
                <PluginSlotView key={i} trackId={id} slot={i} plugin={p} title={name} />
              ))}
              {fx.more.length > 0 && <MoreSlot n={fx.more.length} shown={fx.shown} noun={["plug-in", "plug-ins"]} menu={() => moreFxMenu(id, inserts, fx.more, name)} />}
            </SlotBox>
          </div>

          {isMaster ? <BlankSlots rows={rowsHere.sends} /> : <SendsSection track={track} rows={rowsHere.sends} />}
        </>
      )}

      {folder ? (
        <BlankRow />
      ) : (
        <div className="strip-section">
          {isMaster ? (
            <div className="slot static output" title="Output 1–2">
              <span className="slot-text">Output 1–2</span>
            </div>
          ) : (
            <OutputSlot track={track} />
          )}
        </div>
      )}

      {track && canAssignVca(track.kind) ? (
        <div className="strip-section">
          <VcaSlot track={track} />
        </div>
      ) : folder ? (
        <BlankRow />
      ) : (
        <div className="strip-section">
          <div className="slot static" />
        </div>
      )}

      <div className="strip-pan">
        {/* Logic puts the automation mode right above the pan knob. */}
        {strip && automatable(strip) ? <AutoModeButton track={strip} /> : <div className="auto-mode-spacer" />}
        {isMaster || folder ? <div style={{ height: 26 }} /> : <Knob value={autoPan} onChange={(pan) => api.updateTrack(id, { pan })} {...panTouch} />}
      </div>

      <div className="strip-readouts">
        <span className="readout">{formatDb(autoVolume)}</span>
        <span
          className={"readout peak" + (isClip(peak) ? " clip" : "")}
          title={`Peak level since reset · click to reset, ${fmt("Alt")}-click to reset all`}
          onMouseDown={(e) => {
            e.stopPropagation();
            resetClipIndicators(clipResetTargets(id, e.altKey));
          }}
        >
          {peakText(peak)}
        </span>
      </div>

      <div className="strip-fader" ref={faderBox}>
        <Fader db={autoVolume} onChange={setVolume} height={faderHeight} vca={vca} {...(isMaster ? {} : volumeTouch)} />
        {folder ? null : (
          <div className="strip-meters">
            <Meter source={isMaster ? { kind: "master" } : { kind: "track", id }} height={faderHeight} width={METER_W} />
            {dynamics && <GrMeter source={isMaster ? { kind: "master" } : { kind: "track", id }} height={faderHeight} />}
          </div>
        )}
      </div>

      {!isMaster ? (
        <div className="strip-buttons">
          <button className={"tbtn power" + (track.off ? " on" : "")} title={powerTitle(tracks ?? [], track)} aria-label="Track On" aria-pressed={!track.off} {...onPress((e) => { e.stopPropagation(); api.updateTrack(id, { off: !track.off }); })}>
            <Icon.power />
          </button>
          <div className="btn-row">
            <button className={"tbtn mute" + (track.mute ? " on" : "")} aria-label="Mute" aria-pressed={track.mute} {...onPress((e) => { e.stopPropagation(); api.updateTrack(id, { mute: !track.mute }); })}>
              M
            </button>
            <button className={"tbtn solo" + (track.solo ? " on" : "")} aria-label="Solo" aria-pressed={track.solo} {...onPress((e) => { e.stopPropagation(); api.updateTrack(id, { solo: !track.solo }); })}>
              S
            </button>
          </div>
          <div className="btn-row">
            {!canRecord(track.kind) ? (
              <span className="tbtn ghost" />
            ) : (
              <button className={"tbtn rec" + (track.armed ? " on" : "")} aria-label="Record Enable" aria-pressed={track.armed} {...onPress((e) => { e.stopPropagation(); api.updateTrack(id, { armed: !track.armed }); })}>
                R
              </button>
            )}
            {track.kind === "audio" ? (
              <button className={"tbtn mon" + (track.monitor ? " on" : "")} aria-label="Input Monitoring" aria-pressed={track.monitor} {...onPress((e) => { e.stopPropagation(); api.updateTrack(id, { monitor: !track.monitor }); })}>
                I
              </button>
            ) : extraOutputs ? (
              <button
                className="tbtn add-output"
                disabled={!nextOut}
                title={nextOut ? `New aux for this instrument's next output: ${nextOut.name}` : "Every output of this instrument has an aux"}
                aria-label={nextOut ? `New aux for this instrument's next output: ${nextOut.name}` : "Every output of this instrument has an aux"}
                {...onPress((e) => {
                  e.stopPropagation();
                  if (e.button === 0 && nextOut) actMade("add_output_aux", { track: id });
                })}
              >
                +
              </button>
            ) : (
              <span className="tbtn ghost" />
            )}
          </div>
        </div>
      ) : (
        <BlankButtons />
      )}

      <div
        className={"strip-name with-icon ink-" + plateInk(plate)}
        style={{ "--plate": plate } as React.CSSProperties}
        title={summing ? `${name} (summing stack)` : folder ? `${name} (folder stack)` : name}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          settingMenu(e.clientX, e.clientY);
        }}
      >
        {/* The track's icon by its name, as on its header; Stereo Out's is the output's. */}
        {isMaster ? (
          <span className="strip-icon">
            <TrackIconArt id="speaker" size={14} />
          </span>
        ) : (
          <button
            type="button"
            className="strip-icon"
            aria-label="Track Icon"
            aria-haspopup="menu"
            title="Choose an icon"
            onMouseDown={(e) => e.preventDefault()}
            onClick={(e) => openTrackIconPicker(e.currentTarget, track)}
          >
            <TrackIcon track={track} size={14} />
          </button>
        )}
        {summing && (
          <span className="strip-sum" title="Summing stack">
            Σ
          </span>
        )}
        <PlateName name={name} />
      </div>
    </div>
  );
}
