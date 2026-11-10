//! Commands for insert slots and instrument slots: VST3s and built-in effects and instruments, their
//! parameters and editors, side-chains, copy and paste, presets and channel strip settings.

use super::*;

/// The plug-ins offered in the Library and slot menus (the ones not switched off in
/// Settings › Plug-ins).
#[tauri::command]
pub fn list_plugins(state: State<AppState>) -> Vec<PluginDescriptor> {
    enabled_plugins(with(&state, |s| s.plugins.clone()))
}

#[tauri::command(async)]
pub fn rescan_plugins(state: State<AppState>) -> R<Vec<PluginDescriptor>> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let list = plugin_db::scan_with(&exe, &crate::settings::extra_plugin_folders());
    with(&state, |s| s.set_plugins(list.clone()));
    Ok(enabled_plugins(list))
}

#[tauri::command]
pub fn set_plugin(state: State<AppState>, track: Id, slot: i64, plugin_id: String, open: bool) -> R<Snapshot> {
    let mut warning = None;
    let snap = try_edit(&state, "Insert Plug-in", None, true, |s| {
        let uid = s.set_plugin(track, slot_index(slot), &plugin_id)?;
        // Built-in effects open their panel in the UI instead.
        if open && !plugin_id.starts_with("builtin:") {
            let title = s.project.track(track).map(|t| t.name.clone()).unwrap_or_else(|| "Stereo Out".into());
            // The plug-in stays inserted; the user hears why its window didn't open.
            if let Err(e) = s.open_editor(uid, &title) {
                log::warn!("{e}");
                warning = Some(format!("Couldn't open the plug-in's window: {e}"));
            }
        }
        Ok(())
    })?;
    Ok(Snapshot { warning, ..snap })
}

#[tauri::command]
pub fn remove_plugin(state: State<AppState>, track: Id, slot: i64) -> Snapshot {
    edit(&state, "Remove Plug-in", None, true, |s| s.remove_plugin(track, slot_index(slot)))
}

#[tauri::command]
pub fn set_bypass(state: State<AppState>, track: Id, slot: i64, bypass: bool) -> Snapshot {
    edit(&state, if bypass { "Bypass Plug-in" } else { "Enable Plug-in" }, None, false, |s| {
        // An Audio FX slot's bypass can be learned (Learn Assignment); the instrument's can't.
        if let Ok(slot) = u8::try_from(slot) {
            s.touched(CtlTarget::Bypass { track, slot });
        }
        s.set_bypass(track, slot_index(slot), bypass)
    })
}

/// A plug-in's panel or window was opened or brought forward: the selected track's controller
/// assignments to its focused plug-in follow it.
#[tauri::command]
pub fn focus_plugin(state: State<AppState>, uid: Id) {
    with(&state, |s| s.focus_plugin(uid))
}

#[tauri::command]
pub fn open_editor(state: State<AppState>, uid: Id, title: String) -> R<Snapshot> {
    try_snap(&state, |s| s.open_editor(uid, &title))
}

#[tauri::command]
pub fn list_plugin_params(state: State<AppState>, uid: Id) -> Vec<ParamInfo> {
    with(&state, |s| s.plugin_params(uid))
}

/// The built-in effects, as src/plugins/builtinEffects.ts `FxInfo`.
#[tauri::command]
pub fn list_builtin_effects() -> Vec<daw_engine::session::BuiltinInfo> {
    daw_engine::session::builtin_effects()
}

/// The built-in instruments, as src/plugins/builtinEffects.ts `InstInfo`.
#[tauri::command]
pub fn list_builtin_instruments() -> Vec<daw_engine::session::BuiltinInstrument> {
    daw_engine::session::builtin_instruments()
}

/// Sets a built-in effect's or instrument's parameter (plain units). A drag of one parameter makes one undo
/// step; while automation is being written the move belongs to the pass instead.
#[tauri::command]
pub fn set_builtin_param(state: State<AppState>, track: Id, slot: i64, id: u32, value: f32) -> R<Snapshot> {
    let slot = slot_index(slot);
    with(&state, |s| s.touched_slot(track, slot, id));
    if with(&state, |s| s.automation_writing(track)) {
        return try_snap(&state, |s| s.set_builtin_param(track, slot, id, value));
    }
    let uid = with(&state, |s| s.slot_get(track, slot).map(|p| p.uid));
    let key = uid.map(|uid| format!("fxparam:{uid}:{id}"));
    try_edit(&state, "Plug-in Parameter", key, false, |s| s.set_builtin_param(track, slot, id, value))
}

