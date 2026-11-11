//! Chase: Notes and Controllers against a running engine (silent ALSA null output) through the
//! built-in synth: playback started in the middle of a long note sounds at once, a jump into one
//! does too, and neither does with Chase: Notes off; a controller set before the playhead is in
//! effect from the start, unless Chase: Controllers is off. usage: chase_check <scratch dir>
use daw_engine::model::{CcEvent, CcKind, Id, InputSel, Note, TrackKind};
use daw_engine::session::{MidiContent, Tick};
use daw_engine::{audio_io::AudioConfig, Session};

#[path = "common/transport.rs"]
mod transport;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// The track's peak level in a tick.
fn level(t: &Tick, id: Id) -> f32 {
    transport::levels(t, &[id]).0[0]
}

/// Plays from `from` (with the cycle kept inside the note, so no note starts on its own) and
/// returns the level: once it sounds, or one window's when `sound` isn't expected.
fn play_from(s: &mut Session, id: Id, from: i64, sound: bool) -> f32 {
    let (lv, _) = transport::measure(s, from, &[id], |lv, _| !sound || lv[0] > 0.01);
    lv[0]
}

/// Plays from `away` (past every note: silent), then jumps to `to` while playing. Returns the
/// level before the jump, and after it: once it sounds, or one window's when `sound` isn't
/// expected. The cycle is off meanwhile: the engine would wrap a playhead
/// past its end straight back to its start.
fn jump(s: &mut Session, id: Id, away: i64, to: i64, sound: bool) -> (f32, f32) {
    let c = s.project.cycle.clone();
    s.set_cycle(false, c.start, c.end);
    let before = level(&transport::play_window(s, away), id);
    // The meters hold each strip's peak since the last tick, which `play_window` just read: from
    // here on they hold the jump and what follows it.
    s.locate(to);
    let after = level(&transport::tick_until(s, |t| !sound || level(t, id) > 0.01), id);
    transport::stop(s);
    s.set_cycle(c.enabled, c.start, c.end);
    (before, after)
}

fn set_chase(s: &mut Session, notes: bool, controllers: bool) {
    let mut p = s.prefs.clone();
    p.midi.chase_notes = notes;
    p.midi.chase_controllers = controllers;
    s.set_prefs(p);
}

fn main() {
    let scratch = std::path::PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    std::fs::create_dir_all(&scratch).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("ChaseCheck.marbble");
    let bar = s.project.samples_per_bar().round() as i64;
    let beat = s.project.samples_per_beat().round() as i64;

    // One note held through four bars; the cycle loops bars 2 and 3, inside it.
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.select_track(Some(inst));
    let r = s.create_midi_region(inst, 0, 4 * bar).unwrap();
    let pad = Note { start: 0, length: 4 * bar, pitch: 48, velocity: 100, muted: false };
    s.set_midi_content(vec![MidiContent { region: r, notes: vec![pad], cc: vec![], start: None, length: None }]);
    s.set_cycle(true, bar, 3 * bar);
    let mid = bar + beat;

    check("chase is on by default", s.prefs.midi.chase_notes && s.prefs.midi.chase_controllers);
    let lv = play_from(&mut s, inst, mid, true);
    check(&format!("starting inside a long note sounds ({lv:.3})"), lv > 0.01);
    let (before, after) = jump(&mut s, inst, 6 * bar, mid, true);
    check(&format!("past the note it is silent ({before:.4})"), before < 1e-4);
    check(&format!("jumping into the note sounds ({after:.3})"), after > 0.01);

    set_chase(&mut s, false, true);
    let lv = play_from(&mut s, inst, mid, false);
    check(&format!("with Chase: Notes off it is silent ({lv:.4})"), lv < 1e-4);
    let (_, after) = jump(&mut s, inst, 6 * bar, mid, false);
    check(&format!("and a jump into the note is silent too ({after:.4})"), after < 1e-4);
    set_chase(&mut s, true, true);

    // A bounce that starts inside the note chases it too, as playback does.
    let level = |a: &[Vec<f32>]| a[0].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let bounced = level(&s.render(mid, mid + bar, 0, None, &mut |_| {}).unwrap());
    check(&format!("a bounce starting inside the note sounds it ({bounced:.3})"), bounced > 0.01);
    set_chase(&mut s, false, true);
    let bounced = level(&s.render(mid, mid + bar, 0, None, &mut |_| {}).unwrap());
    check(&format!("but not with Chase: Notes off ({bounced:.4})"), bounced < 1e-4);
    set_chase(&mut s, true, true);

    // Expression 0 set at the region start silences notes that start inside the cycle, but
    // only when it is chased.
    let beats: Vec<Note> = (4..12).map(|i| Note { start: i * beat, length: beat / 2, pitch: 60, velocity: 100, muted: false }).collect();
    let quiet = vec![CcEvent { start: 0, kind: CcKind::Controller(11), value: 0 }];
    s.set_midi_content(vec![MidiContent { region: r, notes: beats, cc: quiet, start: None, length: None }]);
    // Off first: the synth keeps a controller value it was sent until something changes it.
    set_chase(&mut s, true, false);
    let lv = play_from(&mut s, inst, bar, true);
    check(&format!("with Chase: Controllers off the notes play ({lv:.3})"), lv > 0.01);
    set_chase(&mut s, true, true);
    let lv = play_from(&mut s, inst, bar, false);
    check(&format!("a chased expression of 0 silences ({lv:.4})"), lv < 1e-4);

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
