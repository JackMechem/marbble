//! Not a check of the output: plays the same heavy project at each buffer size and thread count on
//! the real default output device, through the normal audio I/O path (cpal: the device's own
//! callback thread and deadlines), and reports per configuration what went wrong and how loaded
//! the blocks were. Run it by hand to see what multi-core processing buys on a machine, and as a
//! regression gate against limits or a baseline from an earlier run.
//!
//! The project is generated from a seed, so every run plays the same thing: instrument tracks
//! cycling through the built-in instruments, playing seeded chords, each with a Channel EQ, a
//! compressor and one more effect (a reverb on every fourth); audio tracks playing a generated
//! test signal through an EQ, a compressor and a phaser; summing stacks of eight with their own
//! compressor; two aux returns (reverb, delay) fed by sends; a limiter on the master. Only built-in
//! plug-ins, no sample files. It never opens an audio input.
//!
//! Per configuration: `late` blocks took longer than their audio (`EngineStatus::late_blocks`),
//! `gaps` are output callbacks that came after the previous one's audio ran out
//! (`EngineStatus::device_gaps`: the device played something the engine never gave it), `over`
//! are System Overloads (`EngineStatus::overloads`, the transport stops and the check starts it
//! again), and p50 / p99 / max are the blocks' load in % of their deadline
//! (`EngineStatus::block_loads`, to 1 %). `pool` is the share of blocks the workers helped with,
//! `play/rt` the threads playing and the workers granted real-time priority.
//!
//! usage: stress_check [options] [scratch dir]
//!   --null              play on the null device (silent, no clock: as fast as the CPU allows),
//!                       as CI does; the loads are then only a rough guide
//!   --host NAME         the audio host, as in Settings › Audio (default: the app's default)
//!   --output NAME       the output device (default: the host's default)
//!   --project PATH      play this saved project (a .marbble folder) instead of generating one,
//!                       with the plug-ins the app has scanned; --tracks, --audio and --seed are
//!                       then unused
//!   --from S            where to play from, in seconds (default 0)
//!   --stopped           measure with the transport stopped, as when the project sits open
//!   --ahead S           Settings › Audio › Render Ahead: tracks that aren't live rendered S
//!                       seconds ahead (#448), on as many render threads as are processing
//!   --tracks N          instrument tracks (default 32)
//!   --audio N           audio tracks (default 8)
//!   --seed N            the project's seed (default 1)
//!   --buffers A,B,..    buffer sizes (default 64,128,256,512)
//!   --threads A,B,..    processing threads, the callback's own included; `auto` is Settings ›
//!                       Processing Threads › Automatic, resolved as the app resolves it and shown
//!                       as auto(N) (default 1 to what Automatic is here, at least 2, then auto)
//!   --seconds S         how long each configuration is measured (default 20)
//!   --warmup S          played first and not measured (default 2)
//!   --json PATH         write the results there
//!   --max-overloads N   fail if any configuration has more System Overloads
//!   --max-gaps N        fail if any configuration has more device gaps
//!   --max-p99 PCT       fail if any configuration's p99 load is higher
//!   --baseline PATH     fail if a configuration in that earlier --json is beaten: more overloads
//!                       or gaps than it had plus --slack, or a p99 more than --tolerance higher
//!   --slack N           (default 2)
//!   --tolerance PCT     percentage points (default 15)
//! The scratch dir holds the generated test signal (default: a new folder in the temp dir,
//! removed afterwards). It always fails when a configuration played no blocks or one panicked.
//! Run it with `--release`, on a quiet machine: other load shows up in the numbers.
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use daw_engine::audio_io::threads::{Topology, AUTOMATIC};
use daw_engine::audio_io::{AudioConfig, NULL_OUTPUT};
use daw_engine::dsp::EngineStatus;
use daw_engine::model::{Id, InputSel, Note, TrackKind, TrackSend, MASTER_ID};
use daw_engine::session::{MidiContent, TrackPatch};
use daw_engine::Session;
use serde_json::{json, Value};

#[path = "common/transport.rs"]
mod transport;

