//! Plays a saved project with render ahead on, on a real output (a silent sink on Linux:
//! `pactl load-module module-null-sink sink_name=marbble_probe`, output `pipewire:marbble_probe`),
//! prints overloads and render-ahead misses for each step, and dumps the probe trace as CSV.
//! usage: ahead_probe <project dir> <out csv> <output id> <scenario> [select id]
//! scenarios: log, starts, long, twice (FROM=seconds), locates. Environment: HOST (default
//! PipeWire, or the system's own host where there's none, as on macOS), AHEAD_MS (200), THREADS
//! (0 = automatic). The `log` scenario's positions are from one session on a 4.5-minute project.
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::dsp::probe;
use daw_engine::Session;

#[path = "common/transport.rs"]
mod transport;

const SR: f64 = 48_000.0;

fn run_for(s: &mut Session, secs: f64, label: &str) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let mut ticked = Instant::now();
    let (o0, m0) = (s.status.overloads.load(Ordering::Relaxed), s.status.ahead_misses.load(Ordering::Relaxed));
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(30));
        s.idle();
        if ticked.elapsed() >= Duration::from_millis(33) {
            s.tick();
            ticked = Instant::now();
        }
    }
    let (o1, m1) = (s.status.overloads.load(Ordering::Relaxed), s.status.ahead_misses.load(Ordering::Relaxed));
    println!(
        "{:>8.3} {label:<28} load {:>3.0}% overloads +{} misses +{} playing={} pos={:.3}s",
        probe::now_us() as f64 / 1e6,
        f32::from_bits(s.status.cpu_load.load(Ordering::Relaxed)) * 100.0,
        o1 - o0,
        m1 - m0,
        s.is_playing(),
        s.status.pos.load(Ordering::Relaxed) as f64 / SR
    );
}

fn a_from() -> f64 {
    std::env::var("FROM").ok().map_or(150.0, |v| v.parse().unwrap())
}

/// Plays for `secs`, starting again from where it stopped whenever an overload stops it, as the
/// owner does.
fn keep_playing(s: &mut Session, secs: f64, label: &str) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let (o0, m0) = (s.status.overloads.load(Ordering::Relaxed), s.status.ahead_misses.load(Ordering::Relaxed));
    let mut ticked = Instant::now();
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(30));
        s.idle();
        if ticked.elapsed() >= Duration::from_millis(33) {
            s.tick();
            ticked = Instant::now();
        }
        if !s.is_playing() {
            mark(999);
            s.play();
            transport::wait_for(s, true);
        }
    }
    let (o1, m1) = (s.status.overloads.load(Ordering::Relaxed), s.status.ahead_misses.load(Ordering::Relaxed));
    println!("{:>8.3} {label:<28} load {:>3.0}% overloads +{} misses +{} pos={:.3}s", probe::now_us() as f64 / 1e6, f32::from_bits(s.status.cpu_load.load(Ordering::Relaxed)) * 100.0, o1 - o0, m1 - m0, s.status.pos.load(Ordering::Relaxed) as f64 / SR);
}

fn mark(code: u64) {
    probe::ev(probe::MARK, code, 0, 0, 0);
}

fn play_from(s: &mut Session, pos: i64, code: u64) {
    if s.is_playing() {
        s.stop();
        transport::wait_for(s, false);
    }
    s.locate(pos);
    mark(code);
    s.play();
    transport::wait_for(s, true);
}

fn stop(s: &mut Session, code: u64) {
    mark(code);
    s.stop();
    transport::wait_for(s, false);
}

