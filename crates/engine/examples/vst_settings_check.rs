//! Copy and Paste Settings and Compare for VST3s (their slot's right-click menu), with LSP's
//! Compressor: pasting loads the copied state into the running instance as one undo step, and
//! Compare swaps between the settings as last saved and the edited ones, lit by the edits Marbble
//! sees. Each change is read back from the controller and heard in a render. Prints SKIP when the
//! plug-in isn't installed.
//! usage: vst_settings_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;
use vst3_host::{Gesture, PluginDescriptor};

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Runs the idle loop for a while, as the app does every 30 ms.
fn pump(s: &mut Session, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        s.idle();
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// The level of one track's sound once the plug-in has settled, in dB.
fn level(s: &mut Session, track: Id, len: i64) -> f32 {
    for t in s.project.tracks.iter().map(|t| t.id).collect::<Vec<_>>() {
        s.update_track(t, TrackPatch { mute: Some(t != track), ..Default::default() });
    }
    let r = s.render(0, len, 0, None, &mut |_| {}).unwrap();
    let peak = r.iter().flat_map(|c| &c[(len / 2) as usize..]).fold(0.0f32, |m, x| m.max(x.abs()));
    20.0 * peak.max(1e-9).log10()
}

/// A drag in the editor to `to`, one value per idle, and the pause after it that makes its step.
fn drag(s: &mut Session, uid: Id, param: u32, to: f64) {
    let from = s.vst_param(uid, param).unwrap();
    s.drive_vst_handler(uid, Gesture::Begin(param));
    for i in 1..=10 {
        s.drive_vst_handler(uid, Gesture::Perform(param, from + (to - from) * i as f64 / 10.0));
        pump(s, 15);
    }
    s.drive_vst_handler(uid, Gesture::End(param));
    pump(s, 300);
}

fn near(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-3)
}

/// (lit, pressed) of an instance's Compare.
fn compare(s: &Session, uid: Id) -> (bool, bool) {
    let c = s.compare_states();
    (c.edited.contains(&uid), c.comparing.contains(&uid))
}

/// What a press of Compare in the slot menu runs (`compare_vst`).
fn press_compare(s: &mut Session, uid: Id) -> bool {
    let pushed = s.checkpoint("Compare", None, false);
    let r = s.toggle_vst_compare(uid);
    if r.is_err() && pushed {
        s.abandon_checkpoint();
    }
    pump(s, 50);
    r.is_ok()
}

/// What Paste Settings in the slot menu runs (`paste_plugin_settings`).
fn paste(s: &mut Session, track: Id, copy: &daw_engine::model::PluginSlot) -> Result<(), String> {
    let pushed = s.checkpoint("Paste Settings", None, false);
    let r = s.paste_plugin_settings(track, 0, copy).map_err(|e| e.to_string());
    if r.is_err() && pushed {
        s.abandon_checkpoint();
    }
    pump(s, 50);
    r
}

