//! A render starts from silence and leaves none of its sound behind (#490), measured with a
//! plug-in whose voice outlasts its note-off: LSP's Multi-Sampler plays its sample to the end,
//! here a 5 s one under a 20 ms note (`common/multi_sampler.rs`). Neither what played live before
//! a render, nor what a render played, may ring into what comes next; delay compensation (a
//! compressor's lookahead on another track) mustn't carry it either. Prints what playing the
//! tails out costs. Prints SKIP when the plug-in isn't installed.
//! usage: vst_tails_check <scratch dir>
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use daw_engine::audio_io::AudioConfig;
use daw_engine::dsp::Engine;
use daw_engine::model::{Id, InputSel, Note, TrackKind};
use daw_engine::session::MidiContent;
use daw_engine::Session;

#[path = "common/multi_sampler.rs"]
mod multi_sampler;
use multi_sampler::{find_plugin, load_samples, peak, wait_for_samples, NOTE_ONE, PLUGIN};

/// The note, and the range a render plays it in.
const NOTE: i64 = 960;
const LEN: i64 = 12_000;
/// Where nothing plays.
const EMPTY: i64 = 960_000;
const BLOCK: usize = 512;

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Plays `blocks` blocks through the engine as the device would, returning their peak.
fn play(e: &mut Engine, blocks: usize) -> f32 {
    let mut out = vec![0.0f32; BLOCK * 2];
    let mut loudest = 0.0f32;
    for _ in 0..blocks {
        e.process(&[], 0, &mut out, 2);
        loudest = out.iter().fold(loudest, |m, x| m.max(x.abs()));
    }
    loudest
}

fn all(r: &[Vec<f32>]) -> f32 {
    peak(r, (0, i64::MAX))
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let Some(desc) = find_plugin() else {
        println!("SKIP {PLUGIN} (lsp-plugins) isn't installed: nothing measured");
        return;
    };
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, vec![desc.clone()]);
    s.dir = scratch.join("Tails.marbble");
    std::fs::create_dir_all(&s.dir).unwrap();
    let rate = s.project.sample_rate;
    let long = scratch.join("long.wav");
    let x: Vec<f32> = (0..rate * 5).map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin()).collect();
    daw_engine::audio_file::decode::write_wav(&long, rate, &[x.clone(), x]).unwrap();

    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let r = s.create_midi_region(inst, 0, LEN).unwrap();
    let notes = vec![Note { start: 0, length: NOTE, pitch: NOTE_ONE, velocity: 127, muted: false }];
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: vec![], start: None, length: None }]);
    // Another track's lookahead puts the instrument's through a delay line.
    let other: Id = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.set_plugin(other, 0, "builtin:compressor").unwrap();
    s.set_builtin_param(other, 0, daw_engine::fx::compressor::param::LOOKAHEAD, 5.0).unwrap();
    // It loads the sample on a thread of its own, and now and then loses it (issue #37).
    let mut loaded = false;
    for _ in 0..5 {
        s.set_plugin(inst, usize::MAX, &desc.id).unwrap();
        load_samples(&mut s, inst, &long, &long);
        loaded = wait_for_samples(&mut s, inst, (0, NOTE));
        if loaded {
            break;
        }
        println!("     the plug-in lost a sample while loading; loading it again");
    }
    check("the sampler loads its sample", loaded);
    if !loaded {
        std::process::exit(1);
    }
    s.engine_for_checks().lock().process(&[], 0, &mut [], 0);
    s.idle();
    check(&format!("delay compensation is on ({} frames)", s.pdc_total()), s.pdc_total() > 0);

    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    // From here the lock keeps the device's callback out: every block is one of these.
    e.render(0, 1, 0, None, &mut |_| {});
    s.locate(0);
    s.play();
    let live = play(&mut e, 8);
    s.stop();
    let ringing = play(&mut e, 2);
    check(&format!("live, its sample plays on after the note-off and Stop (peak {ringing:.3})"), live > 0.1 && ringing > 0.1);
    let t = Instant::now();
    let first = e.render(EMPTY, EMPTY + LEN, 0, None, &mut |_| {});
    let ringing_cost = t.elapsed();
    check(&format!("a render right after Stop starts from silence (peak {:.2e})", all(&first)), all(&first) < 1e-6);

    let a = e.render(0, LEN, 0, None, &mut |_| {});
    let after = play(&mut e, 40);
    check(&format!("a render plays the note (peak {:.3})", all(&a)), all(&a) > 0.1);
    check(&format!("live playback after it starts without its tail (peak {after:.2e})"), after < 1e-6);
    e.render(0, LEN, 0, None, &mut |_| {});
    let next = e.render(EMPTY, EMPTY + LEN, 0, None, &mut |_| {});
    check(&format!("nor does the next render (peak {:.2e})", all(&next)), all(&next) < 1e-6);
    let again = e.render(0, LEN, 0, None, &mut |_| {});
    check(
        "two renders of the same range are bit-identical",
        a.len() == again.len() && a.iter().zip(&again).all(|(x, y)| x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits())),
    );

    // A short render, with the plug-in quiet (before and after) or ringing (after).
    let t = Instant::now();
    e.render(EMPTY, EMPTY + 1, 0, None, &mut |_| {});
    let quiet_cost = t.elapsed();
    let t = Instant::now();
    e.render(0, 1, 0, None, &mut |_| {});
    println!(
        "     cost of a render of one frame: {quiet_cost:.1?} with the plug-in quiet, {:.1?} with its 5 s sample to play out at the end, {ringing_cost:.1?} at the start too",
        t.elapsed()
    );
    drop(e);

    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}
