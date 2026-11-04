//! Not a check: times `Engine::process` per block at real-time pace on 1 to 6 threads, for a wide
//! project of built-in instruments and effects and for a light one, at 128 and 512 frames. Each
//! row is mean / worst µs per block. "gated" lets the engine decide when the workers help, as the
//! app does; "forced" sends every block through the pool, which shows what the pool itself costs.
//! The workers aren't real time (the null device), so the worst blocks are only a rough guide.
//! usage: parallel_bench [seconds per measurement (default 3)]. Run it with `--release`.
use std::time::{Duration, Instant};

use daw_engine::audio_io::{AudioConfig, NULL_OUTPUT};
use daw_engine::dsp::ParallelTest;
use daw_engine::model::{InputSel, Note, TrackKind};
use daw_engine::session::{MidiContent, BUILTIN_SYNTH};
use daw_engine::Session;

const RATE: u32 = 48000;

fn session(tracks: usize, effects: &[&str]) -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let length = 8 * RATE as i64;
    for k in 0..tracks {
        let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(BUILTIN_SYNTH.into()), None).unwrap();
        for (slot, fx) in effects.iter().enumerate() {
            s.set_plugin(t, slot, fx).unwrap();
        }
        let r = s.create_midi_region(t, 0, length).unwrap();
        let notes = (0..32)
            .flat_map(|i| {
                (0..3).map(move |v| Note {
                    start: i * length / 32,
                    length: length / 40,
                    pitch: 40 + (k as u8 * 3 + v * 4 + i as u8) % 36,
                    velocity: 100,
                    muted: false,
                })
            })
            .collect();
        s.set_midi_content(vec![MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
    }
    s.set_cycle(true, 0, length);
    s.idle();
    s
}

/// Mean and worst µs per block over `secs`, one buffer every buffer's length.
fn measure(s: &mut Session, buffer: usize, secs: f64) -> (f64, f64) {
    let engine = s.engine_for_checks();
    s.locate(0);
    s.play();
    let mut guard = engine.lock();
    let mut out = vec![0.0f32; buffer * 2];
    let period = Duration::from_secs_f64(buffer as f64 / RATE as f64);
    // A second to settle: the gate's average, the workers' first wakes.
    for _ in 0..(RATE as usize / buffer) {
        guard.process(&[], 0, &mut out, 2);
    }
    let blocks = (secs * RATE as f64 / buffer as f64) as usize;
    let (mut total, mut worst) = (0.0f64, 0.0f64);
    let start = Instant::now();
    for b in 0..blocks {
        let t = Instant::now();
        guard.process(&[], 0, &mut out, 2);
        let us = t.elapsed().as_secs_f64() * 1e6;
        total += us;
        worst = worst.max(us);
        let next = start + period * (b as u32 + 1);
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    drop(guard);
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    (total / blocks as f64, worst)
}

fn main() {
    let secs: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let projects: [(&str, usize, &[&str]); 2] =
        [("16 synths, reverb + chorus each", 16, &["builtin:reverb", "builtin:chorus"]), ("2 synths, nothing else", 2, &[])];
    for (name, tracks, effects) in projects {
        let mut s = session(tracks, effects);
        for buffer in [512usize, 128] {
            for (mode, test) in [("gated", ParallelTest::default()), ("forced", ParallelTest { always: true, renders: false, patient: true })] {
                let mut row = format!("{name:34} {buffer:4} {mode:6}");
                for threads in [1u32, 2, 4, 6] {
                    s.set_threads_for_checks(threads, if threads == 1 { ParallelTest::default() } else { test });
                    s.idle();
                    let (mean, worst) = measure(&mut s, buffer, secs);
                    row += &format!(" | {threads}: {mean:6.0} / {worst:6.0}");
                }
                println!("{row}");
            }
        }
    }
}
