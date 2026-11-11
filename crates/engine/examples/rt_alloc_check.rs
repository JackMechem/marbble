//! Real-time safety: plays the all-features project (`common/all_features.rs`: every new
//! built-in effect, VCAs, folder and Stereo Out automation with curves, pattern regions, take
//! folders, region parameters, the Linear Phase EQ) through `Engine::process` on this thread,
//! with a counting allocator, and checks that no block allocates or frees memory once warmed
//! up: while playing, cycling, locating (with note chase), and while edits arrive between
//! blocks, among them a latency change that makes delay compensation glide; and a device
//! buffer far larger than usual through the output callback (`audio_io::callback`) as a device runs it.
//! The silent ALSA null device runs, so edits queue their commands for the next block as they
//! do during playback; holding the engine's lock keeps its callback out.
//! Runs twice: with the callback alone, and with every block on the pool of worker threads
//! (`dsp/pool/`), whose threads are counted for as long as a block can be running. On the pool
//! a plug-in then stops responding on a worker: leaving the worker in it, and its track and what
//! that feeds out of the blocks after, must not allocate either.
//! usage: rt_alloc_check <scratch dir>
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{AutoParam, Id, TrackKind};
use daw_engine::session::{RegionParamsPatch, TrackPatch, VcaPatch};
use daw_engine::Session;

#[path = "common/all_features.rs"]
mod all_features;
use all_features::*;

/// Counts the allocations and frees made on a thread while its `COUNTING` flag is set.
struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
/// Backtraces printed so far (the first few allocations are shown).
static SHOWN: AtomicUsize = AtomicUsize::new(0);
const SHOW: usize = 6;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    /// This thread is one of the pool's workers (set by the pool's start hook).
    static WORKER: Cell<bool> = const { Cell::new(false) };
}

/// The workers are being watched: from before a block starts until after it ends.
static WORKERS_COUNTED: AtomicBool = AtomicBool::new(false);
static WORKERS_SEEN: AtomicUsize = AtomicUsize::new(0);
/// Workers that came back out of the plug-in they were left in, and stopped being counted then:
/// such a thread ends, and a thread's end frees what std set it up with.
static WORKERS_LET_GO: AtomicUsize = AtomicUsize::new(0);

fn counted(what: &str) {
    let worker = WORKERS_COUNTED.load(Ordering::Relaxed) && WORKER.try_with(|c| c.get()).unwrap_or(false);
    let on = worker || COUNTING.try_with(|c| c.get()).unwrap_or(false);
    if !on {
        return;
    }
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    if SHOWN.fetch_add(1, Ordering::Relaxed) < SHOW {
        // The backtrace allocates too: not counted.
        let was = WORKER.with(|c| c.replace(false));
        let counting = COUNTING.with(|c| c.replace(false));
        eprintln!("{what} inside Engine::process:\n{}", std::backtrace::Backtrace::force_capture());
        COUNTING.with(|c| c.set(counting));
        WORKER.with(|c| c.set(was));
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        counted("allocation");
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        counted("free");
        System.dealloc(p, l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        counted("allocation");
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        counted("reallocation");
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

const BLOCK: usize = 256;

fn lsp_compressor() -> Option<vst3_host::PluginDescriptor> {
    let bundles = vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths());
    let lsp = bundles.iter().filter(|b| b.to_string_lossy().contains("lsp-plugins")).flat_map(|b| vst3_host::scan::scan_bundle(b).unwrap_or_default());
    lsp.into_iter().find(|d| d.name == "Sidechain Compressor Stereo")
}

/// A VST3's key and its gate around a change of its buses, on the processor directly: the
/// `Cmd::VstGate`s arrive on the audio thread, the `setActive` in between runs off it.
fn vst_gate(d: &vst3_host::PluginDescriptor) {
    let (mut p, mut c) = vst3_host::instantiate_keyed(d, 48000.0, BLOCK, false).unwrap();
    let t = vst3_host::TransportInfo { sample_rate: 48000.0, tempo: 120.0, sig_num: 4, sig_den: 4, ..Default::default() };
    let (mut l, mut r) = (vec![0.1f32; BLOCK], vec![0.1f32; BLOCK]);
    let key = vec![0.5f32; BLOCK];
    let mut run = |p: &mut vst3_host::PluginProcessor, keyed: bool, blocks: usize| {
        for _ in 0..blocks {
            COUNTING.with(|c| c.set(true));
            p.process_keyed(&mut l, &mut r, keyed.then_some((&key[..], &key[..])), &[], &t);
            COUNTING.with(|c| c.set(false));
        }
    };
    run(&mut p, false, 100);
    let mark = ALLOCS.load(Ordering::Relaxed);
    for on in [true, false, true] {
        COUNTING.with(|c| c.set(true));
        p.close_gate();
        COUNTING.with(|c| c.set(false));
        run(&mut p, !on, 4);
        COUNTING.with(|c| c.set(true));
        p.shut_gate();
        COUNTING.with(|c| c.set(false));
        run(&mut p, !on, 1);
        c.set_key_active(on).unwrap();
        COUNTING.with(|c| c.set(true));
        p.open_gate(0);
        COUNTING.with(|c| c.set(false));
        run(&mut p, on, 50);
    }
    let n = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("a VST3's key and its gate allocate nothing ({n})"), n == 0);

    // Edits from its editor while it's bypassed, in a block of its own.
    let id = c.parameters()[0].id;
    let mark = ALLOCS.load(Ordering::Relaxed);
    for i in 0..50 {
        c.edit_param(id, i as f64 / 50.0);
        COUNTING.with(|c| c.set(true));
        p.pass_edits(&t);
        COUNTING.with(|c| c.set(false));
    }
    let n = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("a bypassed VST3 takes its editor's edits without allocating ({n})"), n == 0 && !p.has_edits());
}

