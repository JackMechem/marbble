// One track's header: its icon, name, switches (M, S, R, I, On/Off, freeze…), volume and automation
// controls, laid out for its height, with the group badge and the drag that reorders it.
import { useState, type ReactNode } from "react";
import { canRecord, isStack, type Group, type Track } from "../../engine/project";
import { api } from "../../engine/commands";
import { Icon } from "../../ui/icons";
import { getState, setState, useStore } from "../../state/store";
import { clampHeight, setTrackHeight } from "../trackLayout";
import { clickTrack, selectedTracks, setHidden, toggleCollapsed } from "../trackCommands";
import { trackMenu } from "../contextMenus";
import { openMenu, openMenuAt, type MenuItem } from "../../ui/Menu";
import { automatable, VOLUME } from "../automation/params";
import { paramMenu } from "../automation/menus";
import { useAutomation } from "../automation/useAutomation";
import { AutoModeButton, AutomationControls } from "./AutomationHeader";
import { canFreeze, toggleFreeze } from "../freeze";
import { takeRowsHeight } from "../takes/takes";
import { fmt, keyHint } from "../../keyboard/keymap";
import { withKey } from "../../keyboard/keyFormat";
import { dragsTrack, headerTier, nameLines } from "./headerGeometry";
import type { SwitchId } from "./headerColumns";
import { LinesText } from "../../ui/PathText";
import { isTrackOff, powerTitle } from "../trackOff";
import { LevelSlider } from "./LevelSlider";
import { TrackAltButton } from "../alternatives/AlternativesPopup";
import { showsTrackAlts } from "../../project/alternativesModel";
import { TrackIcon } from "../icons/trackIconArt";
import { openTrackIconPicker } from "../icons/TrackIconPicker";
import { onPress } from "../../keyboard/keyNav";
import { dragWith } from "../../ui/pointerDrag";

export const INDENT = 12;
/** Smallest full header with room for the automation controls on a line of their own. */
const AUTO_ROW_H = 76;

/** The "⋯" of the button menu on short headers. */
function MoreIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <circle cx="2" cy="6" r="1.25" />
      <circle cx="6" cy="6" r="1.25" />
      <circle cx="10" cy="6" r="1.25" />
    </svg>
  );
}

/** One of a header's switches (M S R I, freeze): its button and its item in the "⋯" menu. */
interface Switch {
  cls: string;
  label: ReactNode;
  name: string;
  on: boolean;
  title: string;
  hint?: string;
  toggle: () => void;
  /** The tick in the "⋯" menu, when that isn't `on`. */
  checked?: boolean;
}

function GroupBadge({ group, onClick }: { group: Group; onClick: () => void }) {
  const short = /(\d+)$/.exec(group.name)?.[1] ?? group.name.slice(0, 2);
  return (
    <button
      className={"th-group" + (group.enabled ? "" : " off")}
      style={{ background: group.enabled ? group.color : undefined, borderColor: group.color }}
      title={`${group.name}${group.enabled ? "" : " (off)"} — click for settings`}
      {...onPress((e) => {
        e.stopPropagation();
        onClick();
      })}
    >
      {/^\d+$/.test(short) ? `G${short}` : short}
    </button>
  );
}

