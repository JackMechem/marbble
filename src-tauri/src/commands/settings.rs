//! The Settings window's backend: audio and MIDI devices, the system input level check, the plug-in
//! manager, reading and writing `settings.json`, and the update check's version and release pages.

use super::*;

#[tauri::command(async)]
pub fn audio_devices(host: Option<String>) -> DeviceList {
    list_devices(host.as_deref())
}

#[tauri::command]
pub fn thread_choices(state: State<AppState>) -> daw_engine::threads::ThreadChoices {
    with(&state, |s| s.thread_choices())
}

/// Settings › Audio › Lock audio memory: whether the memory is locked, or why not.
#[tauri::command]
pub fn memory_lock(state: State<AppState>) -> daw_engine::memlock::MemoryLock {
    with(&state, |s| s.memory_lock.clone())
}

#[tauri::command]
pub fn get_audio_config(state: State<AppState>) -> AudioConfig {
    with(&state, |s| s.audio_config.clone())
}

#[tauri::command]
pub fn set_audio_config(state: State<AppState>, config: AudioConfig) -> Snapshot {
    let mut settings = crate::settings::load();
    settings.audio = config.clone();
    crate::settings::save(&settings);
    snap(&state, |s| s.set_audio_config(config))
}

/// The system mixer volume on the current recording device (Linux sound servers only).
#[tauri::command(async)]
pub fn input_level(state: State<AppState>) -> Option<serde_json::Value> {
    #[cfg(target_os = "linux")]
    {
        let (host, input, enabled) = with(&state, |s| (s.audio_status.host.clone(), s.audio_config.input.clone(), s.audio_config.input_enabled));
        if !enabled || host == "JACK" {
            return None;
        }
        let level = daw_engine::sysvol::input_level(input.as_deref())?;
        let attenuating = level.attenuating();
        let mut v = serde_json::to_value(level).ok()?;
        v["attenuating"] = serde_json::Value::Bool(attenuating);
        Some(v)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        None
    }
}

#[tauri::command(async)]
pub fn reset_input_level(state: State<AppState>) -> R<Option<serde_json::Value>> {
    #[cfg(target_os = "linux")]
    {
        let input = with(&state, |s| s.audio_config.input.clone());
        daw_engine::sysvol::reset_input_level(input.as_deref())?;
    }
    Ok(input_level(state))
}

pub(super) fn enabled_plugins(list: Vec<PluginDescriptor>) -> Vec<PluginDescriptor> {
    let off = crate::settings::load().plugins.disabled;
    list.into_iter().filter(|p| !off.contains(&p.id)).collect()
}

/// Everything in Settings except the audio devices (which have their own commands).
#[derive(Serialize, serde::Deserialize)]
pub struct SettingsView {
    prefs: Prefs,
    ui: crate::settings::UiPrefs,
    plugins: crate::settings::PluginPrefs,
}

#[tauri::command]
pub fn get_settings() -> SettingsView {
    let s = crate::settings::load();
    SettingsView { prefs: s.prefs, ui: s.ui, plugins: s.plugins }
}

/// Stores the settings and applies them at once.
#[tauri::command]
pub fn set_settings(state: State<AppState>, settings: SettingsView) -> Snapshot {
    let mut all = crate::settings::load();
    all.prefs = settings.prefs.clone();
    all.ui = settings.ui;
    all.plugins = settings.plugins;
    crate::settings::save(&all);
    snap(&state, |s| s.set_prefs(settings.prefs))
}

#[derive(Serialize)]
pub struct ManagedPlugin {
    #[serde(flatten)]
    desc: PluginDescriptor,
    enabled: bool,
}

#[derive(Serialize)]
pub struct PluginManager {
    plugins: Vec<ManagedPlugin>,
    report: Option<plugin_db::ScanReport>,
    /// The folders a scan searches: the standard ones, then the user's.
    folders: Vec<String>,
}

/// Settings › Plug-ins: every scanned plug-in, the last scan's report and the search folders.
#[tauri::command]
pub fn plugin_manager(state: State<AppState>) -> PluginManager {
    let off = crate::settings::load().plugins.disabled;
    let plugins = with(&state, |s| s.plugins.clone()).into_iter().map(|d| ManagedPlugin { enabled: !off.contains(&d.id), desc: d }).collect();
    let folders = plugin_db::search_paths(&crate::settings::extra_plugin_folders()).iter().map(|p| p.to_string_lossy().to_string()).collect();
    PluginManager { plugins, report: plugin_db::load_report(), folders }
}

/// Settings › Audio › Reset: closes and reopens the audio devices.
#[tauri::command]
pub fn restart_audio(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.restart_audio())
}

#[tauri::command]
pub fn midi_ports(state: State<AppState>) -> Vec<daw_engine::midi_io::MidiPort> {
    with(&state, |s| s.midi_ports())
}

/// MIDI output ports, for controller feedback.
#[tauri::command]
pub fn midi_outputs(state: State<AppState>) -> Vec<daw_engine::midi_io::MidiPort> {
    with(&state, |s| s.midi_outputs())
}

#[tauri::command]
pub fn midi_monitor(state: State<AppState>) -> Vec<daw_engine::midi_io::MidiLogEntry> {
    with(&state, |s| s.midi_monitor())
}

/// Settings › MIDI › Control Surfaces: whether each unit's ports are connected and it answered.
#[tauri::command]
pub fn control_surfaces(state: State<AppState>) -> Vec<daw_engine::session::SurfaceStatus> {
    with(&state, |s| s.control_surfaces())
}

/// The running version, for the update check (src/updates.ts). A debug build takes
/// `MARBBLE_FAKE_VERSION` in its place, so the check can be tried against the real releases.
#[tauri::command]
pub fn app_version(app: tauri::AppHandle) -> String {
    let fake = if cfg!(debug_assertions) { std::env::var("MARBBLE_FAKE_VERSION").ok().filter(|v| !v.trim().is_empty()) } else { None };
    fake.map(|v| v.trim().to_string()).unwrap_or_else(|| app.package_info().version.to_string())
}

/// Where Marbble's release pages are; `open_release_page` opens nothing else.
const RELEASE_PAGES: &str = "https://github.com/JackMechem/marbble/releases/";

/// The update notice's View Release: opens the release's page in the browser, as Help's pages
/// open.
#[tauri::command]
pub fn open_release_page(url: String) -> R<()> {
    if !url.starts_with(RELEASE_PAGES) {
        return Err(format!("{url} isn't a Marbble release page"));
    }
    crate::menu::open_url(&url);
    Ok(())
}