/// A plug-in on `track` that stops responding on a worker (`tar_for_checks`), with blocks given up on
/// when they run that late, as they are live: the worker is left in it, the track and the stack it
/// plays through sit out, and once the plug-in returns its strip goes back as garbage and the
/// session brings the track back. None of it may allocate, in the callback or on the workers. The
/// transport plays on, the session says what happened, and renders are refused meanwhile.
fn stuck_plugin(
    s: &mut Session,
    guard: &mut parking_lot::MutexGuard<daw_engine::dsp::Engine>,
    run: &mut impl FnMut(&mut parking_lot::MutexGuard<daw_engine::dsp::Engine>, usize, bool),
    track: Id,
) {
    // Whatever the session had to say so far.
    s.tick();
    let hold = Arc::new(AtomicBool::new(true));
    s.set_threads_for_checks(4, daw_engine::dsp::ParallelTest { always: true, renders: false, patient: false });
    s.insert_for_checks(track, 7, daw_engine::dsp::parallel::tar_for_checks(999_999, s.sample_rate() as f32, hold.clone()));
    let mark = ALLOCS.load(Ordering::Relaxed);
    let started = std::time::Instant::now();
    // The callback plays a block alone now and then, and Tar only holds up a worker.
    while guard.status.left_out.load(Ordering::Relaxed) == 0 && started.elapsed().as_secs() < 30 {
        run(guard, 1, true);
    }
    let left = guard.status.left_out.load(Ordering::Relaxed);
    // Past the overload's silence.
    run(guard, 200, true);
    let n = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("a plug-in that stops responding on a worker and its track left out allocate nothing ({n}; {left} tracks left out)"), n == 0 && left >= 2);
    let warning = s.tick().warning.unwrap_or_default();
    check(&format!("the transport plays on, and the session says why ({warning:?})"), s.is_playing() && warning.contains("stopped responding"));
    s.idle();
    let refused = parking_lot::MutexGuard::unlocked(guard, || s.render(0, 4800, 0, None, &mut |_| {}));
    let why = refused.as_ref().err().map(|e| e.to_string()).unwrap_or_default();
    check(&format!("a render is refused meanwhile ({why:?})"), why.contains("sitting out"));

    // Tar returns: the strip goes back, and the track comes back with a new one. The other workers
    // are counted all along; the one left in Tar only until it's out.
    let mark = ALLOCS.load(Ordering::Relaxed);
    let let_go = WORKERS_LET_GO.load(Ordering::Relaxed);
    hold.store(false, Ordering::SeqCst);
    let started = std::time::Instant::now();
    while !guard.status.held().is_empty() && started.elapsed().as_secs() < 10 {
        run(guard, 1, true);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    s.idle();
    run(guard, 20, true);
    let n = ALLOCS.load(Ordering::Relaxed) - mark;
    let out = guard.status.left_out.load(Ordering::Relaxed);
    let let_go = WORKERS_LET_GO.load(Ordering::Relaxed) - let_go;
    check(
        &format!("the strip taken back and the track brought back allocate nothing in the callback or on the workers ({n}; {out} tracks left out; {let_go} worker let go)"),
        n == 0 && out == 0 && let_go == 1,
    );
    let warning = s.tick().warning.unwrap_or_default();
    check(&format!("and the session says so ({warning:?})"), warning.contains("responds again"));
}