const INSTRUMENTS: [&str; 7] = ["builtin:synth", "builtin:uno", "builtin:tre", "builtin:cinque", "builtin:sei", "builtin:sedici", "builtin:onda"];
const EFFECTS: [&str; 7] =
    ["builtin:chorus", "builtin:phaser", "builtin:delay", "builtin:overdrive", "builtin:flanger", "builtin:tremolo", "builtin:distortion"];
const BPM: f64 = 120.0;
const BARS: i64 = 8;

struct Opts {
    null: bool,
    host: Option<String>,
    output: Option<String>,
    project: Option<PathBuf>,
    from: f64,
    stopped: bool,
    ahead: Option<f64>,
    tracks: usize,
    audio: usize,
    seed: u64,
    buffers: Vec<u32>,
    /// Thread settings, [`AUTOMATIC`] for `auto`; `None` until the session can tell what Automatic is.
    threads: Option<Vec<u32>>,
    seconds: f64,
    warmup: f64,
    json: Option<PathBuf>,
    max_overloads: Option<u32>,
    max_gaps: Option<u32>,
    max_p99: Option<f64>,
    baseline: Option<PathBuf>,
    slack: u32,
    tolerance: f64,
    scratch: Option<PathBuf>,
}

fn usage(msg: &str) -> ! {
    eprintln!("stress_check: {msg} (see the top of examples/stress_check.rs)");
    std::process::exit(2);
}

fn list<T: std::str::FromStr>(v: &str) -> Vec<T> {
    let l: Vec<T> = v.split(',').map(|x| x.trim().parse().unwrap_or_else(|_| usage(&format!("not a list of numbers: {v}")))).collect();
    if l.is_empty() {
        usage("an empty list");
    }
    l
}

/// Thread settings: numbers, or `auto` for Automatic.
fn thread_list(v: &str) -> Vec<u32> {
    v.split(',')
        .map(|x| match x.trim() {
            "auto" => AUTOMATIC,
            "0" => usage("0 threads is Automatic; write auto"),
            n => n.parse().unwrap_or_else(|_| usage(&format!("not a list of thread counts or auto: {v}"))),
        })
        .collect()
}

fn parse() -> Opts {
    let mut o = Opts {
        null: false,
        host: None,
        output: None,
        project: None,
        from: 0.0,
        stopped: false,
        ahead: None,
        tracks: 32,
        audio: 8,
        seed: 1,
        buffers: vec![64, 128, 256, 512],
        threads: None,
        seconds: 20.0,
        warmup: 2.0,
        json: None,
        max_overloads: None,
        max_gaps: None,
        max_p99: None,
        baseline: None,
        slack: 2,
        tolerance: 15.0,
        scratch: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| usage(&format!("{a} needs a value")));
        let num = |v: String| v.parse::<f64>().unwrap_or_else(|_| usage(&format!("not a number: {v}")));
        match a.as_str() {
            "--null" => o.null = true,
            "--host" => o.host = Some(val()),
            "--output" => o.output = Some(val()),
            "--project" => o.project = Some(val().into()),
            "--from" => o.from = num(val()),
            "--stopped" => o.stopped = true,
            "--ahead" => o.ahead = Some(num(val())),
            "--tracks" => o.tracks = num(val()) as usize,
            "--audio" => o.audio = num(val()) as usize,
            "--seed" => o.seed = num(val()) as u64,
            "--buffers" => o.buffers = list(&val()),
            "--threads" => o.threads = Some(thread_list(&val())),
            "--seconds" => o.seconds = num(val()),
            "--warmup" => o.warmup = num(val()),
            "--json" => o.json = Some(val().into()),
            "--max-overloads" => o.max_overloads = Some(num(val()) as u32),
            "--max-gaps" => o.max_gaps = Some(num(val()) as u32),
            "--max-p99" => o.max_p99 = Some(num(val())),
            "--baseline" => o.baseline = Some(val().into()),
            "--slack" => o.slack = num(val()) as u32,
            "--tolerance" => o.tolerance = num(val()),
            "-h" | "--help" => usage("usage"),
            s if s.starts_with('-') => usage(&format!("unknown option {s}")),
            _ => o.scratch = Some(a.into()),
        }
    }
    o
}

/// xorshift64*: the same numbers on every machine.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn float(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
}

