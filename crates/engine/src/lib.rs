//! Marbble's audio engine: project model, real-time graph, device I/O, recording and MIDI.
//!
//! Two halves never share mutable data: [`Session`] (`session.rs`, `session/`) owns the document and
//! runs off the audio thread; [`dsp::Engine`] (`dsp.rs`, `dsp/`) is the real-time graph. See
//! `docs/ARCHITECTURE.md`, and the map in `docs/DEVELOPING.md`.

// The two halves.
pub mod dsp;
pub mod session;

// The document, and pure logic on it that both halves and the tests use.
pub mod arrange_cmds;
pub mod arrangement;
pub mod auto_move;
pub mod automation;
pub mod controllers;
pub mod drag_modes;
pub mod drum_replace;
pub mod eq;
pub mod fades;
pub mod feedback;
pub mod model;
pub mod pattern;
pub mod region_params;
pub mod strip_settings;
pub mod strip_silence;
pub mod takes;
pub mod tempo;
pub mod transients;
pub mod vca;

// Built-in sound: effects, instruments and what they are built from.
pub mod fx;
pub mod instrument;
pub mod synth;
pub mod synth_dsp;

// Files, devices and the system.
pub mod audio_file;
pub mod audio_io;
pub mod clip;
pub mod decode;
pub mod denormals;
pub mod memlock;
pub mod midi;
pub mod midi_io;
pub mod midi_out;
pub mod old_name;
pub mod plugin_db;
pub mod prefs;
pub mod presets;
pub mod recorder;
pub mod smf;
#[cfg(target_os = "linux")]
pub mod sysvol;
pub mod threads;
pub mod wav_repair;

pub use session::Session;
pub use vst3_host;
