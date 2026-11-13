//! Exercises track automation against a running engine (silent ALSA null output): reading
//! volume and pan lanes, Off mode, writing in Write and Touch modes, and undo.
//! usage: automation_check <audio file> <scratch dir> [<plug-in name> <parameter name>]
//! With a plug-in (from the scanned plug-in cache), also automates one of its parameters.
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::edit::automation::value_at;
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

#[path = "common/transport.rs"]
mod transport;

/// Loops `from..to` and returns the track's peak meter (left, right). The null device has no
/// clock, so a short cycle is how to listen to one stretch of the timeline. Every stretch
/// measured here sounds (quietly, at -40 dB, or on one side), so it plays until the track does
/// (`transport::play_until`) and the checks compare the levels.
fn levels_at(s: &mut Session, id: u64, from: i64, to: i64) -> (f32, f32) {
    s.set_cycle(true, from, to);
    let meter = |t: &daw_engine::session::Tick| t.meters.iter().find(|m| m.0 == id).map(|m| (m.1, m.2)).unwrap_or((-1.0, -1.0));
    let t = transport::play_until(s, from, |t| {
        let (l, r) = meter(t);
        l.max(r) > 1e-5
    });
    transport::stop(s);
    meter(&t)
}

fn db(g: f32) -> f32 {
    20.0 * g.max(1e-9).log10()
}

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn pts(v: &[(i64, f32)]) -> Vec<AutoPoint> {
    v.iter().map(|&(pos, value)| AutoPoint { pos, value, curve: 0.0 }).collect()
}

fn lane(s: &Session, id: u64, param: &AutoParam) -> Vec<AutoPoint> {
    s.project.track(id).and_then(|t| t.automation.iter().find(|l| l.param.same(param))).map(|l| l.points.clone()).unwrap_or_default()
}

/// Plays from `from` with the cycle off, running `during` while playing, then stops.
fn pass(s: &mut Session, from: i64, during: impl FnOnce(&mut Session)) {
    s.set_cycle(false, 0, 0);
    s.locate(from);
    s.play();
    transport::wait_for(s, true);
    std::thread::sleep(Duration::from_millis(40));
    during(s);
    s.stop();
    transport::wait_for(s, false);
}

/// Keeps `idle` (the UI's 30 Hz housekeeping) running while time passes.
fn wait(s: &mut Session, ms: u64) {
    for _ in 0..ms / 10 {
        std::thread::sleep(Duration::from_millis(10));
        s.idle();
    }
}

