// The file dialogs (Open, Save, choose a folder), behind a check that they can open. On Linux
// GTK's file chooser needs the org.gtk.Settings.FileChooser GSettings schema, and GLib ends the
// whole process when it is missing: File › Save As used to take the session with it. The app
// asks first (src-tauri/src/schemas.rs) and, when the schema is missing, says what to do instead.
// Every file dialog goes through here (tests/project/fileDialog.test.ts).
import { invoke } from "@tauri-apps/api/core";
import { message, open as openDialog, save as saveDialog, type OpenDialogOptions, type SaveDialogOptions } from "@tauri-apps/plugin-dialog";

/** Whether a file dialog can open; tells the user why when it can't. */
async function canOpen(): Promise<boolean> {
  let problem: string | null = null;
  try {
    problem = await invoke<string | null>("file_dialog_problem");
  } catch {
    // An older backend without the check: open as before.
  }
  if (!problem) return true;
  await message(problem, { title: "File Dialogs Unavailable", kind: "error" });
  return false;
}

/** The dialog plug-in's `open`; null (as if cancelled) when the dialog can't open. */
export async function open<T extends OpenDialogOptions>(options?: T): ReturnType<typeof openDialog<T>> {
  if (!(await canOpen())) return null as Awaited<ReturnType<typeof openDialog<T>>>;
  return openDialog(options);
}

/** The dialog plug-in's `save`; null (as if cancelled) when the dialog can't open. */
export async function save(options?: SaveDialogOptions): Promise<string | null> {
  return (await canOpen()) ? saveDialog(options) : null;
}
