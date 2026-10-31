//! Bounce and export.
//!
//! Renders run as async commands, off the main thread, so the window stays responsive. They hold
//! the session for the whole render; the UI keeps a busy sheet up meanwhile.

use super::*;

/// Reports render progress (0..1) to the UI as `bounce-progress` events.
pub(super) fn progress(app: &tauri::AppHandle) -> impl FnMut(f64) {
    let app = app.clone();
    let mut last = -1.0;
    move |f| {
        if f - last >= 0.01 || f >= 1.0 {
            last = f;
            let _ = app.emit("bounce-progress", f);
        }
    }
}

/// The tracks Export All Tracks would write, for the dialog to list; with `ids`, those
/// Export Selected Tracks would write for that selection.
#[tauri::command]
pub fn export_targets(state: State<AppState>, ids: Option<Vec<Id>>) -> Vec<Id> {
    with(&state, |s| match ids {
        Some(ids) => s.export_targets_of(&ids),
        None => s.export_targets(),
    })
}

#[tauri::command(async)]
pub fn bounce_project(app: tauri::AppHandle, state: State<AppState>, path: String, options: RenderOptions) -> R<BounceResult> {
    let mut p = progress(&app);
    with(&state, |s| s.bounce_project(&PathBuf::from(path), &options, &mut p)).map_err(|e| e.to_string())
}

/// Export All Tracks, or with `ids` Export Selected Tracks.
#[tauri::command(async)]
pub fn export_tracks(app: tauri::AppHandle, state: State<AppState>, dir: String, options: RenderOptions, ids: Option<Vec<Id>>) -> R<BounceResult> {
    let mut p = progress(&app);
    let dir = PathBuf::from(dir);
    with(&state, |s| match ids {
        Some(ids) => s.export_selected_tracks(&dir, &ids, &options, &mut p),
        None => s.export_tracks(&dir, &options, &mut p),
    })
    .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn export_region(app: tauri::AppHandle, state: State<AppState>, id: Id, path: String, options: RenderOptions) -> R<BounceResult> {
    let mut p = progress(&app);
    with(&state, |s| s.export_region(id, &PathBuf::from(path), &options, &mut p)).map_err(|e| e.to_string())
}

/// Bounce in Place for `ids` (tracks), or for `regions` when that isn't empty. The render runs
/// here; the track edits (which may delete plug-ins) then run on the main thread as one undo
/// step.
#[tauri::command(async)]
pub fn bounce_in_place(app: tauri::AppHandle, state: State<AppState>, ids: Vec<Id>, regions: Vec<Id>, options: BipOptions) -> R<Made> {
    let mut p = progress(&app);
    let renders = with(&state, |s| s.render_in_place(&ids, &regions, &options, &mut p)).map_err(|e| e.to_string())?;
    let session = state.session.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let mut s = session.lock();
        s.checkpoint("Bounce in Place", None, options.source == BipSource::Delete);
        let res = s.place_bounces(renders, &options).map(|made| {
            if let Some(&last) = made.last() {
                s.select_track(Some(last));
            }
            Made { snapshot: snapshot(&s), ids: made }
        });
        if res.is_err() {
            s.abandon_checkpoint();
        }
        let _ = tx.send(res.map_err(|e| e.to_string()));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}