/// A stereo test signal: a slowly beating chord of sines with noise bursts on the beats.
fn write_signal(path: &Path, rate: u32, frames: usize, rng: &mut Rng) {
    let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let beat = (rate as f64 * 60.0 / BPM) as usize;
    let freqs = [110.0f32, 164.8, 220.0, 277.2, 329.6];
    for i in 0..frames {
        let t = i as f32 / rate as f32;
        let tone: f32 = freqs.iter().enumerate().map(|(k, f)| (TAU * f * t).sin() * (0.5 + 0.5 * (TAU * (0.3 + k as f32 * 0.1) * t).sin())).sum::<f32>() * 0.08;
        let since = (i % beat) as f32 / rate as f32;
        let burst = rng.float() * (-since * 30.0).exp() * 0.3;
        w.write_sample(tone + burst).unwrap();
        w.write_sample(tone * 0.9 - burst).unwrap();
    }
    w.finalize().unwrap();
}

/// Builds the project. Returns its length in samples.
fn build(s: &mut Session, o: &Opts, scratch: &Path) -> i64 {
    let rate = s.sample_rate();
    let mut rng = Rng(o.seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    s.set_tempo(BPM);
    let beat = (rate * 60.0 / BPM) as i64;
    let length = BARS * 4 * beat;
    let quiet = |s: &mut Session, t: Id| {
        s.update_track(t, TrackPatch { volume_db: Some(-18.0), ..Default::default() });
    };

    let mut feeders = Vec::new();
    for k in 0..o.tracks {
        let inst = INSTRUMENTS[k % INSTRUMENTS.len()];
        let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(inst.into()), None).unwrap();
        quiet(s, t);
        s.set_plugin(t, 0, "builtin:compressor").unwrap();
        s.set_plugin(t, 1, EFFECTS[(k / INSTRUMENTS.len() + k) % EFFECTS.len()]).unwrap();
        if k % 4 == 3 {
            s.set_plugin(t, 2, "builtin:reverb").unwrap();
        }
        s.insert_channel_eq(t).unwrap();
        // Chords on the half beats, held a beat or two: several voices at once on every track.
        let mut notes = Vec::new();
        let mut at = 0;
        while at < length {
            let root = 36 + rng.below(36) as u8;
            for v in 0..2 + rng.below(3) {
                let len = beat * (2 + rng.below(3) as i64) / 2;
                notes.push(Note {
                    start: at,
                    length: len.min(length - at),
                    pitch: root + [0, 4, 7, 11, 14][v as usize],
                    velocity: 70 + rng.below(50) as u8,
                    muted: false,
                });
            }
            at += beat / 2;
        }
        let r = s.create_midi_region(t, 0, length).unwrap();
        s.set_midi_content(vec![MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
        feeders.push(t);
    }

    let signal = scratch.join("stress signal.wav");
    write_signal(&signal, rate as u32, length as usize, &mut rng);
    for _ in 0..o.audio {
        let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        quiet(s, t);
        s.set_plugin(t, 0, "builtin:compressor").unwrap();
        s.set_plugin(t, 1, "builtin:phaser").unwrap();
        s.insert_channel_eq(t).unwrap();
        s.import_audio(&signal, t, 0).unwrap();
        feeders.push(t);
    }

    let reverb = s.create_aux(Some("Reverb".into())).unwrap();
    s.set_plugin(reverb, 0, "builtin:reverb").unwrap();
    let delay = s.create_aux(Some("Delay".into())).unwrap();
    s.set_plugin(delay, 0, "builtin:delay").unwrap();
    s.insert_channel_eq(delay).unwrap();
    let send = |bus: Id, level_db: f32| Some(TrackSend { bus, level_db, pre_fader: false, muted: false });
    for (i, &t) in feeders.iter().enumerate() {
        s.set_send(t, 0, send(reverb, -12.0)).unwrap();
        if i % 2 == 0 {
            s.set_send(t, 1, send(delay, -15.0)).unwrap();
        }
    }
    for group in feeders.chunks(8).filter(|g| g.len() > 1) {
        let stack = s.create_stack(group, true).unwrap();
        s.set_plugin(stack, 0, "builtin:compressor").unwrap();
    }
    s.set_plugin(MASTER_ID, 0, "builtin:limiter").unwrap();
    s.set_cycle(true, 0, length);
    s.idle();
    length
}

/// The engine's counters, read at the start and end of a measurement.
#[derive(Clone, Copy)]
struct Counters {
    overloads: u32,
    late: u32,
    gaps: u32,
    errors: u32,
    blocks: u64,
    parallel: u64,
}

impl Counters {
    fn read(st: &EngineStatus) -> Counters {
        Counters {
            overloads: st.overloads.load(Ordering::Relaxed),
            late: st.late_blocks.load(Ordering::Relaxed),
            gaps: st.device_gaps.load(Ordering::Relaxed),
            errors: st.engine_errors.load(Ordering::Relaxed),
            blocks: st.blocks.load(Ordering::Relaxed),
            parallel: st.parallel_blocks.load(Ordering::Relaxed),
        }
    }
}

struct Run {
    buffer: u32,
    actual_buffer: u32,
    /// The setting was Automatic; `threads` is what it resolved to.
    automatic: bool,
    threads: u32,
    playing: u32,
    realtime: u32,
    blocks: u64,
    late: u32,
    gaps: u32,
    overloads: u32,
    errors: u32,
    p50: f64,
    p99: f64,
    max: f64,
    pool: f64,
}

impl Run {
    fn json(&self) -> Value {
        json!({
            "buffer": self.buffer, "actual_buffer": self.actual_buffer, "threads": self.threads,
            "automatic": self.automatic,
            "threads_playing": self.playing, "realtime_workers": self.realtime, "blocks": self.blocks,
            "late_blocks": self.late, "device_gaps": self.gaps, "overloads": self.overloads, "engine_errors": self.errors,
            "load_p50": self.p50, "load_p99": self.p99, "load_max": self.max, "pool_share": self.pool,
        })
    }
}

/// Plays for `secs` as the app does: `idle` every 20 ms, a tick every 100 ms (which stops the
/// transport on an overload), and playing again whenever it stopped, unless `stopped`.
fn play_for(s: &mut Session, secs: f64, stopped: bool) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let mut ticked = Instant::now();
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
        s.idle();
        if ticked.elapsed() >= Duration::from_millis(100) {
            s.tick();
            ticked = Instant::now();
        }
        if !stopped && !s.is_playing() {
            transport::wait_for(s, false);
            s.play();
            transport::wait_for(s, true);
        }
    }
}

