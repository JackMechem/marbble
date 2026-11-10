// The crash-recovery sheet: unsaved work found in autosaves and interrupted recordings.
import { useEffect, useMemo, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { act, call } from "../engine/commands";
import { recoveryDone, useRecoveries, type Recovery } from "./startup";
import { setState } from "../state/store";
import { SheetFrame, toast } from "../settings/panes/parts";
import { recordingSummary, recoveryHelp, recoveryIntro, when } from "../settings/settingsText";

/** Offers unsaved work found in autosaves and interrupted recordings (after a crash, or when
 * opening such a project). */
export function RecoverySheet() {
  const list = useRecoveries();
  const [busy, setBusy] = useState<string | null>(null);
  const [gone, setGone] = useState<string[]>([]);
  const items = useMemo(() => list.filter((r) => !gone.includes(r.path)), [list, gone]);
  const single = list.length === 1;
  const later = () => recoveryDone(false);
  useEffect(() => {
    if (list.length && !items.length) recoveryDone(false);
  }, [items.length, list.length]);

  const recover = async (r: Recovery) => {
    setBusy(r.path);
    const snap = await act("open_recovered", { path: r.path });
    setBusy(null);
    if (snap) {
      setState({ selectedRegions: [] });
      const takes = r.recording ? `, with ${r.recording.takes === 1 ? "the interrupted take" : `${r.recording.takes} interrupted takes`}` : "";
      toast(`Recovered “${r.name}”${takes}. Save it to keep the changes.`, 6000);
      recoveryDone(true);
    }
  };
  const discard = async (r: Recovery) => {
    const what = r.has_autosave ? `the autosaved changes to “${r.name}”` : `the interrupted recording in “${r.name}”`;
    const audio = r.recording ? " The recorded audio files stay in its Audio Files folder, where you can import them." : "";
    const ok = await ask(`Delete ${what}? ${r.saved ? "The last saved version stays as it is." : "The project was never saved, so nothing of it will remain."}${audio}`, {
      title: r.has_autosave ? "Discard Autosave" : "Discard Recording",
      kind: "warning",
      okLabel: "Discard",
      cancelLabel: "Cancel",
    });
    if (!ok) return;
    await call("discard_autosave", { path: r.path });
    setGone((g) => [...g, r.path]);
  };
  const openSaved = async (r: Recovery) => {
    await call("discard_autosave", { path: r.path });
    const snap = await act("open_project", { path: r.path });
    if (snap) setState({ selectedRegions: [] });
    recoveryDone(true);
  };

  return (
    <SheetFrame title={single ? "Recover Unsaved Changes?" : "Recover Unsaved Work"} onClose={later} width={560}>
      <div className="pref-note recovery-intro">
        {recoveryIntro(list)}
      </div>
      <div className="recovery-list">
        {items.map((r) => (
          <div key={r.path} className="recovery-item">
            <div className="recent-text">
              <span className="recent-name">{r.name}</span>
              <span className="recent-path">{r.path}</span>
              <span className="dim">
                {r.has_autosave && `Autosaved ${when(r.autosaved)} · `}
                {r.saved ? `last saved ${when(r.saved)}` : "never saved"}
              </span>
              {r.recording && (
                <span className="dim">
                  Recording interrupted {when(r.recording.started)}: {recordingSummary(r.recording)}
                </span>
              )}
            </div>
            <div className="sheet-buttons">
              <button className="push" disabled={busy !== null} onClick={() => discard(r)}>
                Discard
              </button>
              {single && r.saved && (
                <button className="push" disabled={busy !== null} onClick={() => openSaved(r)}>
                  Open Last Saved
                </button>
              )}
              <button className="push default" disabled={busy !== null} onClick={() => recover(r)}>
                {busy === r.path ? "Opening…" : "Recover"}
              </button>
            </div>
          </div>
        ))}
      </div>
      <div className="pref-note recovery-help">
        {recoveryHelp(single, items.some((r) => !!r.saved)).map(([name, what]) => (
          <span key={name}>
            <b>{name}</b> {what}
          </span>
        ))}
      </div>
      <div className="sheet-foot">
        <span className="dim" />
        <div className="sheet-buttons">
          {/* Not "Cancel": it keeps the unsaved work for later, which Open Last Saved doesn't. */}
          <button className="push" onClick={later}>
            Decide Later
          </button>
        </div>
      </div>
    </SheetFrame>
  );
}
