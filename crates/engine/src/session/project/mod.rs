//! The project as files: new, open and save, autosave and crash recovery, Settings and Project
//! Settings, project alternatives, backups and templates, bounces and exports (with a multi-output
//! instrument's auxes in Bounce in Place, `bounce_outputs.rs`), decoding the clips a project
//! plays, and whether anything still names a file before it is deleted (`file_refs.rs`).

pub mod alternatives;
pub mod autosave;
pub mod backups;
pub mod bounce;
mod bounce_outputs;
pub mod clip_loading;
pub mod file_refs;
pub mod files;
pub mod recovery;
pub mod settings;
pub mod templates;
