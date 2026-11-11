//! Before the window opens: the `--scan-vst3` child mode, the display backend and logging, and
//! the session, with its plug-ins, settings and the project named on the command line.

use std::sync::Arc;

use daw_engine::library::plugin_db;
use daw_engine::Session;
use parking_lot::Mutex;

use crate::{crash, recent, schemas, settings};

/// In a child process the plug-in scanner starts (`marbble --scan-vst3 <bundle>`): scans the bundle
/// and exits.
pub fn scan_child_if_asked(args: &[String]) {
    if let Some(i) = args.iter().position(|a| a == plugin_db::SCAN_ARG) {
        let code = args.get(i + 1).map(|p| plugin_db::scan_child_main(p.as_ref())).unwrap_or(2);
        std::process::exit(code);
    }
}

/// The display backend, logging, and what has to happen before GTK starts.
pub fn prepare_environment() {
    // Plug-in editors are X11 windows, so the interface runs through XWayland too unless the
    // user chose a backend. MARBBLE_WAYLAND=1 runs it natively on Wayland (falling back to X11);
    // editors still open through XWayland, with OpenGL kept apart (`set_gl_isolation` below).
    #[cfg(target_os = "linux")]
    if std::env::var_os("GDK_BACKEND").is_none() {
        let native = std::env::var("MARBBLE_WAYLAND").is_ok_and(|v| v == "1");
        std::env::set_var("GDK_BACKEND", if native { "wayland,x11" } else { "x11" });
    }

    // To stderr, and kept for the Diagnostics window.
    crate::diagnostics::logger::install("info,jack=off");

    // Before GTK starts and anything else runs: it may set XDG_DATA_DIRS.
    schemas::ensure_file_chooser_schema();

    #[cfg(target_os = "linux")]
    vst3_host::editor::set_event_loop(Box::new(crate::glib_loop::GlibLoop));
}

/// The session: plug-ins (scanned the first time), settings, controller assignments, and the
/// project named on the command line.
pub fn session(args: &[String]) -> Arc<Mutex<Session>> {
    daw_engine::library::old_name::migrate();
    let plugins = plugin_db::load_cache().unwrap_or_else(|| {
        log::info!("scanning plug-ins for the first time");
        std::env::current_exe().map(|exe| plugin_db::scan_with(&exe, &settings::extra_plugin_folders())).unwrap_or_default()
    });
    let prefs = settings::load();
    let mut session = Session::new(prefs.audio, plugins);
    // Before a project loads, so its clips are locked as they're read (Settings › Audio).
    session.apply_memory_lock();
    session.set_prefs(prefs.prefs);
    session.load_controller_assignments(plugin_db::config_dir().join("controller-assignments.json"));
    // `marbble path/to/Song.marbble` opens a project at launch.
    if let Some(path) = args.iter().skip(1).find(|a| daw_engine::library::old_name::is_project(std::path::Path::new(a.trim_end_matches('/')))) {
        match session.open(std::path::Path::new(path)) {
            Ok(()) => recent::add(&session.project_path()),
            Err(e) => log::error!("could not open {path}: {e}"),
        }
    }
    let session = Arc::new(Mutex::new(session));
    crash::install(session.clone());
    session
}