fn s_state(s: &Session, t: Id) -> Option<std::sync::Arc<str>> {
    s.project.track(t).and_then(|t| t.instrument.as_ref()).and_then(|i| i.state.clone())
}

/// A track with Marbble Sampler, its zones in the way a real instrument would use them, and enough
/// notes at low polyphony to keep it stealing.
fn add_sampler(s: &mut Session, scratch: &std::path::Path) -> Id {
    use daw_engine::instrument::sampler::{params::param, state::SamplerState};
    let dir = scratch.join("sampler-files");
    std::fs::create_dir_all(&dir).unwrap();
    let wave = |hz: f32, rate: u32| -> Vec<f32> { (0..rate * 2).map(|i| (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * 0.3).collect() };
    let a = dir.join("A.wav");
    let b = dir.join("B.wav");
    daw_engine::audio_file::decode::write_wav(&a, 44100, &[wave(220.0, 44100), wave(221.0, 44100)]).unwrap();
    daw_engine::audio_file::decode::write_wav(&b, 96000, &[wave(330.0, 96000)]).unwrap();
    let t = s.add_track(TrackKind::Instrument, daw_engine::model::InputSel::None, Some(daw_engine::instrument::sampler::ID.into()), None).unwrap();
    s.sampler_add_files(t, &[a, b.clone(), b], Some(40)).unwrap();
    let mut zones = SamplerState::parse(s_state(s, t).as_deref()).zones;
    for z in &mut zones {
        z.key_lo = 0;
        z.key_hi = 127;
    }
    zones[0].loop_on = true;
    zones[0].loop_start = 10000;
    zones[0].loop_end = 50000;
    zones[0].xfade = 4000;
    zones[1].rr_group = 1;
    zones[2].rr_group = 1;
    zones[2].reverse = true;
    s.sampler_update_zones(t, zones).unwrap();
    s.set_builtin_param(t, usize::MAX, param::POLYPHONY, 2.0).unwrap();
    s.set_builtin_param(t, usize::MAX, param::LFO_PITCH, 30.0).unwrap();
    s.set_builtin_param(t, usize::MAX, param::GLIDE, 20.0).unwrap();
    let r = s.create_midi_region(t, 0, 48000 * 8).unwrap();
    let notes: Vec<daw_engine::model::Note> =
        (0..64).map(|k| daw_engine::model::Note { start: k * 6000, length: 18000, pitch: 40 + (k % 37) as u8, velocity: 100, muted: false }).collect();
    s.set_midi_content(vec![daw_engine::session::MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
    t
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    daw_engine::dsp::pool::set_worker_hook(|| {
        WORKER.with(|c| c.set(true));
        WORKERS_SEEN.fetch_add(1, Ordering::Relaxed);
    });
    daw_engine::dsp::pool::set_left_hook(|| {
        WORKER.with(|c| c.set(false));
        WORKERS_LET_GO.fetch_add(1, Ordering::Relaxed);
    });
    println!("-- the callback alone");
    scenario(&scratch.join("serial"), 1);
    println!("-- four threads");
    scenario(&scratch.join("pool"), 4);
    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}

fn scenario(scratch: &Path, threads: u32) {
    std::fs::create_dir_all(scratch).unwrap();
    // From the start: where Automatic means several threads (macOS, a Linux machine with enough
    // cores), the project's edits start the workers before the setting below asks for as many.
    let workers_before = WORKERS_SEEN.load(Ordering::Relaxed);
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("RtAlloc.marbble");
    let mut w = base(&mut s, scratch);
    for (name, step) in steps() {
        s.checkpoint(name, None, true);
        step(&mut s, &mut w);
    }
    // A summing stack and an aux with a send, so buses run too.
    let sum = s.create_stack(&[w.a2], true).unwrap();
    let aux = s.create_aux(None).unwrap();
    s.set_send(w.inst, 0, Some(daw_engine::model::TrackSend { bus: aux, level_db: -6.0, pre_fader: false, muted: false })).unwrap();
    s.set_plugin(aux, 0, "builtin:reverb").unwrap();
    s.set_plugin(aux, 1, "builtin:delay").unwrap();
    // A built-in instrument playing notes, with its lane and its costliest path.
    let tre = add_tre(&mut s);
    // A sampler with loops, layers, reversed audio and round robin, stealing its voices.
    let sampler = add_sampler(&mut s, scratch);
    let models: Vec<daw_engine::model::Id> = MODELS.iter().map(|(plugin, preset)| add_model(&mut s, plugin, preset)).collect();
    s.idle();
    println!("audio: {:?}", s.audio_status.running);
    let test = daw_engine::dsp::ParallelTest { always: threads > 1, renders: false, patient: true };
    s.set_threads_for_checks(threads, test);
    // The counter itself works (quietly: no backtrace for this one).
    SHOWN.store(SHOW, Ordering::Relaxed);
    COUNTING.with(|c| c.set(true));
    let v: Vec<u8> = Vec::with_capacity(SHOW);
    COUNTING.with(|c| c.set(false));
    check("the counter sees an allocation", ALLOCS.swap(0, Ordering::Relaxed) == 1 && v.capacity() == SHOW);
    drop(v);
    SHOWN.store(0, Ordering::Relaxed);
    // A real VST3 keyed by another track, where one is installed.
    let lsp = lsp_compressor();
    let vst_key = lsp.as_ref().map(|d| {
        s.set_plugins(vec![d.clone()]);
        let u = s.set_plugin(w.a2, 6, &d.id).unwrap();
        s.set_sidechain(w.a2, 6, Some(w.a1)).unwrap();
        check("the VST3 hears its key", s.vst_key_active(u) == Some(true));
        u
    });
    let latency = s.pdc_total();

    let engine = s.engine_for_checks();
    let mut buf = vec![0.0f32; BLOCK * 2];
    let mut peak = 0.0f32;
    let mut guard = engine.lock();
    // Runs `blocks` blocks, counting what they allocate when `count` is set.
    let mut run = |guard: &mut parking_lot::MutexGuard<daw_engine::dsp::Engine>, blocks: usize, count: bool| {
        for _ in 0..blocks {
            WORKERS_COUNTED.store(count, Ordering::Relaxed);
            COUNTING.with(|c| c.set(count));
            guard.process(&[], 0, &mut buf, 2);
            COUNTING.with(|c| c.set(false));
            WORKERS_COUNTED.store(false, Ordering::Relaxed);
            peak = buf.iter().fold(peak, |m, x| m.max(x.abs()));
        }
    };
    let pooled_before = guard.status.parallel_blocks.load(Ordering::Relaxed);
    // Between blocks the session makes edits; their commands wait for the next block.
    s.locate(0);
    s.play();
    run(&mut guard, 400, false);
    let before = ALLOCS.load(Ordering::Relaxed);
    let bars = |b: f64| (b * BAR as f64 / BLOCK as f64) as usize;

    run(&mut guard, bars(1.0), true);
    let playing = ALLOCS.load(Ordering::Relaxed) - before;
    check(&format!("playing allocates nothing ({playing} allocations or frees)"), playing == 0);

    // A device that delivers a far larger buffer than usual, through the output callback as a
    // device runs it: input from its ring, the output in 16-bit samples.
    const HUGE: usize = 3 * 8192 + 100;
    let (mut in_tx, in_rx) = rtrb::RingBuffer::new(HUGE * 2);
    let mut callback = daw_engine::audio_io::callback::Callback::new(Some(in_rx), 2, guard.status.clone());
    let mut huge = vec![0i16; HUGE * 2];
    let mark = ALLOCS.load(Ordering::Relaxed);
    for _ in 0..3 {
        while in_tx.push(0.1).is_ok() {}
        WORKERS_COUNTED.store(true, Ordering::Relaxed);
        COUNTING.with(|c| c.set(true));
        callback.play(Some(&mut guard), &mut huge, 2);
        COUNTING.with(|c| c.set(false));
        WORKERS_COUNTED.store(false, Ordering::Relaxed);
    }
    let n = ALLOCS.load(Ordering::Relaxed) - mark;
    let heard = huge.iter().any(|&s| s != 0);
    check(&format!("a device buffer of {HUGE} frames allocates nothing in the callback ({n})"), n == 0 && heard);
    drop((callback, in_tx, huge));

    let mark = ALLOCS.load(Ordering::Relaxed);
    // A latency change: the Linear Phase EQ's quality, then the glide as compensation follows.
    s.set_builtin_param(w.drums, 0, daw_engine::fx::linear_phase_eq::QUALITY, 0.0).unwrap();
    for _ in 0..60 {
        run(&mut guard, 4, true);
        s.idle();
    }
    let changed = s.pdc_total() != latency;
    let glide = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("a latency change and its glide allocate nothing ({glide}; compensation {latency} -> {} frames)", s.pdc_total()), glide == 0 && changed);

    let mark = ALLOCS.load(Ordering::Relaxed);
    // Edits arriving while playing: new automation, VCA level and mute, region parameters,
    // a pattern change, a folder's fader, comps.
    s.set_automation_lane(w.folder, AutoParam::Volume, vec![pt(0, -3.0, 0.2), pt(8 * BAR, -20.0, 0.0)]);
    run(&mut guard, 50, true);
    s.update_vca(w.vca, VcaPatch { level_db: Some(-12.0), ..Default::default() });
    run(&mut guard, 50, true);
    s.update_vca(w.vca, VcaPatch { mute: Some(true), ..Default::default() });
    run(&mut guard, 50, true);
    s.update_vca(w.vca, VcaPatch { mute: Some(false), ..Default::default() });
    s.set_region_params(&[w.midi], &RegionParamsPatch { transpose: Some(-5), ..Default::default() });
    run(&mut guard, 50, true);
    s.update_track(w.folder, TrackPatch { volume_db: Some(-4.0), ..Default::default() });
    s.update_track(sum, TrackPatch { volume_db: Some(-2.0), ..Default::default() });
    run(&mut guard, 50, true);
    // Tracks and a whole stack going off, sleeping and waking while playing, an aux with effects
    // and an instrument among them.
    for id in [w.folder, aux, w.inst] {
        s.update_track(id, TrackPatch { off: Some(true), ..Default::default() });
        run(&mut guard, 50, true);
        s.update_track(id, TrackPatch { off: Some(false), ..Default::default() });
        run(&mut guard, 50, true);
    }
    if let Some(p) = s.project.track(w.drums).unwrap().regions.iter().find(|r| r.id == w.pattern).and_then(|r| r.pattern.clone()) {
        let mut p = p;
        p.swing = 0;
        s.set_pattern(w.pattern, p).unwrap();
    }
    s.set_comp_range(w.take_folder, None, 0, 20_000);
    s.set_builtin_param(w.a1, 0, 0, 0.7).unwrap();
    run(&mut guard, 50, true);
    // Plug-ins and tracks coming and going, and undo and redo, while playing.
    s.set_plugin(aux, 2, "builtin:chorus").unwrap();
    run(&mut guard, 50, true);
    // A built-in instrument's parameter moving, its preset changing and it going off and on, mid-note.
    s.set_builtin_param(tre, usize::MAX, daw_engine::instrument::tre::params::param::EMPHASIS, 9.0).unwrap();
    run(&mut guard, 50, true);
    s.load_preset(tre, usize::MAX, "builtin:tre", "Wall of Saws (unison)", true).unwrap();
    run(&mut guard, 50, true);
    s.update_track(tre, TrackPatch { off: Some(true), ..Default::default() });
    run(&mut guard, 50, true);
    s.update_track(tre, TrackPatch { off: Some(false), ..Default::default() });
    run(&mut guard, 50, true);
    // A sampler's zones and samples replaced under its held notes, with the old ones going back.
    use daw_engine::instrument::sampler::state::SamplerState;
    let zones = |s: &Session| SamplerState::parse(s_state(s, sampler).as_deref()).zones;
    let mut z = zones(&s);
    z[0].gain_db = -3.0;
    s.sampler_update_zones(sampler, z).unwrap();
    run(&mut guard, 50, true);
    let mut z = zones(&s);
    z[0].file = z[1].file.clone();
    z[0].loop_end = 30000;
    s.sampler_update_zones(sampler, z).unwrap();
    run(&mut guard, 50, true);
    s.sampler_delete_zones(sampler, &[zones(&s)[1].id]).unwrap();
    run(&mut guard, 50, true);
    s.idle();
    s.set_builtin_param(sampler, usize::MAX, daw_engine::instrument::sampler::params::param::F_TYPE, 4.0).unwrap();
    run(&mut guard, 50, true);
    s.update_track(sampler, TrackPatch { off: Some(true), ..Default::default() });
    run(&mut guard, 20, true);
    s.update_track(sampler, TrackPatch { off: Some(false), ..Default::default() });
    run(&mut guard, 20, true);
    // The same for each of the other models: a parameter from their spec, a preset, off and on.
    for (&(plugin, _), &t) in MODELS.iter().zip(&models) {
        let info = daw_engine::instrument::find(plugin).unwrap();
        for p in info.params.iter().step_by(3) {
            s.set_builtin_param(t, usize::MAX, p.id, 0.5 * (p.min + p.max)).unwrap();
        }
        run(&mut guard, 30, true);
        s.load_preset(t, usize::MAX, plugin, info.presets[info.presets.len() / 2].name, true).unwrap();
        run(&mut guard, 30, true);
        s.update_track(t, TrackPatch { off: Some(true), ..Default::default() });
        run(&mut guard, 30, true);
        s.update_track(t, TrackPatch { off: Some(false), ..Default::default() });
        run(&mut guard, 30, true);
    }
    let extra = s.add_track(TrackKind::Instrument, daw_engine::model::InputSel::None, Some(daw_engine::session::BUILTIN_SYNTH.into()), None).unwrap();
    run(&mut guard, 50, true);
    s.delete_track(extra);
    run(&mut guard, 50, true);
    for _ in 0..6 {
        s.undo();
        run(&mut guard, 30, true);
    }
    for _ in 0..6 {
        s.redo();
        run(&mut guard, 30, true);
    }
    for _ in 0..20 {
        run(&mut guard, 4, true);
        s.idle();
    }
    if vst_key.is_some() {
        // Between sources and from before to after the fader the bus stays as it is.
        s.set_sidechain_pre(w.a2, 6, true).unwrap();
        run(&mut guard, 50, true);
        s.set_sidechain(w.a2, 6, Some(w.inst)).unwrap();
        run(&mut guard, 50, true);
        s.set_sidechain(w.a2, 6, Some(w.a1)).unwrap();
        run(&mut guard, 50, true);
    }
    let edits = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("edits while playing allocate nothing inside process ({edits})"), edits == 0);

    let mark = ALLOCS.load(Ordering::Relaxed);
    // Locating into the middle of notes (chase) and cycling over the take folders and patterns.
    s.locate(BAR + BAR / 3);
    run(&mut guard, 100, true);
    s.set_cycle(true, 3 * BAR, 5 * BAR);
    s.locate(4 * BAR + 1000);
    run(&mut guard, bars(3.5), true);
    s.set_cycle(false, 3 * BAR, 5 * BAR);
    s.stop();
    run(&mut guard, 100, true);
    s.play();
    run(&mut guard, bars(4.0), true);
    let moves = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("locating, cycling and stopping allocate nothing ({moves})"), moves == 0);
    if threads > 1 {
        stuck_plugin(&mut s, &mut guard, &mut run, w.a2);
    }
    check(&format!("it played something (peak {peak:.3})"), peak > 0.01);
    let pooled = guard.status.parallel_blocks.load(Ordering::Relaxed) - pooled_before;
    if threads > 1 {
        // The route changes above play serially until the idle after them sends a new graph.
        check(&format!("the pool played it ({pooled} blocks)"), pooled > 1000);
        let seen = WORKERS_SEEN.load(Ordering::Relaxed) - workers_before;
        check(&format!("and its workers were counted ({seen})"), seen >= threads as usize - 1);
    } else {
        check("the callback played it alone", pooled == 0);
    }
    drop(guard);
    s.stop();
    let dropped = s.status.cmds_dropped.load(Ordering::Relaxed);
    check(&format!("no command for the engine was dropped ({dropped})"), dropped == 0);
    match &lsp {
        Some(d) => vst_gate(d),
        None => println!("SKIP the VST3 side-chain: LSP's Sidechain Compressor isn't installed"),
    }
}