fn main() {
    let mut a = std::env::args().skip(1);
    let dir = PathBuf::from(a.next().unwrap());
    let out = PathBuf::from(a.next().unwrap());
    let output = a.next().unwrap();
    let scenario = a.next().unwrap();
    let select: Option<u64> = a.next().map(|v| v.parse().unwrap());
    let ahead_ms: u32 = std::env::var("AHEAD_MS").ok().map_or(200, |v| v.parse().unwrap());
    let threads: u32 = std::env::var("THREADS").ok().map_or(0, |v| v.parse().unwrap());
    probe::init(6_000_000);
    let cfg = AudioConfig {
        host: Some(std::env::var("HOST").unwrap_or_else(|_| "PipeWire".into())),
        output: Some(output),
        input: None,
        input_enabled: false,
        buffer_size: 1024,
        sample_rate: 48_000,
        processing_threads: threads,
        render_ahead_ms: ahead_ms,
        ..Default::default()
    };
    let plugins = daw_engine::library::plugin_db::load_cache().unwrap_or_default();
    let mut s = Session::new(cfg, plugins);
    assert!(s.audio_status.running, "{:?}", s.audio_status.error);
    println!("device {:?} {:?} sr {} buf {}", s.audio_status.host, s.audio_status.output_name, s.audio_status.sample_rate, s.audio_status.buffer_size);
    s.open(&dir).unwrap();
    s.select_track(select.map(|v| v as _));
    println!("tracks {} threads {} ahead {} ms", s.project.tracks.len(), s.processing_threads(), ahead_ms);
    run_for(&mut s, 12.0, "opened, stopped");
    let f = |secs: f64| (secs * SR) as i64;
    match scenario.as_str() {
        // The owner's sequence from the log (song positions).
        "log" => {
            play_from(&mut s, 11_623_168, 1);
            run_for(&mut s, 2.5, "play 4:02.149");
            stop(&mut s, 2);
            run_for(&mut s, 1.7, "stopped");
            let here = s.status.pos.load(Ordering::Relaxed);
            play_from(&mut s, here, 3);
            run_for(&mut s, 20.0, "play 4:04.645 for 20 s");
            stop(&mut s, 4);
            run_for(&mut s, 8.0, "stopped 8 s");
            play_from(&mut s, 12_361_472, 5);
            run_for(&mut s, 4.0, "play 4:17.530");
            stop(&mut s, 6);
            run_for(&mut s, 5.0, "stopped 5 s");
            play_from(&mut s, 12_363_520, 7);
            run_for(&mut s, 4.0, "play 4:17.573");
            stop(&mut s, 8);
            run_for(&mut s, 5.0, "stopped 5 s");
            play_from(&mut s, 11_712_000, 9);
            run_for(&mut s, 30.0, "play 4:04 for 30 s");
            stop(&mut s, 10);
        }
        // Play from a position after being stopped `wait` s, several times.
        "starts" => {
            for (i, (pos, wait)) in [(244.0, 1.5), (244.0, 8.0), (257.5, 1.5), (257.5, 8.0), (120.0, 8.0), (60.0, 1.5), (257.5, 8.0), (244.0, 8.0)].iter().enumerate() {
                run_for(&mut s, *wait, &format!("stopped {wait} s"));
                play_from(&mut s, f(*pos), 100 + i as u64);
                run_for(&mut s, 3.0, &format!("play from {pos} s"));
                stop(&mut s, 200 + i as u64);
            }
        }
        "long" => {
            play_from(&mut s, f(200.0), 1);
            run_for(&mut s, 60.0, "play 200 s for 60 s");
            stop(&mut s, 2);
        }
        // The same passage twice: first touch, then again.
        "twice" => {
            let from = a_from();
            for k in 0..2 {
                play_from(&mut s, f(from), 10 + k);
                keep_playing(&mut s, 25.0, &format!("pass {k} from {from} s"));
                stop(&mut s, 20 + k);
                run_for(&mut s, 3.0, "stopped 3 s");
            }
        }
        // Locates while playing, every 3 s.
        "locates" => {
            play_from(&mut s, f(30.0), 1);
            for (i, to) in [100.0, 150.0, 40.0, 200.0, 120.0, 250.0, 70.0, 180.0].iter().enumerate() {
                keep_playing(&mut s, 3.0, "playing");
                mark(300 + i as u64);
                s.locate(f(*to));
                keep_playing(&mut s, 0.5, &format!("located to {to} s"));
            }
            stop(&mut s, 2);
        }
        _ => panic!("scenario"),
    }
    run_for(&mut s, 1.0, "end");
    println!("total overloads {} misses {}", s.status.overloads.load(Ordering::Relaxed), s.status.ahead_misses.load(Ordering::Relaxed));
    probe::dump(&out);
    drop(s);
}