/// Every parameter value of a built-in effect instance: (id, plain value).
#[tauri::command]
pub fn builtin_params(state: State<AppState>, uid: Id) -> Vec<(u32, f32)> {
    with(&state, |s| s.builtin_params(uid))
}

/// Keys an insert's side-chain from a track (null: its own input); refuses loops.
#[tauri::command]
pub fn set_sidechain(state: State<AppState>, track: Id, slot: i64, source: Option<Id>) -> R<Snapshot> {
    try_edit(&state, "Side Chain", None, false, |s| s.set_sidechain(track, slot_index(slot), source))
}

/// A built-in effect's panel opened or closed: its meters and live values come with each tick
/// while it is open.
#[tauri::command]
pub fn watch_builtin(state: State<AppState>, uid: Id, on: bool) {
    with(&state, |s| s.watch_builtin(uid, on))
}

/// A slot's complete settings (a VST3's state included), for the UI's clipboard.
#[tauri::command]
pub fn copy_plugin(state: State<AppState>, track: Id, slot: i64) -> R<daw_engine::model::PluginSlot> {
    with(&state, |s| s.copy_plugin(track, slot_index(slot)).map_err(|e| e.to_string()))
}

/// Pastes a copied slot into `slot` of each of `tracks`, as one undo step.
#[tauri::command]
pub fn paste_plugin(state: State<AppState>, tracks: Vec<Id>, slot: i64, plugin: daw_engine::model::PluginSlot) -> R<Snapshot> {
    try_edit(&state, "Paste Plug-in", None, true, |s| s.paste_plugin(&tracks, slot_index(slot), &plugin))
}

/// The EQ thumbnail's click on a strip without a Channel EQ: one at the top of its Audio FX.
#[tauri::command]
pub fn insert_channel_eq(state: State<AppState>, track: Id) -> R<Snapshot> {
    try_edit(&state, "Insert Channel EQ", None, true, |s| s.insert_channel_eq(track).map(|_| ()))
}

/// An effect's factory and user presets.
#[tauri::command]
pub fn list_presets(effect: String) -> daw_engine::library::presets::PresetList {
    daw_engine::library::presets::list(&daw_engine::library::presets::user_root(), &effect)
}

/// One preset's settings.
#[tauri::command]
pub fn get_preset(effect: String, name: String, factory: bool) -> R<daw_engine::library::presets::Preset> {
    daw_engine::library::presets::get(&daw_engine::library::presets::user_root(), &effect, &name, factory).map_err(|e| e.to_string())
}

/// Applies a preset to a built-in effect's slot.
#[tauri::command]
pub fn load_preset(state: State<AppState>, track: Id, slot: i64, effect: String, name: String, factory: bool) -> R<Snapshot> {
    try_edit(&state, "Load Preset", None, false, |s| s.load_preset(track, slot_index(slot), &effect, &name, factory))
}

/// A preset's name as saved or renamed, and the snapshot showing it in the slots that refer to it.
#[derive(Serialize)]
pub struct NamedPreset {
    snapshot: Snapshot,
    name: String,
}

/// Saves a slot's settings as a user preset, which the slot then shows as loaded. Refuses to replace an existing one unless `overwrite`. Returns the name it
/// was saved under.
#[tauri::command]
pub fn save_preset(state: State<AppState>, track: Id, slot: i64, name: String, overwrite: Option<bool>) -> R<NamedPreset> {
    with(&state, |s| {
        let name = s.save_preset(track, slot_index(slot), &name, overwrite.unwrap_or(false)).map_err(|e| e.to_string())?;
        Ok(NamedPreset { snapshot: snapshot(s), name })
    })
}

/// Deletes a user preset; slots that showed it as loaded forget it.
#[tauri::command]
pub fn delete_preset(state: State<AppState>, effect: String, name: String) -> R<Snapshot> {
    try_snap(&state, |s| s.delete_user_preset(&effect, &name))
}

/// Renames a user preset, and the slots that show it as loaded; returns the new name.
#[tauri::command]
pub fn rename_preset(state: State<AppState>, effect: String, old: String, new: String) -> R<NamedPreset> {
    with(&state, |s| {
        let name = s.rename_user_preset(&effect, &old, &new).map_err(|e| e.to_string())?;
        Ok(NamedPreset { snapshot: snapshot(s), name })
    })
}

