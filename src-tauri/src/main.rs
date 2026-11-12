//! The application: what runs before the window opens (`startup.rs`, the `--scan-vst3` child mode
//! among it), the window's setup with the timers that drive the session (`timers.rs`), and the
//! list of commands registered with Tauri, each passing the activity log on its way in. The
//! commands themselves are in `commands/` (the Diagnostics window's in `diagnostics/`), and
//! `Session` in the engine crate does the work.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod crash;
mod diagnostics;
#[cfg(target_os = "linux")]
mod glib_loop;
mod menu;
mod recent;
mod schemas;
mod settings;
mod startup;
mod timers;
mod windows;

use std::sync::Arc;

use daw_engine::Session;
use parking_lot::Mutex;
use tauri::Manager;

pub struct AppState {
    pub session: Arc<Mutex<Session>>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    startup::scan_child_if_asked(&args);
    startup::prepare_environment();
    let session = startup::session(&args);
    // Record Everything, if it was on: from here, so it sees the window open.
    if settings::load().diagnostics.record_activity {
        if let Err(e) = diagnostics::activity::set(true) {
            log::warn!("{e}");
        }
    }
    diagnostics::activity::start_watch(session.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { session: session.clone() })
        .on_window_event(windows::on_window_event)
        .setup(move |app| {
            // GTK is up now. On native Wayland its EGL context would stop GLX editors opening.
            #[cfg(target_os = "linux")]
            if gdk::Display::default().is_some_and(|d| glib::prelude::ObjectExt::type_(&d).name() == "GdkWaylandDisplay") {
                log::info!("running natively on Wayland; plug-in editors open through XWayland");
                vst3_host::editor::set_gl_isolation(gdk::GLContext::clear_current);
            }
            menu::install(app)?;
            let handle = app.handle().clone();
            windows::forward_plugin_keys(&handle);

            timers::start(&handle, &session)?;

            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("Marbble");
            }
            Ok(())
        })
        .invoke_handler(logged(tauri::generate_handler![
            commands::get_state,
            commands::list_plugins,
            commands::rescan_plugins,
            commands::audio_devices,
            commands::get_audio_config,
            commands::thread_choices,
            commands::memory_lock,
            commands::set_audio_config,
            commands::play,
            commands::stop,
            commands::toggle_play,
            commands::record,
            commands::discard_last_recording,
            commands::mark_take,
            commands::locate,
            commands::set_cycle,
            commands::set_tempo,
            commands::set_signature,
            commands::set_key,
            commands::set_metronome,
            commands::set_count_in,
            commands::set_master_volume,
            commands::add_tracks,
            commands::delete_track,
            commands::update_track,
            commands::set_track_icon,
            commands::select_track,
            commands::reorder_tracks,
            commands::move_region,
            commands::resize_region,
            commands::resize_regions,
            commands::delete_regions,
            commands::set_region_muted,
            commands::rename_region,
            commands::split_regions,
            commands::import_audio,
            commands::import_audio_files,
            commands::midi_event,
            commands::undo,
            commands::redo,
            commands::clip_waveform,
            commands::delete_tracks,
            commands::delete_unused_tracks,
            commands::duplicate_tracks,
            commands::create_stack,
            commands::flatten_stack,
            commands::move_tracks,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::set_track_group,
            commands::paste_regions,
            commands::repeat_regions,
            commands::set_region_gain,
            commands::normalize_regions,
            commands::set_region_fades,
            commands::set_region_color,
            commands::transpose_regions,
            commands::quantize_regions,
            commands::create_midi_region,
            commands::regions_to_new_track,
            commands::reverse_regions,
            commands::join_regions,
            commands::set_plugin,
            commands::remove_plugin,
            commands::set_bypass,
            commands::open_editor,
            commands::new_project,
            commands::save_project,
            commands::open_project,
            commands::list_dir,
            commands::input_level,
            commands::reset_input_level,
            commands::set_analyzer,
            commands::recent_projects,
            commands::forget_recent,
            commands::path_exists,
            commands::default_project_dir,
            commands::old_projects_folder,
            commands::rename_old_projects_folder,
            commands::set_region_loop,
            commands::set_regions_looped,
            commands::convert_loops,
            commands::set_tracks_hidden,
            commands::create_aux,
            commands::set_send,
            commands::set_send_level,
            commands::set_output,
            commands::export_targets,
            commands::bounce_project,
            commands::export_tracks,
            commands::export_region,
            commands::bounce_in_place,
            commands::split_regions_at,
            commands::edit_range,
            commands::clip_transients,
            commands::clip_info,
            commands::set_automation_lane,
            commands::set_auto_mode,
            commands::clear_automation,
            commands::automation_touch,
            commands::list_plugin_params,
            commands::get_settings,
            commands::set_settings,
            commands::plugin_manager,
            commands::restart_audio,
            commands::midi_ports,
            commands::midi_outputs,
            commands::midi_monitor,
            commands::control_surfaces,
            commands::set_click,
            commands::set_record_bit_depth,
            commands::set_project_notes,
            commands::nudge_regions,
            commands::recovery_info,
            commands::recoverable_projects,
            commands::open_recovered,
            commands::discard_autosave,
            commands::set_midi_content,
            commands::import_midi,
            commands::export_midi,
            commands::freeze_tracks,
            commands::unfreeze_tracks,
            commands::add_marker,
            commands::delete_markers,
            commands::rename_marker,
            commands::move_marker,
            commands::set_marker_color,
            commands::set_punch,
            commands::set_comp_range,
            commands::select_take,
            commands::step_take,
            commands::move_comp_edge,
            commands::remove_comp_section,
            commands::move_comp_section,
            commands::rename_take,
            commands::delete_take,
            commands::delete_unused_takes,
            commands::set_take_rating,
            commands::set_take_color,
            commands::delete_bad_takes,
            commands::flatten_take_folder,
            commands::merge_take_folder,
            commands::unpack_take_folder,
            commands::unpack_to_existing_tracks,
            commands::export_active_take,
            commands::pack_take_folder,
            commands::move_regions,
            commands::clear_undo_history,
            commands::set_tempo_map,
            commands::set_signatures,
            commands::set_tempo_at,
            commands::set_signature_at,
            commands::move_midi_with_tempo,
            commands::list_builtin_effects,
            commands::list_builtin_instruments,
            commands::set_builtin_param,
            commands::builtin_params,
            commands::set_sidechain,
            commands::watch_builtin,
            commands::copy_plugin,
            commands::paste_plugin,
            commands::insert_channel_eq,
            commands::list_presets,
            commands::get_preset,
            commands::load_preset,
            commands::save_preset,
            commands::delete_preset,
            commands::rename_preset,
            commands::move_plugin,
            commands::set_region_fade_shape,
            commands::crossfade_regions,
            commands::set_drag_mode,
            commands::reset_clip_indicators,
            commands::copy_strip,
            commands::paste_strip,
            commands::reset_strip,
            commands::list_strip_settings,
            commands::save_strip_setting,
            commands::load_strip_setting,
            commands::delete_strip_setting,
            commands::create_vca,
            commands::delete_vca,
            commands::update_vca,
            commands::assign_vca,
            commands::set_vca_parent,
            commands::set_sidechain_pre,
            windows::open_panel_window,
            windows::close_panel_window,
            windows::focus_panel_window,
            windows::broadcast_snapshot,
            commands::set_region_params,
            commands::apply_region_params,
            commands::marquee_split,
            commands::marquee_delete,
            commands::marquee_copy,
            commands::controller_assignments,
            commands::start_learn,
            commands::cancel_learn,
            commands::focus_plugin,
            commands::update_controller_assignment,
            commands::set_controller_feedback,
            commands::delete_controller_assignment,
            commands::project_files,
            commands::new_alternative,
            commands::switch_alternative,
            commands::rename_alternative,
            commands::delete_alternative,
            commands::revert_to_saved,
            commands::revert_to_backup,
            commands::save_template,
            commands::user_templates,
            commands::delete_template,
            commands::rename_template,
            commands::new_from_template,
            commands::new_track_alternative,
            commands::switch_track_alternative,
            commands::rename_track_alternative,
            commands::delete_track_alternative,
            commands::create_pattern_region,
            commands::set_pattern,
            commands::convert_patterns_to_midi,
            commands::convert_midi_to_patterns,
            commands::set_snap_zero_crossings,
            commands::set_region_gains,
            commands::create_comp,
            commands::duplicate_comp,
            commands::select_comp,
            commands::rename_comp,
            commands::delete_comp,
            commands::set_loop_crossfade,
            menu::project_menus::set_project_menus,
            menu::set_menu_state,
            menu::gtk_keys::gtk_key_names,
            schemas::file_dialog_problem,
            commands::set_marker_length,
            commands::add_arrangement_marker,
            commands::rename_arrangement_marker,
            commands::set_arrangement_marker_color,
            commands::resize_arrangement_marker,
            commands::move_arrangement_marker,
            commands::delete_arrangement_marker,
            commands::set_auto_lanes,
            commands::set_builtin_params,
            commands::sampler_view,
            commands::sampler_waveform,
            commands::sampler_update_zones,
            commands::sampler_delete_zones,
            commands::sampler_add_files,
            commands::sampler_load_sfz,
            commands::sampler_slice,
            commands::sampler_replace_file,
            commands::sampler_record_start,
            commands::sampler_record_status,
            commands::sampler_record_stop,
            commands::sampler_record_cancel,
            commands::convert_region_to_sampler,
            commands::drum_hits,
            commands::drum_replace,
            commands::section_edit,
            commands::insert_cut_section,
            commands::remove_gaps,
            commands::remove_overlaps,
            commands::trim_to_next_region,
            commands::fill_within_locators,
            commands::resize_to_same_length,
            commands::trim_regions_to,
            commands::crop_regions,
            commands::delete_and_shuffle,
            commands::paste_replace,
            commands::move_regions_to,
            commands::strip_silence_preview,
            commands::strip_silence,
            commands::name_regions_by_track,
            commands::name_tracks_by_region,
            commands::color_regions_by_track,
            commands::color_tracks_by_region,
            commands::move_regions_to_new_tracks,
            commands::copy_regions_to_new_tracks,
            commands::add_output_aux,
            commands::app_version,
            commands::open_release_page,
            commands::paste_plugin_settings,
            commands::set_track_midi,
            diagnostics::window::open_diagnostics_window,
            diagnostics::diagnostics,
            diagnostics::reset_diagnostics,
            diagnostics::activity::activity_log,
            diagnostics::activity::set_activity_log,
            diagnostics::activity::log_activity,
            diagnostics::activity::show_activity_logs,
        ]))
        .run(tauri::generate_context!())
        .expect("error while running Marbble");
}

/// The commands, each written to the activity log on its way in (diagnostics/activity.rs).
fn logged<R: tauri::Runtime>(
    handler: impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        diagnostics::activity::command(invoke.message.command(), invoke.message.payload());
        handler(invoke)
    }
}