impl Run {
    /// The thread setting as the table shows it: a number, or auto(N).
    fn threads_label(&self) -> String {
        if self.automatic {
            format!("auto({})", self.threads)
        } else {
            self.threads.to_string()
        }
    }
}

fn measure(s: &mut Session, o: &Opts, buffer: u32, setting: u32) -> Run {
    s.set_audio_config(AudioConfig { processing_threads: setting, ..s.audio_config.clone() });
    // What the session resolves the setting to, Automatic included.
    let threads = s.processing_threads() as u32;
    s.idle();
    let st = s.status.clone();
    s.locate((o.from * s.sample_rate()) as i64);
    if !o.stopped {
        s.play();
        transport::wait_for(s, true);
    }
    play_for(s, o.warmup, o.stopped);
    let before = Counters::read(&st);
    st.block_loads.clear();
    play_for(s, o.seconds, o.stopped);
    let loads = st.block_loads.read();
    let after = Counters::read(&st);
    let health = s.threads_health_for_checks();
    s.stop();
    transport::wait_for(s, false);
    let blocks = after.blocks - before.blocks;
    Run {
        buffer,
        actual_buffer: s.audio_status.buffer_size,
        automatic: setting == AUTOMATIC,
        threads,
        playing: health.map_or(1, |h| h.playing),
        realtime: health.map_or(0, |h| h.realtime),
        blocks,
        late: after.late - before.late,
        gaps: after.gaps - before.gaps,
        overloads: after.overloads - before.overloads,
        errors: after.errors - before.errors,
        p50: loads.percentile(0.5) as f64 * 100.0,
        p99: loads.percentile(0.99) as f64 * 100.0,
        max: loads.max as f64 * 100.0,
        pool: (after.parallel - before.parallel) as f64 / blocks.max(1) as f64,
    }
}

