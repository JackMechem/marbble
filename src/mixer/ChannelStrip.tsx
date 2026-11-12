// A mixer channel strip: the setting, input, Channel EQ, plug-in and send slots, the output, pan,
// fader and meter, the buttons and the name plate. The plug-in slots are in PluginSlot.tsx.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { canRecord, INSTRUMENT_SLOT, isStack, MASTER_ID, SEND_SLOTS, type Id, type InputSel, type Track } from "../engine/project";
import { act, actMade, api } from "../engine/commands";
import { Icon } from "../ui/icons";
import { onTick, peakHold, STEREO_OUT_METER, useStore } from "../state/store";
import { formatDb } from "../lib/decibels";
import { inputLabel } from "../engine/project";
import { Fader, GrMeter, Knob, Meter, type MeterSource, type TouchProps } from "../ui/Controls";
import { EqThumb } from "./EqThumb";
import { automatable, outputTrack, VOLUME } from "../tracks/automation/params";
import { useAutomation } from "../tracks/automation/useAutomation";
import { AutoModeButton } from "../tracks/headers/AutomationHeader";
import { openMenu, openMenuAt, type MenuItem } from "../ui/Menu";
import { OutputSlot, SendsSection } from "./Sends";
import { StripLatencyBadge } from "./Latency";
import { fxInfo } from "../plugins/fxHost";
import { clipResetTargets, isClip, peakText, reducesGain } from "./stripMeters";
import { resetClipIndicators, stripSettingMenu } from "./stripSettings";
import { fmt } from "../keyboard/keymap";
import { stripLearnMenu } from "../controllers/learn";
import { canAssignVca, vcaOffset } from "./vcaModel";
import { isTrackOff, powerTitle } from "../tracks/trackOff";
import { auxInput, hasExtraOutputs, nextOutput } from "./multiOut";
import { keepRegionsOn } from "../tracks/trackCommands";
import { useVcas, VcaSlot } from "./Vca";
import { slotProps } from "./slotKeys";
import { BlankButtons, BlankEq, BlankRow, BlankSlots, MoreSlot, PlateName, SlotBox } from "./StripSlots";
import { TrackIcon, TrackIconArt } from "../tracks/icons/trackIconArt";
import { openTrackIconPicker } from "../tracks/icons/TrackIconPicker";
import { slotLayout, stripRows, stripWants, type SlotRows } from "./stripRows";
import { METER_W, PAN_KNOB, plateInk, STRIP_ICON } from "./mixerLayout";
import { panLabel } from "./panText";
import { onPress } from "../keyboard/keyNav";
import { PluginSlotView, moreFxMenu } from "./PluginSlot";

const PAN = { type: "pan" } as const;
/** Stereo Out's name plate. */
const MASTER_PLATE = "#8d9099";

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
 * The fader and meters, which take whatever height the strip has left, like Logic's. They're
 * their own component so that a new height (the Mixer resized) renders only them.
 */
function StripFader({ db, onChange, vca, touch, meter, dynamics }: { db: number; onChange: (db: number) => void; vca?: number; touch: TouchProps; meter: MeterSource | null; dynamics: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(160);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setHeight(Math.max(60, Math.floor(e.contentRect.height))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return (
    <div className="strip-fader" ref={box}>
      <Fader db={db} onChange={onChange} height={height} vca={vca} {...touch} />
      {meter ? (
        <div className="strip-meters">
          <Meter source={meter} height={height} width={METER_W} />
          {dynamics && <GrMeter source={meter} height={height} />}
        </div>
      ) : null}
    </div>
  );
}

/**
 * Logic-style channel strip. `track` is null for the Stereo Out strip. `rows` are the insert and
 * send rows shared by the strips beside it (stripRows.ts); without them it sizes to its own.
 */
export function ChannelStrip({ track, selected, rows }: { track: Track | null; selected?: boolean; rows?: SlotRows }) {
  const master = useStore((s) => s.snap?.project.master);
  const inputs = useStore((s) => s.snap?.audio.input_channels ?? 2);
  const isMaster = track === null;
  const id = track?.id ?? MASTER_ID;
  const peak = usePeakHold(isMaster ? STEREO_OUT_METER : id);
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
        {/* Then the track's icon, as in Logic (click it to choose another); Stereo Out's is the
            output's. */}
        <div className="strip-icon-row">
          {isMaster ? (
            <span className="strip-icon">
              <TrackIconArt id="speaker" size={STRIP_ICON} />
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
              <TrackIcon track={track} size={STRIP_ICON} />
            </button>
          )}
        </div>
        {isMaster || folder ? (
          <div className="strip-pan-knob" />
        ) : (
          <div className="strip-pan-knob">
            <Knob value={autoPan} size={PAN_KNOB} onChange={(pan) => api.updateTrack(id, { pan })} {...panTouch} />
            {/* The knob's own aria-valuetext says it; this is for the eye. */}
            <span className="pan-value" aria-hidden>
              {panLabel(autoPan)}
            </span>
          </div>
        )}
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

      <StripFader
        db={autoVolume}
        onChange={setVolume}
        vca={vca}
        touch={isMaster ? {} : volumeTouch}
        meter={folder ? null : isMaster ? { kind: "master" } : { kind: "track", id }}
        dynamics={dynamics}
      />

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
        className={"strip-name ink-" + plateInk(plate)}
        style={{ "--plate": plate } as React.CSSProperties}
        title={summing ? `${name} (summing stack)` : folder ? `${name} (folder stack)` : name}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          settingMenu(e.clientX, e.clientY);
        }}
      >
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
