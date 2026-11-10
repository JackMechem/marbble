//! Marbble's audio engine: project model, real-time graph, device I/O, recording and MIDI.
//!
//! Two halves never share mutable data: [`Session`] (`session/`) owns the document and runs off
//! the audio thread; [`dsp::Engine`] (`dsp/`) is the real-time graph. Everything else is what they
//! are built from. See `docs/ARCHITECTURE.md`.

// The two halves.
pub mod dsp; // The real-time graph: everything `Engine::process` runs on the audio thread.
pub mod session; // The non-real-time side: owns the project, turns edits into engine commands.

// The document, and pure logic on it that both halves and the tests use.
pub mod edit; // Editing logic on the document (comping, fades, drag modes, arranging…), pure.
pub mod model; // The project document: what is saved and mirrored to the UI, with its tempo map.
pub mod prefs; // Options chosen in Settings and Project Settings.

// Built-in sound.
pub mod fx; // Built-in effects and their registry.
pub mod instrument; // Built-in instruments and their registry.
pub mod synth_dsp; // Oscillators, filters, envelopes and voice allocation the instruments are built from.

// The outside world.
pub mod audio_file; // Audio files: reading, writing, holding in memory, repairing.
pub mod audio_io; // The audio device, the recorder thread, and the machine settings for audio.
pub mod control; // Controller assignments, controller feedback and control surfaces.
pub mod library; // What is kept outside any project: presets, strip settings, the plug-in database.
pub mod midi; // MIDI ports in and out, and Standard MIDI Files.

pub use session::Session;
pub use vst3_host;
