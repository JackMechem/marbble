//! What Marbble keeps outside any one project and shares between them: presets for the built-in
//! effects and instruments, saved channel strip settings, the database of scanned VST3 plug-ins,
//! and the migration of files left under the old name (Fermata). App settings are in `prefs.rs`.

pub mod old_name;
pub mod plugin_db;
pub mod presets;
pub mod strip_settings;
