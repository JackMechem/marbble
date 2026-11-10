// The Browser side pane (All Files): the folders on disk, to import an audio or MIDI file at the
// playhead or drag it into the Tracks area (project/fileDrop.ts takes the drop).
import { useEffect, useState } from "react";
import { act, call } from "../engine/commands";
import { Icon } from "../ui/icons";
import { isAudioPath, startFileDrag } from "../project/fileDrop";
import { importMidiFiles, isMidiPath } from "../project/midiFiles";
import { currentTick, getState, showToast } from "../state/store";

interface DirListing {
  path: string;
  parent: string | null;
  entries: { name: string; path: string; is_dir: boolean }[];
}

export function Browser() {
  const [dir, setDir] = useState<DirListing | null>(null);
  const load = (path: string | null) => call<DirListing>("list_dir", { path }).then((d) => d && setDir(d));
  useEffect(() => {
    load(null);
  }, []);
  const importAt = async (path: string) => {
    const snap = getState().snap;
    const pos = currentTick()?.pos ?? 0;
    // MIDI files go onto the selected instrument track, or a new one.
    if (isMidiPath(path)) {
      const inst = snap?.project.tracks.find((t) => t.id === snap.selected && t.kind === "instrument");
      await importMidiFiles([path], inst?.id ?? null, pos, null);
      return;
    }
    if (!isAudioPath(path)) {
      showToast("Only audio and MIDI files can be imported");
      return;
    }
    const track = snap?.project.tracks.find((t) => t.id === snap.selected && t.kind === "audio");
    if (!track) {
      showToast("Select an audio track to import into");
      return;
    }
    act("import_audio", { path, track: track.id, start: pos });
  };
  return (
    <div className="browser">
      <div className="pane-title">All Files</div>
      <div className="browser-path">
        <button className="small-btn icon" disabled={!dir?.parent} onClick={() => dir?.parent && load(dir.parent)} title="Enclosing folder" aria-label="Enclosing folder">
          <Icon.up />
        </button>
        <span title={dir?.path}>{dir?.path.split("/").pop() || dir?.path}</span>
      </div>
      <div className="browser-list">
        {dir?.entries.map((e) => (
          <div
            key={e.path}
            className={"browser-item" + (e.is_dir ? " dir" : "")}
            onMouseDown={(ev) => !e.is_dir && startFileDrag(ev, e.path, e.name)}
            onDoubleClick={() => (e.is_dir ? load(e.path) : importAt(e.path))}
            title={e.is_dir ? "Double-click to open" : "Drag onto a track (or the track list for a new track), or double-click to import at the playhead"}
          >
            {e.is_dir ? <Icon.folder /> : <Icon.file />}
            <span>{e.name}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