/// Drags a plug-in to another Audio FX slot: on the same strip it moves (the plug-ins between
/// shift over); onto another strip it moves there, or is copied when `copy` (Alt-drag).
#[tauri::command]
pub fn move_plugin(state: State<AppState>, from: Id, from_slot: i64, to: Id, to_slot: i64, copy: bool) -> R<Snapshot> {
    let label = if copy { "Copy Plug-in" } else { "Move Plug-in" };
    try_edit(&state, label, None, true, |s| s.move_plugin(from, slot_index(from_slot), to, slot_index(to_slot), copy))
}

/// Resets the clip indicators of these strips (0 for Stereo Out), or of every strip when
/// `tracks` is omitted. Not an undo step.
#[tauri::command]
pub fn reset_clip_indicators(state: State<AppState>, tracks: Option<Vec<Id>>) {
    with(&state, |s| s.reset_peak_holds(tracks.as_deref()))
}

/// A strip's channel strip setting (instrument, Audio FX with their state, Channel EQ), for
/// the clipboard.
#[tauri::command]
pub fn copy_strip(state: State<AppState>, track: Id) -> R<daw_engine::library::strip_settings::StripSetting> {
    with(&state, |s| s.copy_strip(track).map_err(|e| e.to_string()))
}

/// Applies a channel strip setting to each of `tracks`, as one undo step.
#[tauri::command]
pub fn paste_strip(state: State<AppState>, tracks: Vec<Id>, setting: daw_engine::library::strip_settings::StripSetting) -> R<Snapshot> {
    try_edit(&state, "Paste Channel Strip", None, true, |s| s.paste_strip(&tracks, &setting))
}

/// Reset Channel Strip on each of `tracks`, as one undo step.
#[tauri::command]
pub fn reset_strip(state: State<AppState>, tracks: Vec<Id>) -> R<Snapshot> {
    try_edit(&state, "Reset Channel Strip", None, true, |s| s.reset_strip(&tracks))
}

/// The saved channel strip settings, by name.
#[tauri::command]
pub fn list_strip_settings() -> Vec<String> {
    daw_engine::library::strip_settings::list(&daw_engine::library::strip_settings::user_root())
}

/// Saves a strip's setting under `name`. Refuses to replace an existing one unless
/// `overwrite`. Returns the name it was saved under.
#[tauri::command]
pub fn save_strip_setting(state: State<AppState>, track: Id, name: String, overwrite: Option<bool>) -> R<String> {
    let root = daw_engine::library::strip_settings::user_root();
    with(&state, |s| s.save_strip_setting(&root, track, &name, overwrite.unwrap_or(false)).map_err(|e| e.to_string()))
}

/// Loads a saved channel strip setting onto each of `tracks`, as one undo step.
#[tauri::command]
pub fn load_strip_setting(state: State<AppState>, tracks: Vec<Id>, name: String) -> R<Snapshot> {
    let root = daw_engine::library::strip_settings::user_root();
    try_edit(&state, "Load Channel Strip Setting", None, true, |s| s.load_strip_setting(&root, &tracks, &name))
}

#[tauri::command]
pub fn delete_strip_setting(name: String) -> R<()> {
    daw_engine::library::strip_settings::delete(&daw_engine::library::strip_settings::user_root(), &name).map_err(|e| e.to_string())
}

/// Takes an insert's side-chain key before (`pre`) or after the source's fader.
#[tauri::command]
pub fn set_sidechain_pre(state: State<AppState>, track: Id, slot: i64, pre: bool) -> R<Snapshot> {
    try_edit(&state, "Side Chain", None, false, |s| s.set_sidechain_pre(track, slot_index(slot), pre))
}

/// Sets several of a built-in effect's parameters at once (plain units), as one undo step: a
/// point dragged in the Linear Phase EQ's graph moves its frequency and gain together, and a
/// drag of the same set merges into one step.
#[tauri::command]
pub fn set_builtin_params(state: State<AppState>, track: Id, slot: i64, values: Vec<(u32, f32)>) -> R<Snapshot> {
    let slot = slot_index(slot);
    with(&state, |s| values.iter().for_each(|(id, _)| s.touched_slot(track, slot, *id)));
    let set_all = |s: &mut Session| values.iter().try_for_each(|&(id, v)| s.set_builtin_param(track, slot, id, v));
    if with(&state, |s| s.automation_writing(track)) {
        return try_snap(&state, set_all);
    }
    let uid = with(&state, |s| s.slot_get(track, slot).map(|p| p.uid));
    let mut ids: Vec<u32> = values.iter().map(|v| v.0).collect();
    ids.sort_unstable();
    let key = uid.map(|uid| format!("fxparams:{uid}:{ids:?}"));
    try_edit(&state, "Plug-in Parameter", key, false, set_all)
}
