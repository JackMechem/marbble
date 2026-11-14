// The LCD's Chord mode: the chord (or note) held on MIDI input, named as it's played, with its
// notes. Once the keys are up, the last chord stays, dimmed, until the next is played.
import { useEffect, useState } from "react";
import { onTick, useStore } from "../../state/store";
import { describeHeld } from "./chordName";
import { spellingFor } from "./noteNames";

export function LcdChord() {
  const [held, setHeld] = useState<number[]>([]);
  const [last, setLast] = useState<number[]>([]);
  useEffect(
    () =>
      onTick((t) => {
        const now = t.midi_held ?? [];
        setHeld((prev) => (prev.join() === now.join() ? prev : now));
        if (now.length) setLast((prev) => (prev.join() === now.join() ? prev : now));
      }),
    [],
  );
  const spelling = spellingFor(useStore((s) => s.snap?.project.key) ?? "");
  const shown = describeHeld(held.length ? held : last, spelling);
  // Long names step down a size or two to fit.
  const size = shown.main.length > 9 ? " smaller" : shown.main.length > 6 ? " small" : "";

  return (
    <div className={"lcd-body lcd-chord" + (held.length ? "" : " idle")} title="The chord or note held on MIDI input">
      <div className="lcd-field lcd-chord-name">
        <div className={"lcd-digits" + size}>{shown.main || "—"}</div>
        <div className="lcd-labels">
          <span>{shown.kind === "chord" ? "CHORD" : shown.kind === "note" ? "NOTE" : "NOTES"}</span>
        </div>
      </div>
      <div className="lcd-sep" />
      <div className="lcd-field lcd-chord-notes">
        <div className="lcd-notes">{shown.notes || (shown.main ? "" : "Play a chord")}</div>
      </div>
    </div>
  );
}
