//! Bounce and export.
//!
//! Renders run as async commands, off the main thread, so the window stays responsive. They hold
//! the session for the whole render; the UI keeps a busy sheet up meanwhile. Around each command
//! the main thread readies the plug-ins for it and puts them back after (`offline`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

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

/// A render command is under way. Another is refused rather than wait: its main-thread steps would
/// hold the main thread on the session for as long as the first renders, and its end would put
/// the first one's plug-ins back in real time.
static RENDERING: AtomicBool = AtomicBool::new(false);

/// Runs `render` (on this command's thread) between `Session::begin_renders` and `end_renders`,
/// which call the plug-ins and so run on the main thread: what the render plays with is settled
/// there (a CLAP plug-in's restart), the device stays silent while plug-ins are in their offline
/// mode, and they're back in real time afterwards, however the render ends.
pub(super) fn offline<T, E: std::fmt::Display>(app: &tauri::AppHandle, state: &State<AppState>, render: impl FnOnce() -> Result<T, E>) -> R<T> {
    if RENDERING.swap(true, Ordering::SeqCst) {
        return Err("Another bounce, export or freeze is still running.".into());
    }
    let _renders = Renders::begin(app, &state.session);
    render().map_err(|e| e.to_string())
}

/// Ends the renders when dropped, a panic's unwinding included.
struct Renders {
    app: tauri::AppHandle,
    session: Arc<Mutex<Session>>,
}

impl Renders {
    fn begin(app: &tauri::AppHandle, session: &Arc<Mutex<Session>>) -> Renders {
        on_main(app, session, Session::begin_renders);
        Renders { app: app.clone(), session: session.clone() }
    }
}

impl Drop for Renders {
    fn drop(&mut self) {
        on_main(&self.app, &self.session, Session::end_renders);
        RENDERING.store(false, Ordering::SeqCst);
    }
}

/// Runs `f` on the session on the main thread, and waits for it.
fn on_main(app: &tauri::AppHandle, session: &Arc<Mutex<Session>>, f: fn(&mut Session)) {
    let session = session.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let sent = app.run_on_main_thread(move || {
        f(&mut session.lock());
        let _ = tx.send(());
    });
    if sent.is_ok() {
        let _ = rx.recv();
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
    offline(&app, &state, || with(&state, |s| s.bounce_project(&PathBuf::from(path), &options, &mut p)))
}

/// Export All Tracks, or with `ids` Export Selected Tracks.
#[tauri::command(async)]
pub fn export_tracks(app: tauri::AppHandle, state: State<AppState>, dir: String, options: RenderOptions, ids: Option<Vec<Id>>) -> R<BounceResult> {
    let mut p = progress(&app);
    let dir = PathBuf::from(dir);
    offline(&app, &state, || {
        with(&state, |s| match ids {
            Some(ids) => s.export_selected_tracks(&dir, &ids, &options, &mut p),
            None => s.export_tracks(&dir, &options, &mut p),
        })
    })
}

#[tauri::command(async)]
pub fn export_region(app: tauri::AppHandle, state: State<AppState>, id: Id, path: String, options: RenderOptions) -> R<BounceResult> {
    let mut p = progress(&app);
    offline(&app, &state, || with(&state, |s| s.export_region(id, &PathBuf::from(path), &options, &mut p)))
}

/// Bounce in Place for `ids` (tracks), or for `regions` when that isn't empty. The render runs
/// here; the track edits (which may delete plug-ins) then run on the main thread as one undo
/// step.
#[tauri::command(async)]
pub fn bounce_in_place(app: tauri::AppHandle, state: State<AppState>, ids: Vec<Id>, regions: Vec<Id>, options: BipOptions) -> R<Made> {
    let mut p = progress(&app);
    let renders = offline(&app, &state, || with(&state, |s| s.render_in_place(&ids, &regions, &options, &mut p)))?;
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
