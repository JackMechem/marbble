// Settings › General.
import { useEffect, useState } from "react";
import { act, call } from "../../api";
import { moveFolderKeys } from "../../oldName";
import { loadSettings, useSettings } from "../../prefs";
import { oldFolderNote, type OldProjectsFolder } from "../../settingsText";
import { useStore } from "../../store";
import { Check, NumberField, Select } from "../Form";
import { Foot, Row, Section, setPrefs, setUi, FolderField, toast } from "./parts";

export function GeneralPane({ close }: { close: () => void }) {
  const s = useSettings((s) => s);
  const [root, setRoot] = useState("");
  // The projects folder from before the rename, while it can be renamed; asked again when the
  // open project changes, since that can hold it.
  const [oldFolder, setOldFolder] = useState<OldProjectsFolder | null>(null);
  const path = useStore((st) => st.snap?.path);
  const saved = useStore((st) => st.snap?.saved);
  const [renamed, setRenamed] = useState(0);
  useEffect(() => {
    call<string>("default_project_dir").then((d) => d && setRoot(d));
  }, [s.prefs.general.project_folder, renamed]);
  useEffect(() => {
    call<OldProjectsFolder | null>("old_projects_folder").then(setOldFolder);
  }, [path, saved, renamed]);
  const renameOld = async () => {
    if (!oldFolder || !(await act("rename_old_projects_folder"))) return;
    moveFolderKeys(oldFolder.old, oldFolder.new);
    // The backend moved the folder settings with it; take them back before anything saves the old ones.
    await loadSettings();
    setRenamed((n) => n + 1);
    toast(`Renamed to ${oldFolder.new}`);
  };
  const g = s.prefs.general;
  return (
    <>
      <Section title="Startup">
        <Row label="At Launch:">
          <Select
            value={s.ui.startup}
            options={[
              ["chooser", "Show the Project Chooser"],
              ["last_project", "Open the most recent project"],
              ["empty_project", "Start with an empty project"],
            ]}
            onChange={(v) => setUi({ startup: v })}
          />
        </Row>
      </Section>
      <Section title="Projects">
        <Row label="Project Folder:" note="Where new projects are saved. The Project Chooser starts here.">
          <FolderField value={g.project_folder} placeholder={root || "~/Music/Marbble"} title="Choose the folder for new projects" onChange={(v) => setPrefs("general", { project_folder: v })} />
        </Row>
        {oldFolder && (
          <Row label="Old Folder:" note={oldFolderNote(oldFolder)}>
            <button className="push" disabled={!!oldFolder.open} onClick={renameOld}>
              Rename to Marbble
            </button>
          </Row>
        )}
        <Row
          label="Autosave:"
          note="Unsaved changes are written to project.autosave.json inside the project folder, never over your saved project. If Marbble quits unexpectedly, it offers to recover them at the next launch, along with anything that was being recorded."
        >
          <Select
            value={g.autosave_minutes}
            options={[
              [0, "Off"],
              [1, "Every minute"],
              [2, "Every 2 minutes"],
              [5, "Every 5 minutes"],
              [10, "Every 10 minutes"],
              [15, "Every 15 minutes"],
            ]}
            onChange={(v) => setPrefs("general", { autosave_minutes: v })}
          />
        </Row>
        <Row
          label="After Recording:"
          note="Takes are protected the moment you stop: if Marbble quits before you save, they come back at the next launch. Works with the interval autosave off too. Your saved project only changes when you save."
        >
          <Check checked={g.autosave_after_recording} onChange={(v) => setPrefs("general", { autosave_after_recording: v })}>
            Autosave as soon as a recording, Bounce in Place, Freeze or import finishes
          </Check>
        </Row>
        <Row
          label="Backups:"
          note="Each save keeps the version it replaces in the project's Backups folder, per project alternative, so File › Revert to can go back to it. 0 keeps just the last one, as project.json.bak."
        >
          <div className="row-inline">
            <NumberField value={g.backups ?? 10} min={0} max={100} step={1} resolution={1} onChange={(v) => setPrefs("general", { backups: Math.round(v) })} width={64} />
            <span className="dim">versions to keep</span>
          </div>
        </Row>
        <Row label="Undo Steps:" note="How many edits Undo can step back through. More steps use more memory on large projects.">
          <NumberField value={g.undo_limit} min={10} max={1000} step={10} resolution={1} onChange={(v) => setPrefs("general", { undo_limit: Math.round(v) })} />
        </Row>
        <Row label="Unsaved Changes:">
          <Check checked={s.ui.confirm_unsaved} onChange={(v) => setUi({ confirm_unsaved: v })}>
            Ask before quitting or closing a project with unsaved changes
          </Check>
        </Row>
      </Section>
      <Section title="Plug-in Windows">
        <Row
          label="Open Effect Panels:"
          note="Where a built-in effect's panel or the Channel EQ opens when you click it: floating inside the main window, docked in its bottom section, or in a window of its own that you can move to another monitor. Each panel can still be floated, docked or detached from its header. VST3 editors always open in their own window."
        >
          <Select
            value={s.ui.fx_panels ?? "floating"}
            options={[
              ["floating", "Floating"],
              ["docked", "Docked in the bottom section"],
              ["window", "Separate windows"],
            ]}
            onChange={(v) => setUi({ fx_panels: v })}
          />
        </Row>
      </Section>
      <Foot close={close} />
    </>
  );
}
