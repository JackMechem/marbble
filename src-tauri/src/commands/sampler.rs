//! Fermata Sampler's panel: zones, adding and slicing files, recording into it. Long jobs decode
//! on a worker and hand the edit back to the main thread.

use super::*;

/// A snapshot plus what a sampler command has to say.
#[derive(Serialize)]
pub struct WithSnap<T> {
    snapshot: Snapshot,
    result: T,
}

/// Runs an edit as one undo step and reports what it said.
fn sampler_edit<T>(state: &State<AppState>, label: &str, key: Option<String>, f: impl FnOnce(&mut Session) -> anyhow::Result<T>) -> R<WithSnap<T>> {
    with(state, |s| {
        let pushed = s.checkpoint(label, key, false);
        match f(s) {
            Ok(result) => Ok(WithSnap { snapshot: snapshot(s), result }),
            Err(e) => {
                if pushed {
                    s.abandon_checkpoint();
                }
                Err(e.to_string())
            }
        }
    })
}

/// The same from a worker: the edit itself runs on the main thread.
fn sampler_edit_on_main<T: Send + 'static>(
    app: &tauri::AppHandle,
    state: &State<AppState>,
    label: &'static str,
    f: impl FnOnce(&mut Session) -> anyhow::Result<T> + Send + 'static,
) -> R<WithSnap<T>> {
    let session = state.session.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let mut s = session.lock();
        let pushed = s.checkpoint(label, None, false);
        let res = match f(&mut s) {
            Ok(result) => Ok(WithSnap { snapshot: snapshot(&s), result }),
            Err(e) => {
                if pushed {
                    s.abandon_checkpoint();
                }
                Err(e.to_string())
            }
        };
        let _ = tx.send(res);
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

/// A sampler's zones and what's known of their files.
#[tauri::command]
pub fn sampler_view(state: State<AppState>, track: Id) -> R<daw_engine::session::SamplerView> {
    with(&state, |s| s.sampler_view(track).map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn sampler_waveform(state: State<AppState>, track: Id, zone: u64, from: u64, to: u64, points: usize) -> R<daw_engine::session::SamplerWave> {
    with(&state, |s| s.sampler_waveform(track, zone, from, to, points).map_err(|e| e.to_string()))
}

/// New versions of some zones. A drag of the same zones merges into one undo step.
#[tauri::command]
pub fn sampler_update_zones(state: State<AppState>, track: Id, zones: Vec<Zone>, merge: Option<String>) -> R<Snapshot> {
    let key = merge.map(|m| format!("sampler:{track}:{m}"));
    try_edit(&state, "Edit Sampler Zone", key, false, |s| s.sampler_update_zones(track, zones))
}

#[tauri::command]
pub fn sampler_delete_zones(state: State<AppState>, track: Id, ids: Vec<u64>) -> R<Snapshot> {
    try_edit(&state, "Delete Sampler Zones", None, false, |s| s.sampler_delete_zones(track, &ids))
}

/// Adds audio files, decoding them on this worker first so a big drop doesn't hold up the window.
#[tauri::command(async)]
pub fn sampler_add_files(
    app: tauri::AppHandle,
    state: State<AppState>,
    track: Id,
    paths: Vec<PathBuf>,
    at_key: Option<u8>,
) -> R<WithSnap<daw_engine::session::SamplerAdded>> {
    let (pool, dir) = with(&state, |s| (s.sampler_pool(), s.sampler_dir()));
    let prepared = daw_engine::session::prepare_files(&pool, &dir, std::path::Path::new(""), &paths);
    sampler_edit_on_main(&app, &state, "Add Samples", move |s| s.sampler_add_prepared(track, prepared, at_key))
}

/// Loads an SFZ file in place of the zones, off the main thread until the edit itself.
#[tauri::command(async)]
pub fn sampler_load_sfz(app: tauri::AppHandle, state: State<AppState>, track: Id, path: PathBuf) -> R<WithSnap<daw_engine::session::SfzReport>> {
    let (pool, dir) = with(&state, |s| (s.sampler_pool(), s.sampler_dir()));
    let file = daw_engine::instrument::sampler::sfz::read(&path).map_err(|e| e.to_string())?;
    let sub = path.file_stem().map(PathBuf::from).unwrap_or_default();
    let paths: Vec<PathBuf> = file.regions.iter().map(|r| r.path.clone()).collect();
    let prepared = daw_engine::session::prepare_files(&pool, &dir, &sub, &paths);
    sampler_edit_on_main(&app, &state, "Load SFZ", move |s| s.sampler_finish_sfz(track, file, prepared))
}

/// Cuts a zone at its transients, one slice per key. Returns how many.
#[tauri::command]
pub fn sampler_slice(state: State<AppState>, track: Id, zone: u64, sensitivity: f32) -> R<WithSnap<usize>> {
    sampler_edit(&state, "Slice Zone", None, |s| s.sampler_slice(track, zone, sensitivity))
}

/// Points a zone at another file.
#[tauri::command]
pub fn sampler_replace_file(state: State<AppState>, track: Id, zone: u64, path: PathBuf) -> R<Snapshot> {
    try_edit(&state, "Replace Sample", None, false, |s| s.sampler_replace_file(track, zone, &path))
}

#[tauri::command]
pub fn sampler_record_start(state: State<AppState>, track: Id, input: InputSel) -> R<()> {
    with(&state, |s| {
        if s.audio_status.input_channels == 0 {
            return Err("No audio input is open. Choose one in Settings › Audio.".to_string());
        }
        s.sampler_record_start(track, input).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn sampler_record_status(state: State<AppState>) -> Option<daw_engine::session::SamplerRecording> {
    with(&state, |s| s.sampler_recording())
}

/// Stops recording and makes the take a zone. Returns the zone.
#[tauri::command]
pub fn sampler_record_stop(state: State<AppState>, trim: bool, at_key: Option<u8>) -> R<WithSnap<u64>> {
    sampler_edit(&state, "Record Sample", None, |s| s.sampler_record_stop(trim, at_key))
}

#[tauri::command]
pub fn sampler_record_cancel(state: State<AppState>) {
    with(&state, |s| s.sampler_record_cancel())
}

/// Convert to Sampler Track. `slices`: a transient sensitivity (0..1) to cut it to keys by, or None
/// for one note playing the whole region. Returns the new track.
#[tauri::command]
pub fn convert_region_to_sampler(state: State<AppState>, region: Id, slices: Option<f32>) -> R<Made> {
    edit_made(&state, "Convert to Sampler Track", false, |s| s.convert_region_to_sampler(region, slices).map(|t| vec![t]))
}
