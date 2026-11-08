//! Channel strip settings, clip indicators and the strip's gain-reduction meter, against a
//! running engine (silent ALSA null output): a saved setting loads onto two tracks as one undo
//! step, a clipping track holds its peak over 0 dBFS until reset, and a compressor's gain
//! reduction reaches the tick.
//! usage: strip_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::fx::compressor::param;
use daw_engine::model::{Id, InputSel, RegionContent, TrackKind, MASTER_ID};
use daw_engine::session::{RenderOptions, Tick};
use daw_engine::Session;

#[path = "common/transport.rs"]
mod transport;

static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Plays until `ready` accepts a tick (see `transport::play_until`) and returns it.
fn play(s: &mut Session, ready: impl Fn(&Tick) -> bool) -> Tick {
    let t = transport::play_until(s, 0, ready);
    s.stop();
    // play() is ignored until the engine has really stopped.
    transport::wait_for(s, false);
    // As the app's tick thread would, take in what played up to the stop, and what still
    // rings on after it, so a reset after this one starts from silence.
    wait_quiet(s);
    t
}

/// Takes ticks until the engine has played silence for 100 ms, for at most two seconds. A
/// Channel EQ rings on for a few blocks after the tone stops dead at Stop, and on a loaded
/// machine those blocks can come after the first tick: a reset made then would see the ring in
/// the next tick. One silent tick isn't enough, as no block may have run since the last one.
fn wait_quiet(s: &mut Session) {
    let start = Instant::now();
    let mut quiet_since: Option<Instant> = None;
    loop {
        let t = s.tick();
        let quiet = t.master == (0.0, 0.0) && t.meters.iter().all(|m| m.1 == 0.0 && m.2 == 0.0);
        quiet_since = if quiet { quiet_since.or(Some(Instant::now())) } else { None };
        if quiet_since.is_some_and(|q| q.elapsed() >= Duration::from_millis(100)) {
            return;
        }
        if start.elapsed() > Duration::from_secs(2) {
            println!("  (the engine didn't go quiet after Stop: {:?} {:?})", t.master, t.meters);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// For `play`: whether all these strips' meters show sound.
fn sounding(ids: &[Id]) -> impl Fn(&Tick) -> bool + '_ {
    |t| transport::levels(t, ids).0.iter().all(|&l| l > 1e-4)
}

fn held(t: &Tick, id: Id) -> f32 {
    t.peak_holds.iter().find(|h| h.0 == id).map(|h| h.1).unwrap_or(0.0)
}

fn inserts(s: &Session, id: Id) -> Vec<Option<String>> {
    s.project.track(id).unwrap().inserts.iter().map(|x| x.as_ref().map(|x| x.plugin_id.clone())).collect()
}

fn region_of(s: &Session, id: Id) -> Id {
    s.project.track(id).unwrap().regions.iter().find(|r| matches!(r.content, RegionContent::Audio { .. })).unwrap().id
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("StripCheck.marbble");
    // Saved settings go to the scratch folder, never the real config folder.
    let root = scratch.join("channel-strips");
    let _ = std::fs::remove_dir_all(&root);

    let src = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    for t in [src, a, b] {
        s.import_audio(Path::new(&tone), t, 0).unwrap();
    }
    s.set_cycle(true, 0, 48000);

    // ---- a saved channel strip setting onto two tracks -------------------------------------
    s.set_plugin(src, 0, "builtin:compressor").unwrap();
    s.set_builtin_param(src, 0, param::THRESHOLD, -45.0).unwrap();
    s.set_builtin_param(src, 0, param::RATIO, 8.0).unwrap();
    s.set_plugin(src, 2, "builtin:limiter").unwrap();
    let mut eq = s.eq_of(src).unwrap();
    eq.bands[4].on = true;
    eq.bands[4].gain = -6.0;
    s.set_eq(src, eq);
    s.set_plugin(b, 1, "builtin:reverb").unwrap();
    let name = s.save_strip_setting(&root, src, "Squashed", false).unwrap();
    check("the setting is saved as a file", root.join("Squashed.json").exists() && daw_engine::strip_settings::list(&root) == vec![name.clone()]);

    s.checkpoint("Load Channel Strip Setting", None, true);
    s.load_strip_setting(&root, &[a, b], &name).unwrap();
    let want = inserts(&s, src);
    check(&format!("both tracks get the inserts ({:?})", inserts(&s, a)), inserts(&s, a) == want && inserts(&s, b) == want);
    let uids: Vec<Id> = [src, a, b].iter().map(|t| s.project.track(*t).unwrap().inserts[0].as_ref().unwrap().uid).collect();
    check("as new instances with the compressor's values", uids[0] != uids[1] && uids[1] != uids[2] && s.builtin_params(uids[1]) == s.builtin_params(uids[0]));
    check("and the Channel EQ", [a, b].iter().all(|t| s.project.track(*t).unwrap().eq.bands[4].gain == -6.0));

    // The gain-reduction meter: the tone at -21.5 dBFS is well over a -45 dB threshold.
    let t = play(&mut s, sounding(&[src, a, b]));
    let gr = |id: Id| t.gain_reduction.iter().find(|g| g.0 == id).map(|g| g.1);
    check(
        &format!("the compressed strips report gain reduction ({:?} {:?} {:?})", gr(src), gr(a), gr(b)),
        [src, a, b].iter().all(|&id| gr(id).map(|g| g > 3.0).unwrap_or(false)),
    );

    let label = s.undo();
    check(
        &format!("one undo ({label:?}) takes it off both tracks"),
        inserts(&s, a).iter().all(|x| x.is_none()) && inserts(&s, b)[1].as_deref() == Some("builtin:reverb") && inserts(&s, b)[0].is_none(),
    );
    let t = play(&mut s, sounding(&[src]));
    check(
        "a strip without dynamics shows no gain reduction",
        !t.gain_reduction.iter().any(|g| g.0 == a || g.0 == b) && t.gain_reduction.iter().any(|g| g.0 == src),
    );

    // ---- clip indicators ---------------------------------------------------------------------
    // Without its compressor, region gain takes track b's tone 30 dB up: +8.5 dBFS.
    s.remove_plugin(src, 0);
    s.remove_plugin(src, 2);
    // Nothing may ring on after a stop (b's reverb would keep feeding its meter).
    s.remove_plugin(b, 1);
    s.reset_peak_holds(None);
    let r = region_of(&s, b);
    s.set_region_gain(&[r], Some(30.0), None);
    let t = play(&mut s, sounding(&[a, b]));
    let peak_db = 20.0 * held(&t, b).log10();
    check(&format!("a clipping track holds a peak over 0 dBFS ({peak_db:+.1} dBFS)"), held(&t, b) > 1.0);
    check(&format!("a quiet track's hold stays under it ({:+.1} dBFS)", 20.0 * held(&t, a).log10()), held(&t, a) > 0.01 && held(&t, a) < 1.0);
    check("Stereo Out holds its peak too", held(&t, MASTER_ID) > 1.0);

    s.set_region_gain(&[r], Some(0.0), None);
    let t = play(&mut s, sounding(&[b]));
    check(&format!("the hold outlasts the clip, and playing again doesn't reset it ({:+.1} dBFS)", 20.0 * held(&t, b).log10()), held(&t, b) > 1.0);
    s.reset_peak_holds(Some(&[b]));
    let t = s.tick();
    check("resetting one strip clears just its indicator", held(&t, b) == 0.0 && held(&t, a) > 0.0 && held(&t, MASTER_ID) > 1.0);
    let t = play(&mut s, sounding(&[b]));
    check("after a reset the hold follows the new level", held(&t, b) > 0.01 && held(&t, b) < 1.0);
    s.reset_peak_holds(None);
    let t = s.tick();
    check(&format!("resetting all clears every indicator (left: {:?})", t.peak_holds), t.peak_holds.is_empty());

    // A bounce doesn't light the clip indicators.
    s.set_region_gain(&[r], Some(30.0), None);
    let opts = RenderOptions { start: 0, end: 48000, tail_secs: 0.0, ..Default::default() };
    let rendered = s.bounce_project(&scratch.join("bounce.wav"), &opts, &mut |_| {});
    let t = s.tick();
    check(&format!("an offline bounce leaves the indicators alone ({})", rendered.is_ok()), rendered.is_ok() && held(&t, b) == 0.0);

    let _ = std::fs::remove_dir_all(&root);
    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
