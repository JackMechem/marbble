//! Records long streams of MIDI against a running engine (silent ALSA null output), played in
//! through the same path as Musical Typing: far more messages than the ring between the engine
//! and the session holds, all of which reach the take in order while `idle` runs; the same in a
//! cycle, where the pass ends must stay between the right messages; and, with `idle` held off
//! so the ring fills, a warning that counts what was lost.
//! usage: midi_record_check <scratch dir>
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{CcEvent, CcKind, Id, InputSel, RegionContent, TrackKind};
use daw_engine::prefs::MidiOverlap;
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// More than the ring holds (65,536, `RECORDED_MIDI_QUEUE`), so only draining it keeps them all.
const MANY: u32 = 100_000;

/// The `i`th message of a stream: pitch bend carrying `i` (mod 2^14), so order shows.
fn bend(i: u32) -> [u8; 3] {
    [0xE0, (i & 127) as u8, ((i >> 7) & 127) as u8]
}

/// Starts recording and waits until the engine is recording.
fn start(s: &mut Session) {
    s.record().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !s.status.recording.load(Ordering::Relaxed) {
        assert!(Instant::now() < deadline, "the engine never started recording");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Plays messages `0..n` of the stream in batches the live ring and one block can hold, each
/// taken by the engine before the next is sent. With `idle`, the session's idle runs after every
/// batch: the null device runs far faster than real time, so 30 ms of it would bring in more than
/// a performer could play in seconds.
fn play(s: &mut Session, n: u32, idle: bool) {
    const BATCH: u32 = 256;
    let mut sent = 0;
    while sent < n {
        let batch = (n - sent).min(BATCH);
        let before = s.status.midi_activity.load(Ordering::Relaxed);
        for i in sent..sent + batch {
            s.send_midi(bend(i));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while s.status.midi_activity.load(Ordering::Relaxed).wrapping_sub(before) < batch {
            assert!(Instant::now() < deadline, "the engine stopped taking live MIDI");
            std::thread::yield_now();
        }
        sent += batch;
        if idle {
            s.idle();
        }
    }
}

/// Stops, and returns the warning the next tick shows, if any.
fn stop(s: &mut Session) -> Option<String> {
    s.stop();
    s.tick().warning
}

/// The pitch bend values of the track's MIDI, region by region and take by take in order.
fn bends(s: &Session, track: Id) -> (Vec<u16>, usize) {
    let mut out = Vec::new();
    let mut takes = 0;
    let values = |cc: &[CcEvent]| cc.iter().filter(|c| c.kind == CcKind::PitchBend).map(|c| c.value).collect::<Vec<_>>();
    for r in &s.project.track(track).unwrap().regions {
        match (&r.takes, &r.content) {
            (Some(f), _) => {
                takes += f.takes.len();
                f.takes.iter().for_each(|t| out.extend(values(&t.cc)));
            }
            (None, RegionContent::Midi { cc, .. }) => out.extend(values(cc)),
            _ => {}
        }
    }
    (out, takes)
}

fn in_order(values: &[u16], n: u32) -> bool {
    values.len() == n as usize && values.iter().enumerate().all(|(i, &v)| v as u32 == i as u32 & 0x3FFF)
}

fn main() {
    let scratch = std::path::PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    std::fs::create_dir_all(&scratch).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("MidiRecordCheck.marbble");
    s.prefs.recording.pre_roll_secs = 0.0;
    s.set_count_in(false);
    let bar = s.project.samples_per_bar().round() as i64;
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.select_track(Some(inst));
    s.update_track(inst, TrackPatch { armed: Some(true), ..Default::default() });

    // ---- A long take, with idle running as in the app -----------------------------------------
    s.prefs.recording.midi_overlap = MidiOverlap::NewRegion;
    start(&mut s);
    play(&mut s, MANY, true);
    let warning = stop(&mut s);
    let (values, _) = bends(&s, inst);
    check(&format!("all {MANY} messages are in the take, in order ({} recorded)", values.len()), in_order(&values, MANY));
    check(&format!("nothing is reported lost ({warning:?})"), warning.is_none());
    s.undo();

    // ---- In a cycle, each pass a take: the pass ends stay between the right messages ----------
    s.prefs.recording.midi_overlap = MidiOverlap::TakeFolders;
    // Short, so the stream spans many passes.
    s.set_cycle(true, 0, bar / 4);
    start(&mut s);
    play(&mut s, MANY, true);
    let warning = stop(&mut s);
    let (values, takes) = bends(&s, inst);
    // A pass end out of place would put two passes' messages in one take, sorted by time.
    check(&format!("all {MANY} messages are in {takes} takes, in order ({} recorded)", values.len()), takes > 1 && in_order(&values, MANY));
    check(&format!("nothing is reported lost ({warning:?})"), warning.is_none());
    s.undo();
    s.set_cycle(false, 0, bar / 4);

    // ---- Idle held off: the ring fills, and the loss is reported -------------------------------
    s.prefs.recording.midi_overlap = MidiOverlap::NewRegion;
    start(&mut s);
    play(&mut s, MANY, false);
    let warning = stop(&mut s).unwrap_or_default();
    let (values, _) = bends(&s, inst);
    let lost: u32 = warning.split_whitespace().find_map(|w| w.parse().ok()).unwrap_or(0);
    check(&format!("the take keeps what the ring held ({} recorded)", values.len()), !values.is_empty() && in_order(&values, values.len() as u32));
    check(&format!("the warning counts the rest ({warning:?})"), lost > 0 && values.len() as u32 + lost == MANY);
    s.undo();

    // ---- The next take starts the count again ---------------------------------------------------
    start(&mut s);
    play(&mut s, 1000, true);
    let warning = stop(&mut s);
    check(&format!("a take after it loses nothing ({warning:?})"), in_order(&bends(&s, inst).0, 1000) && warning.is_none());

    std::process::exit(FAILED.load(Ordering::Relaxed) as i32);
}
