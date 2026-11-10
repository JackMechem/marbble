// The Inspector's Track section, as in Logic's Track inspector, from what a track already has:
// name, colour, icon, type, input or instrument, output, freeze, regions and group. Name, colour,
// icon, input, output and freeze can be changed here; the channel strip below has the rest.
import { useEffect, useState } from "react";
import { isStack, type Project, type Track } from "../engine/project";
import { api } from "../engine/commands";
import { canFreeze, toggleFreeze } from "../tracks/freeze";
import { Icon } from "../ui/icons";
import { colorMenu } from "../tracks/contextMenus";
import { outputLabel, outputMenu } from "../mixer/routing";
import { useStore } from "../state/store";
import { inputLabel } from "../engine/project";
import { inputMenu } from "../mixer/ChannelStrip";
import { openMenuAt, type MenuItem } from "../ui/Menu";
import { TrackIcon } from "../tracks/icons/trackIconArt";
import { iconLabel } from "../tracks/icons/trackIcons";
import { openTrackIconPicker } from "../tracks/icons/TrackIconPicker";
import { onPress } from "../keyboard/keyNav";

const KIND_NAMES: Record<Track["kind"], string> = { audio: "Audio", instrument: "Software Instrument", folder: "Folder Stack", summing: "Summing Stack", aux: "Aux" };

function Row({ label, children, title }: { label: string; children: React.ReactNode; title?: string }) {
  return (
    <div className="insp-row" title={title}>
      <span className="insp-label">{label}</span>
      <span className="insp-value">{children}</span>
    </div>
  );
}

/** A value that opens a menu, drawn as text with a small chevron. */
function Popup({ text, items }: { text: string; items: () => MenuItem[] }) {
  return (
    <button
      type="button"
      className="insp-popup"
      {...onPress((e) => {
        e.preventDefault();
        e.stopPropagation();
        openMenuAt(e.currentTarget, items());
      })}
    >
      <span className="insp-popup-text">{text}</span>
      <Icon.chevron />
    </button>
  );
}

/** The track's name, renamed on Enter or when the field loses focus; Esc puts it back. */
function NameField({ track }: { track: Track }) {
  const [text, setText] = useState(track.name);
  useEffect(() => setText(track.name), [track.id, track.name]);
  const commit = () => {
    const v = text.trim();
    if (v && v !== track.name) api.updateTrack(track.id, { name: v });
    else setText(track.name);
  };
  return (
    <input
      className="insp-text"
      value={text}
      aria-label="Track name"
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        // Typing here mustn't trigger key commands.
        e.stopPropagation();
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          setText(track.name);
          // Blur after the reset, so the old name isn't committed.
          const el = e.currentTarget;
          requestAnimationFrame(() => el.blur());
        }
      }}
    />
  );
}

export function TrackRows({ project, track }: { project: Project; track: Track }) {
  const inputs = useStore((s) => s.snap?.audio.input_channels ?? 2);
  const stack = isStack(track.kind);
  return (
    <>
      <Row label="Name">
        <NameField track={track} />
      </Row>
      <Row label="Color">
        <button
          type="button"
          className="insp-color"
          style={{ background: track.color }}
          title="Track Color"
          aria-label="Track Color"
          {...onPress((e) => {
            e.preventDefault();
            e.stopPropagation();
            openMenuAt(e.currentTarget, colorMenu(track.color, (c) => c && api.updateTrack(track.id, { color: c })));
          })}
        />
      </Row>
      <Row label="Icon">
        <button
          type="button"
          className="insp-popup insp-icon"
          aria-haspopup="menu"
          title="Track Icon"
          onMouseDown={(e) => {
            e.preventDefault();
            e.stopPropagation();
            openTrackIconPicker(e.currentTarget, track);
          }}
          onClick={(e) => {
            // Return or Space.
            if (e.detail === 0) openTrackIconPicker(e.currentTarget, track);
          }}
        >
          <span className="insp-icon-art" style={{ color: track.color }}>
            <TrackIcon track={track} size={16} />
          </span>
          <span className="insp-popup-text">{iconLabel(track)}</span>
          <Icon.chevron />
        </button>
      </Row>
      <Row label="Type">{KIND_NAMES[track.kind]}</Row>
      {track.kind === "aux" && <Row label="Input">Bus {track.bus}</Row>}
      {track.kind === "audio" && (
        <Row label="Input">
          <Popup text={inputLabel(track.input)} items={() => inputMenu(track, inputs)} />
        </Row>
      )}
      {track.kind === "instrument" && <Row label="Instrument">{track.instrument?.name ?? "None"}</Row>}
      {track.kind !== "folder" && (
        <Row label="Output">
          <Popup text={outputLabel(project, track)} items={() => outputMenu(track)} />
        </Row>
      )}
      {canFreeze(track) && (
        <Row label="Freeze" title="Render the instrument and plug-ins, then unload them (the snowflake button)">
          <input type="checkbox" aria-label="Freeze" checked={!!track.frozen} onChange={() => toggleFreeze([track.id])} />
        </Row>
      )}
      {track.kind === "aux" ? null : stack ? (
        <Row label="Subtracks">{project.tracks.filter((t) => t.parent === track.id).length}</Row>
      ) : (
        <Row label="Regions">{track.regions.length}</Row>
      )}
      {track.group !== null && <Row label="Group">{project.groups.find((g) => g.id === track.group)?.name ?? "—"}</Row>}
    </>
  );
}