fn run(desc: &PluginDescriptor, other: Option<&PluginDescriptor>, tone: &Path, scratch: &Path) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, [Some(desc.clone()), other.cloned()].into_iter().flatten().collect());
    s.dir = scratch.join("vst_settings.marbble");
    std::fs::create_dir_all(&s.dir).unwrap();
    let len = daw_engine::audio_file::decode::decode(tone).unwrap().channels[0].len() as i64;
    let mut strip = || {
        let track = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        s.import_audio(tone, track, 0).unwrap();
        let uid = s.set_plugin(track, 0, &desc.id).unwrap();
        (track, uid)
    };
    let (a, a_uid) = strip();
    let (b, b_uid) = strip();
    s.set_cycle(true, 0, len);
    let id = s.plugin_params(a_uid).iter().find(|p| p.name == "Output gain").map(|p| p.id).expect("Output gain");
    let v0 = s.vst_param(a_uid, id).unwrap();
    let (v1, v2) = (v0 * 0.5, v0 * 0.25);
    pump(&mut s, 400);
    let l0 = level(&mut s, b, len);

    // ---- Copy and Paste Settings ---------------------------------------------------------------
    s.edit_vst_param(a_uid, id, v1);
    pump(&mut s, 100);
    let copy = s.copy_plugin(a, 0).unwrap();
    let steps = s.undo_depths().0;
    let pasted = paste(&mut s, b, &copy);
    let l1 = level(&mut s, b, len);
    check(
        &format!("Paste Settings loads the copy into the running instance ({l0:.2} -> {l1:.2} dB): {pasted:?}"),
        pasted.is_ok() && s.slot_get(b, 0).map(|p| p.uid) == Some(b_uid) && near(s.vst_param(b_uid, id), v1) && (l1 - l0).abs() > 1.0,
    );
    check("it's one undo step, labelled Paste Settings", s.undo_depths().0 == steps + 1 && s.undo_label().as_deref() == Some("Paste Settings"));
    check("the project's copy of the state is the pasted one", s.copy_plugin(b, 0).unwrap().state == copy.state);
    check("pasting into an instance whose editor never opened makes Compare light", compare(&s, b_uid) == (true, false));
    s.undo();
    pump(&mut s, 100);
    let l = level(&mut s, b, len);
    check(
        &format!("undo puts back the settings it had, same instance ({l:.2} dB)"),
        near(s.vst_param(b_uid, id), v0) && s.slot_get(b, 0).map(|p| p.uid) == Some(b_uid) && (l - l0).abs() < 0.05,
    );
    check("and Compare goes out again", compare(&s, b_uid) == (false, false));
    s.redo();
    pump(&mut s, 100);
    check("redo pastes them again", near(s.vst_param(b_uid, id), v1));

    let mut far = copy.clone();
    far.plugin_id = format!("/Library/Audio/Plug-Ins/VST3/elsewhere.vst3|{}", desc.cid.to_lowercase());
    s.edit_vst_param(b_uid, id, v0);
    pump(&mut s, 100);
    check("a copy made where the bundle is elsewhere pastes by its class", paste(&mut s, b, &far).is_ok() && near(s.vst_param(b_uid, id), v1));
    if let Some(other) = other {
        let mut wrong = copy.clone();
        wrong.plugin_id = other.id.clone();
        let steps = s.undo_depths().0;
        let r = paste(&mut s, b, &wrong);
        check(&format!("another plug-in's settings are refused, leaving no step: {r:?}"), r.is_err() && s.undo_depths().0 == steps);
    }
    let mut empty = copy.clone();
    empty.state = None;
    check("a copy without a state is refused", paste(&mut s, b, &empty).is_err());

    // ---- Compare -------------------------------------------------------------------------------
    s.edit_vst_param(a_uid, id, v0);
    pump(&mut s, 400);
    s.watch_vst_edits_headless(a_uid);
    check("Compare is out while the settings are as the editor found them", compare(&s, a_uid) == (false, false));
    check("and a press does nothing", !press_compare(&mut s, a_uid) && near(s.vst_param(a_uid, id), v0));
    drag(&mut s, a_uid, id, v1);
    check("an edit in the editor lights it", compare(&s, a_uid) == (true, false));
    s.undo();
    pump(&mut s, 300);
    check("undoing the edit puts it out", compare(&s, a_uid) == (false, false) && near(s.vst_param(a_uid, id), v0));
    s.redo();
    pump(&mut s, 300);
    check("redoing it lights it", compare(&s, a_uid) == (true, false) && near(s.vst_param(a_uid, id), v1));

    let la = level(&mut s, a, len);
    let steps = s.undo_depths().0;
    let pressed = press_compare(&mut s, a_uid);
    let l = level(&mut s, a, len);
    check(
        &format!("a press plays the saved settings ({la:.2} -> {l:.2} dB), pressed, one step"),
        pressed && near(s.vst_param(a_uid, id), v0) && compare(&s, a_uid) == (false, true) && s.undo_depths().0 == steps + 1 && (l - l0).abs() < 0.05,
    );
    pump(&mut s, 400);
    check("the plug-in's answer to it makes no step", s.undo_depths().0 == steps + 1);
    press_compare(&mut s, a_uid);
    let l = level(&mut s, a, len);
    check(
        &format!("another goes back to the edited ones ({l:.2} dB), lit"),
        near(s.vst_param(a_uid, id), v1) && compare(&s, a_uid) == (true, false) && (l - la).abs() < 0.05,
    );
    press_compare(&mut s, a_uid);
    s.undo();
    pump(&mut s, 300);
    check("undoing a press to the saved settings brings back the edited ones, lit", near(s.vst_param(a_uid, id), v1) && compare(&s, a_uid) == (true, false));
    s.redo();
    pump(&mut s, 300);
    check("redoing it plays the saved ones again", near(s.vst_param(a_uid, id), v0) && compare(&s, a_uid).1);

    drag(&mut s, a_uid, id, v2);
    check("editing while comparing makes those the edited settings", compare(&s, a_uid) == (true, false));
    press_compare(&mut s, a_uid);
    check("so a press plays the saved ones", near(s.vst_param(a_uid, id), v0) && compare(&s, a_uid) == (false, true));
    press_compare(&mut s, a_uid);
    check("and the next the latest edit", near(s.vst_param(a_uid, id), v2) && compare(&s, a_uid) == (true, false));

    // Pasting the saved settings back is no change.
    s.edit_vst_param(b_uid, id, v0);
    pump(&mut s, 400);
    let saved = s.copy_plugin(b, 0).unwrap();
    s.watch_vst_edits_headless(b_uid);
    let _ = paste(&mut s, b, &saved);
    check("pasting settings byte for byte the saved ones leaves Compare out", compare(&s, b_uid) == (false, false));

    s.remove_plugin(a, 0);
    pump(&mut s, 50);
    check("a removed instance's Compare is forgotten", !s.compare_states().edited.contains(&a_uid) && !s.compare_states().comparing.contains(&a_uid));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = PathBuf::from(args.next().expect("audio file"));
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let all: Vec<PluginDescriptor> = vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths())
        .iter()
        .flat_map(|b| vst3_host::scan::scan_bundle(b).unwrap_or_default())
        .collect();
    let find = |n: &str| all.iter().find(|d| d.name == n).cloned();
    match find("Compressor Stereo") {
        Some(d) => run(&d, find("Limiter Stereo").as_ref(), &tone, &scratch),
        None => println!("SKIP LSP's Compressor Stereo isn't installed"),
    }
    std::process::exit(FAILED.load(Ordering::Relaxed) as i32);
}