fn print_header() {
    println!(
        "{:>6} {:>7} {:>7} {:>7} {:>6} {:>5} {:>5} {:>6} {:>6} {:>7} {:>5}",
        "buffer", "threads", "play/rt", "blocks", "late", "gaps", "over", "p50%", "p99%", "max%", "pool"
    );
}

fn print_run(r: &Run) {
    let buffer = if r.actual_buffer == r.buffer { r.buffer.to_string() } else { format!("{}*", r.actual_buffer) };
    println!(
        "{:>6} {:>7} {:>7} {:>7} {:>6} {:>5} {:>5} {:>6.0} {:>6.0} {:>7.0} {:>4.0}%",
        buffer,
        r.threads_label(),
        format!("{}/{}", r.playing, r.realtime),
        r.blocks,
        r.late,
        r.gaps,
        r.overloads,
        r.p50,
        r.p99,
        r.max,
        r.pool * 100.0
    );
}

/// What fails the run, one line each.
fn judge(o: &Opts, runs: &[Run]) -> Vec<String> {
    let mut fails = Vec::new();
    let baseline: Option<Value> = o.baseline.as_ref().map(|p| {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| usage(&format!("can't read {}: {e}", p.display())));
        serde_json::from_str(&text).unwrap_or_else(|e| usage(&format!("{} isn't JSON: {e}", p.display())))
    });
    for r in runs {
        let name = format!("buffer {} on {} threads", r.buffer, r.threads_label());
        if r.blocks == 0 {
            fails.push(format!("{name}: played no blocks"));
        }
        if r.errors > 0 {
            fails.push(format!("{name}: {} blocks panicked", r.errors));
        }
        if let Some(m) = o.max_overloads.filter(|&m| r.overloads > m) {
            fails.push(format!("{name}: {} overloads, more than {m}", r.overloads));
        }
        if let Some(m) = o.max_gaps.filter(|&m| r.gaps > m) {
            fails.push(format!("{name}: {} device gaps, more than {m}", r.gaps));
        }
        if let Some(m) = o.max_p99.filter(|&m| r.p99 > m) {
            fails.push(format!("{name}: p99 load {:.0} %, more than {m:.0} %", r.p99));
        }
        let Some(base) = baseline.as_ref() else { continue };
        let earlier = base["runs"].as_array().into_iter().flatten().find(|b| {
            // Automatic is compared with Automatic, whatever it resolved to (on another machine).
            let auto = b["automatic"].as_bool().unwrap_or(false);
            b["buffer"].as_u64() == Some(r.buffer as u64) && auto == r.automatic && (auto || b["threads"].as_u64() == Some(r.threads as u64))
        });
        let Some(b) = earlier else {
            println!("(no baseline for {name})");
            continue;
        };
        let n = |k: &str| b[k].as_f64().unwrap_or(0.0);
        if r.overloads as f64 > n("overloads") + o.slack as f64 {
            fails.push(format!("{name}: {} overloads, the baseline had {}", r.overloads, n("overloads")));
        }
        if r.gaps as f64 > n("device_gaps") + o.slack as f64 {
            fails.push(format!("{name}: {} device gaps, the baseline had {}", r.gaps, n("device_gaps")));
        }
        if r.p99 > n("load_p99") + o.tolerance {
            fails.push(format!("{name}: p99 load {:.0} %, the baseline had {:.0} %", r.p99, n("load_p99")));
        }
    }
    fails
}