/// Has `idle` write the value being written where the playhead is, and waits (up to a second)
/// for the playhead to move past that, then returns a position read before it: the lane holds
/// the value up to there. Without it a move can land where `idle` last wrote, and the lane then
/// ramps to it from the point before, which on the null device can be seconds of song earlier.
fn hold(s: &mut Session) -> i64 {
    let before = s.position();
    s.idle();
    let (written, begun) = (s.position(), Instant::now());
    while s.position() <= written && s.is_playing() && begun.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(1));
    }
    before
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("AutomationCheck.marbble");

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    let len = s.project.track(a).unwrap().regions[0].length;
    let w = 4800.min(len / 8);
    let (early, late) = ((w, 2 * w), (len - 2 * w, len - w));
    println!("audio: {:?}, region {len} frames", s.audio_status);

    // Read: a volume ramp from -40 dB to 0 dB across the region.
    let (flat, _) = levels_at(&mut s, a, early.0, early.1);
    s.set_automation_lane(a, AutoParam::Volume, pts(&[(0, -40.0), (len, 0.0)]));
    let (e, _) = levels_at(&mut s, a, early.0, early.1);
    let (l, _) = levels_at(&mut s, a, late.0, late.1);
    let rise = db(l) - db(e);
    check(
        &format!("volume ramp: quiet early, loud late ({:.1} dB -> {:.1} dB, flat {:.1} dB)", db(e), db(l), db(flat)),
        rise > 30.0 && (db(flat) - db(e)) > 30.0,
    );

    // Off ignores the lane.
    s.set_auto_mode(a, AutoMode::Off);
    let (e, _) = levels_at(&mut s, a, early.0, early.1);
    check(&format!("Off ignores the lane ({:.1} dB vs flat {:.1} dB)", db(e), db(flat)), (db(e) - db(flat)).abs() < 1.0);
    s.set_auto_mode(a, AutoMode::Read);
    s.clear_automation(a, Some(AutoParam::Volume));
    let (e, _) = levels_at(&mut s, a, early.0, early.1);
    check(&format!("clearing the lane restores the fader ({:.1} dB)", db(e)), (db(e) - db(flat)).abs() < 1.0);

    // Pan from hard left to hard right.
    s.set_automation_lane(a, AutoParam::Pan, pts(&[(0, -1.0), (len, 1.0)]));
    let (el, er) = levels_at(&mut s, a, early.0, early.1);
    let (ll, lr) = levels_at(&mut s, a, late.0, late.1);
    check(
        &format!("pan lane: left early (L {:.1} / R {:.1} dB), right late (L {:.1} / R {:.1} dB)", db(el), db(er), db(ll), db(lr)),
        db(el) - db(er) > 15.0 && db(lr) - db(ll) > 15.0,
    );
    s.clear_automation(a, None);

    // Write: fader moves while playing become a lane, in one undo step. Each value is read back
    // halfway between the playhead just after its move and where `hold` saw it still held: the
    // null device runs the engine far faster than real time, so positions read a moment off from
    // a move can be far from where it landed.
    s.set_auto_mode(a, AutoMode::Write);
    let undo_before = s.undo_label();
    let mut holds = Vec::new();
    pass(&mut s, 0, |s| {
        wait(s, 60);
        for v in [-20.0, -10.0] {
            s.update_track_linked(a, TrackPatch { volume_db: Some(v), ..Default::default() });
            let moved = s.position();
            wait(s, 80);
            holds.push((moved, hold(s)));
        }
    });
    let vol = lane(&s, a, &AutoParam::Volume);
    println!("written volume lane ({} points): {:?} (held {holds:?})", vol.len(), vol.iter().take(12).collect::<Vec<_>>());
    let at = |p: i64| value_at(&vol, p).unwrap_or(f32::NAN);
    let mid1 = (holds[0].0 + holds[0].1) / 2;
    let mid2 = (holds[1].0 + holds[1].1) / 2;
    check(
        &format!("Write records the moves ({:.1} dB at start, {:.1} dB, then {:.1} dB)", at(0), at(mid1), at(mid2)),
        vol.len() >= 3 && (at(0) - 0.0).abs() < 0.01 && (at(mid1) + 20.0).abs() < 0.5 && (at(mid2) + 10.0).abs() < 0.5,
    );
    check("Write is one undo step", s.undo_label().as_deref() == Some("Write Automation"));
    check("Write leaves the track in Latch", s.project.track(a).unwrap().auto_mode == AutoMode::Latch);
    check("untouched pan got no lane", lane(&s, a, &AutoParam::Pan).is_empty());
    s.undo();
    check(
        &format!("undo removes the written lane and restores Write (undo now {:?})", s.undo_label()),
        lane(&s, a, &AutoParam::Volume).is_empty() && s.project.track(a).unwrap().auto_mode == AutoMode::Write && s.undo_label() == undo_before,
    );
    s.redo();
    check("redo brings it back", lane(&s, a, &AutoParam::Volume).len() == vol.len());

    // Touch: writes while held, then returns to the existing curve. Each step is placed by the
    // playhead read on the side of it that bounds it (as Write's moves are): the touch lands at
    // or after `pressed`, the move to -30 dB at or before `held.0`, and the release between
    // `held.1` and `released`.
    s.update_track_linked(a, TrackPatch { volume_db: Some(0.0), ..Default::default() });
    s.set_automation_lane(a, AutoParam::Volume, pts(&[(0, -6.0)]));
    s.set_auto_mode(a, AutoMode::Touch);
    let (mut pressed, mut held, mut released, mut after) = (0, (0, 0), 0, 0);
    pass(&mut s, 0, |s| {
        wait(s, 40);
        pressed = s.position();
        s.automation_touch(a, AutoParam::Volume, true);
        let picked = s.project.track(a).unwrap().volume_db;
        println!("touch picked the fader up at {picked:.1} dB");
        s.update_track_linked(a, TrackPatch { volume_db: Some(-30.0), ..Default::default() });
        held.0 = s.position();
        wait(s, 80);
        held.1 = s.position();
        s.automation_touch(a, AutoParam::Volume, false);
        released = s.position();
        wait(s, 80);
        after = s.position();
    });
    let vol = lane(&s, a, &AutoParam::Volume);
    println!("touched lane: {vol:?} (pressed at {pressed}, held {held:?}, released by {released}, stopped near {after})");
    let at = |p: i64| value_at(&vol, p).unwrap_or(f32::NAN);
    // Where the release landed: the end of the written -30 dB stretch.
    let let_go = vol.iter().rev().find(|p| (p.value + 30.0).abs() < 0.5).map_or(-1, |p| p.pos);
    let ms = |frames: i64| frames as f64 * 1000.0 / s.project.sample_rate as f64;
    // Touch glides back over 0.2 s.
    let back = released + s.project.sample_rate as i64 * 3 / 10;
    check(
        &format!(
            "Touch writes while held and returns ({:.1} before, {:.1} held, {:.1} after; let go at {let_go}, {:.2} ms of song after the playhead read before the release, {:.2} ms before the one after)",
            at(pressed / 2),
            at((held.0 + held.1) / 2),
            at(back),
            ms(let_go - held.1),
            ms(released - let_go),
        ),
        (at(pressed / 2) + 6.0).abs() < 0.01
            && (at((held.0 + held.1) / 2) + 30.0).abs() < 0.5
            && (at(back) + 6.0).abs() < 0.01
            && (held.1..=released).contains(&let_go),
    );

    // Lanes survive undo of an unrelated edit, and reach a rebuilt engine through restore().
    s.checkpoint("Rename Track", None, false);
    s.update_track(a, TrackPatch { name: Some("Renamed".into()), ..Default::default() });
    s.undo();
    check("undo of another edit keeps the lane", lane(&s, a, &AutoParam::Volume).len() == vol.len());
    s.set_auto_mode(a, AutoMode::Read);
    s.set_automation_lane(a, AutoParam::Volume, pts(&[(0, -40.0)]));
    s.checkpoint("Delete Track", None, true);
    s.delete_track(a);
    s.undo();
    let (e, _) = levels_at(&mut s, a, early.0, early.1);
    check(&format!("a track brought back by undo plays its lane ({:.1} dB)", db(e)), (db(e) - (db(flat) - 40.0)).abs() < 1.5);

    // Save and load keep the lanes and the mode.
    s.set_auto_mode(a, AutoMode::Latch);
    let saved = s.save(None).unwrap();
    s.open(&saved).unwrap();
    let t = s.project.track(a).unwrap();
    check("save and load keep the automation", t.auto_mode == AutoMode::Latch && lane(&s, a, &AutoParam::Volume) == pts(&[(0, -40.0)]));
    let (e, _) = levels_at(&mut s, a, early.0, early.1);
    check(&format!("a loaded project plays its lane ({:.1} dB)", db(e)), (db(e) - (db(flat) - 40.0)).abs() < 1.5);

    // A plug-in parameter, through the processor's parameter queue.
    fn done() -> ! {
        std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32)
    }
    let (Some(name), Some(pname)) = (args.next(), args.next()) else {
        println!("SKIP plug-in parameter automation: no plug-in and parameter name given");
        done()
    };
    let list = daw_engine::library::plugin_db::load_cache().unwrap_or_default();
    let Some(desc) = list.iter().find(|d| d.name == name) else {
        println!("SKIP plug-in parameter automation: {name} isn't in the plug-in cache (scan plug-ins first)");
        done()
    };
    s.clear_automation(a, None);
    s.set_auto_mode(a, AutoMode::Read);
    s.set_plugins(list.clone());
    let uid = s.set_plugin(a, 0, &desc.id).unwrap();
    let params = s.plugin_params(uid);
    println!("{name}: {} automatable parameters, e.g. {:?}", params.len(), params.iter().take(10).map(|p| p.name.as_str()).collect::<Vec<_>>());
    let pi = params.iter().find(|p| p.name.to_lowercase().contains(&pname.to_lowercase())).expect("no such parameter");
    let param = AutoParam::Plugin { uid, param: pi.id, name: pi.name.clone() };
    s.set_automation_lane(a, param.clone(), pts(&[(0, 0.0)]));
    let (lo, _) = levels_at(&mut s, a, early.0, early.1);
    s.set_automation_lane(a, param.clone(), pts(&[(0, 1.0)]));
    let (hi, _) = levels_at(&mut s, a, early.0, early.1);
    check(&format!("automating \"{}\" changes the sound ({:.1} dB at 0%, {:.1} dB at 100%)", pi.name, db(lo), db(hi)), (db(hi) - db(lo)).abs() > 3.0);
    s.set_auto_mode(a, AutoMode::Off);
    let (off, _) = levels_at(&mut s, a, early.0, early.1);
    println!("with automation off: {:.1} dB (the plug-in keeps the last value it was sent)", db(off));
    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
