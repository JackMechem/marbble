//! Tauri commands. Synchronous commands run on the main thread, which is where VST3 wants its
//! controllers and editors to live.
//!
//! This file holds what every command shares: the [`Snapshot`] the frontend gets back, and `edit`,
//! `try_edit` and `edit_made`, which take the undo checkpoint around a change. The commands are in the
//! modules below, by what they act on, and all are reachable as `commands::<name>`, which is how
//! `main.rs` registers them. A new command also goes in that list; `tests/engine/tauriCommands.test.ts` checks
//! the frontend's calls against both.

use std::path::PathBuf;

use daw_engine::audio_io::devices::{list_devices, DeviceList};
use daw_engine::audio_io::{AudioConfig, AudioStatus};
use daw_engine::control::controllers::CtlTarget;
use daw_engine::edit::fades::{Crossfade, DragMode, FadeInType, FadeOutType};
use daw_engine::instrument::sampler::state::Zone;
use daw_engine::model::tempo::{SigEvent, TempoEvent};
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, Group, Id, InputSel, Project, Region, TakeRating, TrackKind, TrackSend, MASTER_ID};
use daw_engine::prefs::{BitDepth, ClickSettings, Prefs};
use daw_engine::session::{AssignmentPatch, AssignmentRow};
use daw_engine::session::{
    BipOptions, BipSource, BounceResult, ClipInfo, InstrumentOutputs, MidiContent, PdcReport, PlayedNotes, RangeEdit, RegionParamsPatch, RenderOptions,
    TrackMidiPatch, TrackPatch,
};
use daw_engine::vst3_host::{ParamInfo, PluginDescriptor};
use daw_engine::{edit::transients, library::plugin_db, Session};
use serde::Serialize;
use tauri::{Emitter, State};

use crate::AppState;

type R<T> = Result<T, String>;

mod transport;
pub use transport::*;
mod tracks;
pub use tracks::*;
mod regions;
pub use regions::*;
mod arrangement;
pub use arrangement::*;
mod takes;
pub use takes::*;
mod plugins;
pub use plugins::*;
mod automation;
pub use automation::*;
mod project;
pub use project::*;
mod bounce;
pub use bounce::*;
mod settings;
pub use settings::*;
mod sampler;
pub use sampler::*;

#[derive(Serialize, Clone)]
pub struct Snapshot {
    project: Project,
    selected: Option<Id>,
    path: String,
    saved: bool,
    dirty: bool,
    audio: AudioStatus,
    midi_inputs: Vec<String>,
    open_editors: Vec<Id>,
    midi_target: Option<String>,
    undo: Option<String>,
    redo: Option<String>,
    /// Plug-in latency per strip and the total compensation.
    pdc: PdcReport,
    /// Why the edit was refused and undone (it touched a frozen track).
    refused: Option<String>,
    /// Something the command did went wrong without failing it (the editor of a plug-in just
    /// inserted didn't open); shown as a toast.
    warning: Option<String>,
    /// Where regions crossfade, for drawing.
    crossfades: Vec<Crossfade>,
    /// Instance ids of VST3 slots whose plug-in isn't installed (or didn't load).
    missing: Vec<Id>,
    /// Instance ids of loaded VST3 inserts with a side-chain input.
    key_inputs: Vec<Id>,
    /// The notes of MIDI regions with playback parameters as they play, for drawing.
    played: Vec<PlayedNotes>,
    /// Loaded instruments with extra output buses.
    instrument_outputs: Vec<InstrumentOutputs>,
}

pub fn snapshot(s: &Session) -> Snapshot {
    Snapshot {
        project: s.project.for_ui(),
        selected: s.selected,
        path: s.project_path().to_string_lossy().to_string(),
        saved: s.saved,
        dirty: s.dirty,
        audio: s.audio_status.clone(),
        midi_inputs: s.midi_inputs(),
        open_editors: s.open_editors(),
        midi_target: s.midi_target_name(),
        undo: s.undo_label(),
        redo: s.redo_label(),
        pdc: s.pdc_report(),
        refused: None,
        warning: None,
        crossfades: s.crossfades(),
        missing: s.missing_plugins().iter().map(|p| p.uid).collect(),
        key_inputs: s.vst_key_inputs(),
        played: s.played_notes(),
        instrument_outputs: s.instrument_outputs(),
    }
}