fn main() {
    let o = parse();
    let (scratch, made) = match &o.scratch {
        Some(d) => (d.clone(), false),
        None => (std::env::temp_dir().join(format!("marbble-stress-{}", std::process::id())), true),
    };
    std::fs::create_dir_all(&scratch).unwrap();

    // Never an input: playback only, so no microphone prompt and no duplex ring.
    let output = if o.null { Some(NULL_OUTPUT.to_string()) } else { o.output.clone() };
    let host = if o.null { Some("ALSA".to_string()) } else { o.host.clone() };
    let buffer = o.buffers[0];
    let render_ahead_ms = o.ahead.map_or(0, |s| (s * 1000.0).round() as u32);
    let cfg =
        AudioConfig { host, output, input: None, input_enabled: false, buffer_size: buffer, processing_threads: 1, render_ahead_ms, ..Default::default() };
    // A saved project may use any plug-in the app has scanned.
    let plugins = if o.project.is_some() { daw_engine::library::plugin_db::load_cache().unwrap_or_default() } else { Vec::new() };
    let mut s = Session::new(cfg, plugins);
    if !s.audio_status.running {
        eprintln!("stress_check: the audio device didn't start: {:?}", s.audio_status.error);
        std::process::exit(2);
    }
    assert!(s.audio_status.input_name.is_none(), "opened an input");
    let length = match &o.project {
        Some(path) => {
            if let Err(e) = s.open(path) {
                eprintln!("stress_check: couldn't open {}: {e}", path.display());
                std::process::exit(2);
            }
            s.project_end()
        }
        None => build(&mut s, &o, &scratch),
    };
    // What Settings › Processing Threads › Automatic is here, as the app works it out.
    let automatic = s.thread_choices().automatic;
    let settings = o.threads.clone().unwrap_or_else(|| (1..=automatic.max(2)).chain([AUTOMATIC]).collect());
    let topo = Topology::detect();
    let device = s.audio_status.output_name.clone().unwrap_or_default();
    let cores = if topo.fast.is_empty() { format!("{} CPUs", topo.logical) } else { format!("{} CPUs, {} performance cores", topo.logical, topo.fast_cores) };
    let cores = format!("{cores}; Automatic: {automatic}");
    let what = match &o.project {
        Some(p) => format!("{} ({} tracks) from {:.0} s", p.display(), s.project.tracks.len(), o.from),
        None => format!("{} instrument + {} audio tracks (seed {})", o.tracks, o.audio, o.seed),
    };
    let what = if o.stopped { format!("{what}, stopped") } else { what };
    let what = match o.ahead {
        Some(lead) => format!("{what}, rendering {:.0} ms ahead", lead * 1000.0),
        None => what,
    };
    println!(
        "{what}, {:.0} s each after {:.0} s warm-up, on {} ({}, {} Hz); {cores}",
        o.seconds, o.warmup, device, s.audio_status.host, s.audio_status.sample_rate
    );
    if o.null {
        println!("(the null device has no clock: blocks run back to back, so a load over 100 % is no dropout)");
    }
    println!("(* the device gave another buffer size than the one asked for)");
    print_header();

    let mut runs = Vec::new();
    for &buffer in &o.buffers {
        s.set_audio_config(AudioConfig { buffer_size: buffer, ..s.audio_config.clone() });
        if !s.audio_status.running {
            eprintln!("stress_check: the device didn't start at buffer {buffer}: {:?}", s.audio_status.error);
            std::process::exit(2);
        }
        for &setting in &settings {
            let r = measure(&mut s, &o, buffer, setting);
            print_run(&r);
            runs.push(r);
        }
    }

    if let Some(path) = &o.json {
        let doc = json!({
            "version": 1,
            "device": device,
            "host": s.audio_status.host,
            "sample_rate": s.audio_status.sample_rate,
            "null": o.null,
            "project": match &o.project {
                Some(p) => json!({ "path": p, "tracks": s.project.tracks.len(), "from": o.from, "length": length }),
                None => json!({ "instrument_tracks": o.tracks, "audio_tracks": o.audio, "seed": o.seed, "length": length }),
            },
            "seconds": o.seconds,
            "warmup": o.warmup,
            "machine": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "logical_cpus": topo.logical, "performance_cores": topo.fast_cores, "automatic_threads": automatic },
            "runs": runs.iter().map(Run::json).collect::<Vec<_>>(),
        });
        std::fs::write(path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
        println!("wrote {}", path.display());
    }
    if o.ahead.is_some() {
        println!("blocks not rendered ahead in time: {}", s.status.ahead_misses.load(Ordering::Relaxed));
    }
    drop(s);
    if made {
        let _ = std::fs::remove_dir_all(&scratch);
    }

    let fails = judge(&o, &runs);
    for f in &fails {
        println!("FAIL {f}");
    }
    if !fails.is_empty() {
        std::process::exit(1);
    }
    println!("PASS stress_check: {} configurations", runs.len());
}