export function Header({
  track,
  index,
  selected,
  primary,
  height,
  depth,
  group,
  hideMode,
  columns,
  onReorderStart,
}: {
  track: Track;
  index: number;
  selected: boolean;
  primary: boolean;
  height: number;
  depth: number;
  group: Group | undefined;
  /** Hide mode (H) is on: show the H button. */
  hideMode: boolean;
  /** The button columns every header in view shares (headerColumns.ts). */
  columns: { full: SwitchId[]; compact: SwitchId[] };
  /** `click` runs when the press ends without dragging. */
  onReorderStart: (e: React.MouseEvent, index: number, click?: () => void) => void;
}) {
  const [editing, setEditing] = useState(false);
  const stack = isStack(track.kind);
  const [volume, volumeTouch] = useAutomation(track, VOLUME, track.volume_db);
  // Short headers put everything on one line and move the buttons into a "⋯" menu.
  const tier = headerTier(height);
  const full = tier === "full";
  // The automation controls take a third line on tall tracks. On shorter ones they take the
  // volume slider's place, so M, S, R and I stay in the header.
  const automation = useStore((s) => s.showAutomation) && automatable(track);
  const autoRow = full && automation && height >= AUTO_ROW_H;
  const autoInVolume = full && automation && !autoRow;
  const sliderW = Math.max(56, 88 - depth * INDENT);
  // A full header puts the slider on the buttons' line, so the name above has the header's whole
  // width up to the slider's end; on two lines when the height has room. (The automation controls
  // that stand in for the slider on short full headers are two lines tall, so they keep a column.)
  const sliderInRow = full && !autoInVolume;
  const lines = full ? nameLines(height, autoRow) : 1;
  const allTracks = useStore((s) => s.snap?.project.tracks) ?? [];
  const off = isTrackOff(allTracks, track);
  // "On" here means lit, which for this button is the track being off. Its menu entry reads the
  // other way round.
  const power: Switch = { cls: "power", label: <Icon.power />, name: "Track On", on: !!track.off, checked: !track.off, title: powerTitle(allTracks, track), hint: keyHint("track-on-off"), toggle: () => api.updateTrack(track.id, { off: !track.off }) };
  const switches: Switch[] = [
    power,
    { cls: "mute", label: "M", name: "Mute", on: track.mute, title: "Mute", hint: keyHint("track-mute"), toggle: () => api.updateTrack(track.id, { mute: !track.mute }) },
    { cls: "solo", label: "S", name: "Solo", on: track.solo, title: "Solo", hint: keyHint("track-solo"), toggle: () => api.updateTrack(track.id, { solo: !track.solo }) },
  ];
  if (canRecord(track.kind)) switches.push({ cls: "rec", label: "R", name: "Record Enable", on: track.armed, title: "Record Enable", hint: keyHint("track-arm"), toggle: () => api.updateTrack(track.id, { armed: !track.armed }) });
  if (track.kind === "audio") switches.push({ cls: "mon", label: "I", name: "Input Monitoring", on: track.monitor, title: "Input Monitoring", toggle: () => api.updateTrack(track.id, { monitor: !track.monitor }) });
  if (canFreeze(track)) {
    switches.push({
      cls: "freeze",
      label: <Icon.snowflake />,
      name: "Freeze",
      on: !!track.frozen,
      title: track.frozen ? "Frozen: plays a render, plug-ins unloaded. Click to unfreeze" : "Freeze: render the instrument and plug-ins, then unload them",
      // Acts on the selection when this track is part of it, as Logic's buttons do.
      toggle: () => toggleFreeze(selected ? selectedTracks() : [track.id]),
    });
  }
  // A switch's column, or a gap where this track has no such switch (or, on a one-line header, has
  // it off), so the buttons line up from header to header.
  const column = (id: SwitchId) => {
    const sw = switches.find((x) => x.cls === id);
    return sw && (full || sw.on) ? btn(sw) : <span key={id} className="tbtn ghost th-gap" aria-hidden />;
  };
  const btn = (sw: Switch) => (
    <button
      key={sw.cls}
      className={`tbtn ${sw.cls}` + (sw.on ? " on" : "")}
      title={withKey(sw.title, sw.hint ?? "")}
      aria-label={sw.name}
      aria-pressed={sw.checked ?? sw.on}
      {...onPress((e) => {
        e.stopPropagation();
        sw.toggle();
      })}
    >
      {sw.label}
    </button>
  );
  // The "⋯" menu: every switch, checked when on, then the automation lane's parameter when
  // automation is shown (its mode button stays in the header).
  const openMore = async (el: HTMLElement) => {
    const items: MenuItem[] = switches.map((sw) => ({ label: sw.name, checked: sw.checked ?? sw.on, hint: sw.hint, onClick: sw.toggle }));
    if (automation) items.push({ separator: true }, { label: "Automation Parameter", children: await paramMenu(track) });
    openMenuAt(el, items);
  };
  // Drag the bottom edge to resize this track; Alt-drag sizes every track.
  const startResize = (e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    const startY = e.clientY;
    // Open take folders add rows below the lane; the track's own height is what's resized.
    const startH = height - takeRowsHeight(track, getState().openFolders);
    const all = e.altKey;
    const move = (ev: MouseEvent) => {
      const h = clampHeight(startH + ev.clientY - startY);
      if (all) setState({ trackHeight: h, trackHeights: {} });
      else setTrackHeight(track.id, h);
    };
    const up = () => {
      document.body.style.cursor = "";
    };
    document.body.style.cursor = "ns-resize";
    dragWith(move, up);
  };
  const menu = (e: React.MouseEvent) => {
    e.preventDefault();
    // Right-clicking outside the selection selects just that track first, as in Logic.
    if (!selected) clickTrack(track.id, { shiftKey: false, ctrlKey: false, metaKey: false });
    openMenu(e.clientX, e.clientY, trackMenu(track, () => setEditing(true)));
  };
  const buttonRow = (
    <div className="th-buttons">
      {/* Short headers keep only the switches that are on, so a muted, soloed, armed or
          frozen track still shows it; the rest are in the "⋯" menu. */}
      {(full ? columns.full : columns.compact).map(column)}
      {group && <GroupBadge group={group} onClick={() => setState({ groupSheet: group.id })} />}
      {showsTrackAlts(track) && <TrackAltButton track={track} />}
      {hideMode && (
        <button
          className={"tbtn hide" + (track.hidden ? " on" : "")}
          title={track.hidden ? "Hidden: click to show this track" : "Hide this track"}
          aria-label="Hide Track"
          aria-pressed={track.hidden}
          {...onPress((e) => {
            e.stopPropagation();
            setHidden([track.id], !track.hidden);
          })}
        >
          H
        </button>
      )}
      {!full && automation && <AutoModeButton track={track} compact />}
      {!full && (
        <button
          className="tbtn th-more"
          title="On/Off, Mute, Solo, Record and more"
          aria-label="On/Off, Mute, Solo, Record and more"
          {...onPress((e) => {
            e.stopPropagation();
            openMore(e.currentTarget);
          })}
        >
          <MoreIcon />
        </button>
      )}
    </div>
  );
  const volumeCell = (
    <div className="th-volume">
      {autoInVolume ? (
        <AutomationControls track={track} width={sliderW} />
      ) : (
        <LevelSlider
          db={volume}
          onChange={(db) => api.updateTrack(track.id, { volume_db: db })}
          width={sliderW}
          tier={tier}
          track={track.kind === "folder" ? null : track.id}
          label={track.kind === "folder" ? "Folder volume (moves every subtrack)" : "Volume"}
          {...volumeTouch}
        />
      )}
    </div>
  );
  return (
    <div
      className={`track-header tier-${tier}` + (selected ? " selected" : "") + (primary ? " primary" : "") + (stack ? " stack" : "") + (track.hidden ? " is-hidden" : "") + (track.frozen ? " frozen" : "") + (off ? " is-off" : "")}
      style={{ height }}
      onMouseDown={(e) => {
        if (e.button !== 0) return;
        // A press anywhere that isn't a control can drag the track, as in Logic; without moving
        // it just selects. (The controls mostly stop their own presses; this catches the rest.)
        if (dragsTrack(e.target as Element)) onReorderStart(e, index);
        else clickTrack(track.id, e);
      }}
      onContextMenu={menu}
    >
      <div className="th-color" style={{ background: track.color }} />
      <div className="th-num" title="Drag to reorder">
        {index + 1}
      </div>
      {depth > 0 && <div className="th-indent" style={{ width: depth * INDENT }} />}
      {stack && (
        <button
          className="th-disclosure"
          title={track.collapsed ? "Show subtracks" : "Hide subtracks"}
          aria-label={track.collapsed ? "Show subtracks" : "Hide subtracks"}
          {...onPress((e) => {
            e.stopPropagation();
            toggleCollapsed(track);
          })}
        >
          {Icon.disclosure(!track.collapsed)}
        </button>
      )}
      {tier !== "tiny" && (
        <button
          type="button"
          className="th-icon"
          style={{ color: track.color }}
          aria-label="Track Icon"
          aria-haspopup="menu"
          title={(stack ? (track.kind === "summing" ? "Summing Stack · " : "Folder Stack · ") : track.kind === "aux" ? `Aux · Bus ${track.bus} · ` : "") + "Click to choose an icon, drag to reorder"}
          onMouseDown={(e) => {
            // Not focused by the press: Space must stay Play, not open the picker. The header
            // mustn't take the press as well (it would select the track twice).
            e.preventDefault();
            e.stopPropagation();
            const el = e.currentTarget;
            onReorderStart(e, index, () => openTrackIconPicker(el, track));
          }}
          onClick={(e) => {
            // Return or Space; a mouse click is handled when the press ends, as it may be a drag.
            if (e.detail === 0) openTrackIconPicker(e.currentTarget, track);
          }}
        >
          <TrackIcon track={track} />
        </button>
      )}
      <div className="th-main">
        <div className={"th-name-row" + (lines > 1 ? " two-lines" : "")}>
          {editing ? (
            <input
              className="th-rename"
              autoFocus
              defaultValue={track.name}
              onMouseDown={(e) => e.stopPropagation()}
              onBlur={(e) => {
                setEditing(false);
                const v = e.target.value.trim();
                if (v && v !== track.name) api.updateTrack(track.id, { name: v });
              }}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                if (e.key === "Escape") setEditing(false);
              }}
            />
          ) : (
            <LinesText className="th-name" text={track.name} max={lines} title={track.name} onDoubleClick={() => setEditing(true)} />
          )}
          {/* On the name's line: the button row has no room to spare at the usual widths. */}
          {full && btn(power)}
        </div>
        {sliderInRow ? (
          <div className="th-controls">
            {buttonRow}
            {volumeCell}
          </div>
        ) : (
          buttonRow
        )}
        {autoRow && <AutomationControls track={track} />}
      </div>
      {!sliderInRow && volumeCell}
      <div
        className="th-resize"
        title={`Drag to resize · ${fmt("Alt")}-drag resizes all tracks · double-click to reset`}
        onMouseDown={startResize}
        onDoubleClick={(e) => {
          e.stopPropagation();
          setTrackHeight(track.id, null);
        }}
      />
    </div>
  );
}
