//! Drum Replacement and Doubling through the real session (silent ALSA null output): a recorded
//! kick pattern of ringing kicks at uneven levels, some on top of the last one's ringing, becomes
//! a sampler track whose notes land within a millisecond of each hit with velocities that follow
//! the hits' levels; the render plays the made sample at those places; Replace mutes the
//! recording and Doubling doesn't; one undo takes it back; and it survives saving and reopening.
//!
//! usage: drum_check <scratch dir>
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use daw_engine::drum_replace::{DrumKind, DrumMode, DrumSettings, DrumTiming};
use daw_engine::instrument::sampler::ID as SAMPLER;
use daw_engine::model::{Id, InputSel, Note, RegionContent, TrackKind};
use daw_engine::{audio_io::AudioConfig, transients, Session};

const RATE: i64 = 48000;
/// The region's place on the timeline: bar 2 at 120 bpm.
const AT: i64 = RATE * 2;

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn session() -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

/// A ringing kick as a kick mic hears it: a beater's click, then a body sweeping from 160 Hz down
/// to 50 that rings for a third of a second.
fn kick(buf: &mut [f32], at: usize, amp: f32) {
    let mut seed = 7u32;
    let mut phase = 0.0f32;
    for i in 0..(buf.len() - at) {
        let t = i as f32 / RATE as f32;
        phase += std::f32::consts::TAU * (50.0 + 110.0 * (-t / 0.02).exp()) / RATE as f32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let click = if i < 72 { ((seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * 0.5 * (1.0 - i as f32 / 72.0) } else { 0.0 };
        buf[at + i] += amp * ((1.0 - (-t / 0.0007).exp()) * (-t / 0.33).exp() * phase.sin() + click);
    }
}

fn write(path: &Path, ch: &[f32]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE as u32, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for &v in ch {
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

fn notes_of(s: &Session, t: Id) -> (i64, Vec<Note>) {
    let r = &s.project.track(t).unwrap().regions[0];
    match &r.content {
        RegionContent::Midi { notes, .. } => (r.start, notes.clone()),
        _ => panic!("not MIDI"),
    }
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    std::env::set_var("HOME", scratch.join("home"));
    std::env::set_var("XDG_CONFIG_HOME", scratch.join("home/config"));
    std::env::set_var("XDG_DATA_HOME", scratch.join("home/data"));
    let mut s = session();
    let project = scratch.join("Drums.marbble");
    s.save(Some(project.clone())).unwrap();

    println!("-- A kick mic's take");
    // Two bars of eighths and sixteenths at 120 bpm, with ghost notes, a double on the sixteenth
    // (on the first kick's ringing), and a quiet bleed of something else at -40 dB.
    let beat = RATE as usize / 2;
    let hits: Vec<(usize, f32)> = vec![
        (2400, 0.9),
        (2400 + beat * 3 / 2, 0.45),
        (2400 + beat * 2, 0.8),
        (2400 + beat * 9 / 4, 0.3),
        (2400 + beat * 7 / 2, 0.65),
        (2400 + beat * 4, 0.95),
        (2400 + beat * 11 / 2, 0.2),
        (2400 + beat * 6, 0.85),
        (2400 + beat * 6 + beat / 4, 0.6),
        (2400 + beat * 7, 0.7),
    ];
    let mut take = vec![0.0f32; beat * 9];
    for &(a, g) in &hits {
        kick(&mut take, a, g);
    }
    kick(&mut take, 2400 + beat * 5, 0.012);
    write(&scratch.join("originals/Kick In.wav"), &take);
    let kick_track = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(&scratch.join("originals/Kick In.wav"), kick_track, AT).unwrap();
    let recorded = s.render(0, AT + take.len() as i64, 0, None, &mut |_| {}).unwrap();

    println!("-- Finding the hits");
    let sources = s.drum_sources(kick_track, &[]).unwrap();
    let found = daw_engine::drum_replace::find_hits(&sources, RATE as u32);
    let kept = daw_engine::drum_replace::kept(&found, -30.0);
    check(&format!("{} hits past -30 dB, as many as were played", kept.len()), kept.len() == hits.len());
    let worst = kept.iter().zip(&hits).map(|(h, &(a, _))| (h.pos - (AT + a as i64)).abs()).max().unwrap_or(i64::MAX);
    check(&format!("each at its attack, {:.2} ms off at worst", worst as f64 * 1000.0 / RATE as f64), worst <= RATE / 1000);
    let bleed = AT + 2400 + beat as i64 * 5;
    check("the bleed on a kick's ringing isn't a hit", kept.iter().all(|h| (h.pos - bleed).abs() > RATE / 200));

    println!("-- Replace");
    s.checkpoint("Drum Replacement", None, false);
    let settings = DrumSettings { instrument: DrumKind::Kick, mode: DrumMode::Replace, threshold_db: -30.0, timing: DrumTiming::Attack, offset_ms: 0.0 };
    let new = s.drum_replace(kick_track, &[], &settings).unwrap();
    let pos = |s: &Session, id: Id| s.project.tracks.iter().position(|t| t.id == id).unwrap();
    let nt = s.project.track(new).unwrap();
    check(
        "a sampler track right below the recording",
        pos(&s, new) == pos(&s, kick_track) + 1 && nt.kind == TrackKind::Instrument && nt.instrument.as_ref().unwrap().plugin_id == SAMPLER,
    );
    check("the recording is muted", s.project.track(kick_track).unwrap().mute);
    check("the kick sample is in the project", project.join("Samples/Drum Replacement/Kick.wav").is_file());
    let (start, notes) = notes_of(&s, new);
    check("the MIDI region starts with the audio region", start == AT);
    let worst = notes.iter().zip(&hits).map(|(n, &(a, _))| (start + n.start - (AT + a as i64)).abs()).max().unwrap_or(i64::MAX);
    check(
        &format!("{} notes on C1, within {:.2} ms of the hits", notes.len(), worst as f64 * 1000.0 / RATE as f64),
        notes.len() == hits.len() && notes.iter().all(|n| n.pitch == 36) && worst <= RATE / 1000,
    );
    // Velocity 127 · √(level / loudest): within a few steps of what each hit's level makes it.
    let vel: Vec<u8> = notes.iter().map(|n| n.velocity).collect();
    let want: Vec<u8> = hits.iter().map(|&(_, g)| (127.0 * (g / 0.95f32).sqrt()).round() as u8).collect();
    let off = vel.iter().zip(&want).map(|(&v, &w)| (v as i32 - w as i32).abs()).max().unwrap_or(0);
    check(&format!("velocities follow the hits' levels, {off} off at worst: {vel:?} for {want:?}"), off <= 8 && vel.iter().max() == Some(&127));
    // The sampler plays velocity v at v²: each note's gain against the loudest's is its hit's, for
    // the hits that come out of near silence (on a ringing kick, a hit reads by what it adds).
    let ringing = |i: usize| hits[..i].iter().map(|&(a, g)| g * (-((hits[i].0 - a) as f32 / RATE as f32) / 0.33).exp()).sum::<f32>();
    let worst =
        (0..hits.len()).filter(|&i| ringing(i) < 0.1).map(|i| ((notes[i].velocity as f32 / 127.0).powi(2) - hits[i].1 / 0.95).abs()).fold(0.0f32, f32::max);
    check(&format!("and play as much quieter as they were recorded ({worst:.2} off at worst)"), worst < 0.1);

    let replaced = s.render(0, AT + take.len() as i64, 0, None, &mut |_| {}).unwrap();
    let onsets = transients::detect(&replaced[..1], RATE as u32, 0.5);
    let worst = hits.iter().map(|&(a, _)| onsets.iter().map(|&o| (o - (AT + a as i64)).abs()).min().unwrap_or(i64::MAX)).max().unwrap();
    check(&format!("the render plays the sample at each hit ({:.2} ms off at worst)", worst as f64 * 1000.0 / RATE as f64), worst <= 2 * RATE / 1000);
    let differs = recorded[0].iter().zip(&replaced[0]).any(|(a, b)| (a - b).abs() > 0.05);
    check("and not the recording", differs);

    s.undo();
    check("one undo takes the track away and unmutes the recording", s.project.track(new).is_none() && !s.project.track(kick_track).unwrap().mute);
    s.redo();
    check("redo brings it back", s.project.track(new).is_some() && s.project.track(kick_track).unwrap().mute);
    s.undo();

    println!("-- Doubling, at the peaks, a snare");
    let settings = DrumSettings { instrument: DrumKind::Snare, mode: DrumMode::Double, timing: DrumTiming::Peak, ..settings };
    let double = s.drum_replace(kick_track, &[], &settings).unwrap();
    check("the recording plays on", !s.project.track(kick_track).unwrap().mute);
    let (start, notes) = notes_of(&s, double);
    let late = notes.iter().zip(&hits).all(|(n, &(a, _))| {
        let d = start + n.start - (AT + a as i64);
        d > 0 && d < RATE / 50
    });
    check("its notes are on D1, at the peaks a few ms in", notes.iter().all(|n| n.pitch == 38) && late);
    let doubled = s.render(0, AT + take.len() as i64, 0, None, &mut |_| {}).unwrap();
    let louder = hits.iter().all(|&(a, _)| {
        let w = (AT as usize + a)..(AT as usize + a + 2400);
        let e = |x: &[f32]| x[w.clone()].iter().map(|v| v * v).sum::<f32>();
        e(&doubled[0]) > e(&recorded[0])
    });
    check("both play: every hit is louder than the recording alone", louder);

    println!("-- Saving and reopening");
    s.save(None).unwrap();
    s.open(&project).unwrap();
    let reopened = s.render(0, AT + take.len() as i64, 0, None, &mut |_| {}).unwrap();
    let err = doubled[0].iter().zip(&reopened[0]).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    check(&format!("it plays the same after reopening ({err:.6} apart)"), err < 1e-4);

    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
    println!("all drum replacement checks passed");
}
