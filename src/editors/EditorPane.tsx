// The bottom editor pane: picks the Piano Roll or the Audio Track Editor for the selection.
// editors/ holds the editors of the bottom section (Piano Roll, Audio Track Editor, Event List, Step
// Sequencer), the note entry windows (Step Input, Musical Typing) and the MIDI edits they share.
import type { Track } from "../engine/project";
import type { Region } from "../engine/regions";
import { useStore } from "../state/store";
import { AudioEditor } from "./audioEditor/AudioEditor";
import { DetachButton } from "../app/detach/DetachButton";
import { PianoRoll } from "./pianoRoll/PianoRoll";
import { useEditorFollow } from "./stepSequencer/StepSequencer";

/**
 * The bottom editor pane (E). Follows the selection like Logic's main window editor: the Piano
 * Roll for a software instrument track, the Audio Track editor for an audio track. The region
 * shown first is the selected one on that track, if any.
 */
export function Editor() {
  const project = useStore((s) => s.snap?.project);
  const selectedTrack = useStore((s) => s.snap?.selected ?? null);
  const selected = useStore((s) => s.selectedRegions);
  let region: Region | null = null;
  let track: Track | null = null;
  for (const t of project?.tracks ?? []) {
    const r = t.regions.find((r) => r.id === selected[0]);
    if (r) {
      region = r;
      track = t;
    }
  }
  // A pattern region opens in the Step Sequencer.
  useEditorFollow(region, "editor");
  // No region selected: edit the selected track.
  if (!track) track = project?.tracks.find((t) => t.id === selectedTrack) ?? null;
  if (track?.kind === "instrument") return <PianoRoll track={track} region={region} />;
  if (track?.kind === "audio") return <AudioEditor track={track} region={region} />;
  return (
    <div className="editor">
      <div className="local-bar editor-bar">
        <div className="local-menus">
          <span className="local-title">Editor</span>
        </div>
        <div className="local-right">
          <DetachButton panel="editor" />
        </div>
      </div>
      <div className="editor-body">
        <div className="editor-empty">Select an audio or software instrument track to edit it here.</div>
      </div>
    </div>
  );
}