fn with<T>(state: &State<AppState>, f: impl FnOnce(&mut Session) -> T) -> T {
    let mut s = state.session.lock();
    let out = f(&mut s);
    // Routes an edit changed get their task graph now, not at the next idle: until then the engine
    // plays on one it builds itself (see `Session::sync_parallel`).
    s.sync_parallel();
    out
}

fn snap(state: &State<AppState>, f: impl FnOnce(&mut Session)) -> Snapshot {
    with(state, |s| {
        f(s);
        snapshot(s)
    })
}

/// Like `snap`, recording an undo step first.
fn edit(state: &State<AppState>, label: &str, key: Option<String>, plugins: bool, f: impl FnOnce(&mut Session)) -> Snapshot {
    with(state, |s| {
        s.checkpoint(label, key, plugins);
        f(s);
        // Frozen tracks' regions and plug-ins can't change; such an edit is put back.
        let refused = s.frozen_guard().err().map(|e| e.to_string());
        Snapshot { refused, ..snapshot(s) }
    })
}

fn try_edit(state: &State<AppState>, label: &str, key: Option<String>, plugins: bool, f: impl FnOnce(&mut Session) -> anyhow::Result<()>) -> R<Snapshot> {
    try_snap(state, |s| {
        let pushed = s.checkpoint(label, key, plugins);
        // A failed edit leaves no undo step behind.
        if let Err(e) = f(s) {
            if pushed {
                s.abandon_checkpoint();
            }
            return Err(e);
        }
        s.frozen_guard()
    })
}

fn try_snap(state: &State<AppState>, f: impl FnOnce(&mut Session) -> anyhow::Result<()>) -> R<Snapshot> {
    with(state, |s| {
        f(s).map_err(|e| e.to_string())?;
        Ok(snapshot(s))
    })
}

/// A snapshot plus the ids of what an edit created (new regions or tracks), so the UI can
/// select them.
#[derive(Serialize)]
pub struct Made {
    snapshot: Snapshot,
    ids: Vec<Id>,
}

fn edit_made(state: &State<AppState>, label: &str, plugins: bool, f: impl FnOnce(&mut Session) -> anyhow::Result<Vec<Id>>) -> R<Made> {
    with(state, |s| {
        s.checkpoint(label, None, plugins);
        let ids = f(s).map_err(|e| {
            s.abandon_checkpoint();
            e.to_string()
        })?;
        s.frozen_guard().map_err(|e| e.to_string())?;
        Ok(Made { snapshot: snapshot(s), ids })
    })
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n > 1 {
        many.into()
    } else {
        one.into()
    }
}

#[tauri::command]
pub fn get_state(state: State<AppState>) -> Snapshot {
    snap(&state, |_| {})
}

/// `slot` of -1 addresses the instrument slot.
fn slot_index(slot: i64) -> usize {
    if slot < 0 {
        usize::MAX
    } else {
        slot as usize
    }
}

#[derive(Serialize)]
pub struct UndoResult {
    snapshot: Snapshot,
    /// What was undone or redone, if anything.
    label: Option<String>,
}

#[tauri::command]
pub fn undo(state: State<AppState>) -> UndoResult {
    with(&state, |s| {
        let label = s.undo();
        UndoResult { snapshot: snapshot(s), label }
    })
}

#[tauri::command]
pub fn redo(state: State<AppState>) -> UndoResult {
    with(&state, |s| {
        let label = s.redo();
        UndoResult { snapshot: snapshot(s), label }
    })
}

/// Drops the undo history (after the Project Chooser has set up a template).
#[tauri::command]
pub fn clear_undo_history(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.clear_history())
}
