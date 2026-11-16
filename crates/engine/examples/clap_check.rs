//! CLAP hosting (#558) with real plug-ins, against the same plug-ins' VST3s in the same session:
//! LSP's Compressor Stereo and Multi-Sampler x12 DirectOut. Checks the scan (out of process, as
//! the app does it, with this binary as the scanner), that the CLAP compressor renders what its
//! VST3 does, that edits and automation reach it, that its state survives saving and reopening,
//! that a project whose CLAP plug-in isn't installed keeps the slot and its state as one with a
//! missing VST3 does, that the CLAP sampler plays notes as its VST3 does, and that no block with
//! CLAP plug-ins playing allocates. Then a restart (#560): LSP's Limiter asks for one when its
//! lookahead changes, played live beside another track, and right before a render. Side-chains,
//! extra outputs and tails of CLAP plug-ins are in `vst_sidechain_check`, `multiout_check`,
//! `multiout_freeze_check` and `vst_tails_check`, which run in both formats. Prints SKIP without
//! LSP's CLAP plug-ins.
//! usage: clap_check <audio file> <scratch dir>
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::library::plugin_db;
use daw_engine::model::{AutoParam, AutoPoint, Id, InputSel, Note, TrackKind};
use daw_engine::plugin_api::{PluginDescriptor, PluginFormat};
use daw_engine::session::{MidiContent, TrackPatch};
use daw_engine::Session;

#[path = "common/multi_sampler.rs"]
mod multi_sampler;
use multi_sampler::{load_samples, peak, sine, wait_for_samples, NOTE_ONE};

const COMPRESSOR: &str = "Compressor Stereo";
const LIMITER: &str = "Limiter Stereo";
const SAMPLER: &str = multi_sampler::PLUGIN;
const BLOCK: usize = 256;

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Counts allocations and frees on this thread while `COUNTING` is set.
struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

fn counted() {
    if COUNTING.try_with(|c| c.get()).unwrap_or(false) {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        counted();
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        counted();
        System.dealloc(p, l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        counted();
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        counted();
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn session(plugins: &[PluginDescriptor], dir: PathBuf) -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, plugins.to_vec());
    s.dir = dir;
    std::fs::create_dir_all(&s.dir).unwrap();
    s
}

/// Runs the idle loop for a while, as the app does every 30 ms.
fn pump(s: &mut Session, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        s.idle();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// One track rendered alone through its strip.
fn render_track(s: &mut Session, track: Id, len: i64) -> Vec<Vec<f32>> {
    let iso = s.isolate_track(track, true, true);
    s.render(0, len, 0, Some(iso), &mut |_| {}).unwrap()
}

/// The largest difference between two renders, and whether they're the same length.
fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    if a.len() != b.len() || a.iter().zip(b).any(|(x, y)| x.len() != y.len()) {
        return f32::INFINITY;
    }
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y)).fold(0.0, |m, (p, q)| m.max((p - q).abs()))
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn param(s: &Session, uid: Id, name: &str) -> u32 {
    s.plugin_params(uid).iter().find(|p| p.name == name).map(|p| p.id).unwrap_or_else(|| panic!("no parameter {name}"))
}

fn near(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-3)
}

/// Scans every plug-in folder out of process, as the app does, with this binary as the scanner.
fn scan() -> Vec<PluginDescriptor> {
    let exe = std::env::current_exe().unwrap();
    let t = Instant::now();
    let list = plugin_db::scan_with(&exe, &[]);
    let report = plugin_db::load_report().unwrap_or_default();
    let clap = list.iter().filter(|d| d.format == PluginFormat::Clap).count();
    println!(
        "     scanned {} files in {:.1?}: {} plug-ins, {clap} of them CLAP, {} files failed",
        report.bundles,
        t.elapsed(),
        list.len(),
        report.failed.len()
    );
    for f in &report.failed {
        println!("       {}: {}", f.path, f.reason);
    }
    let cached = plugin_db::load_cache().unwrap_or_default();
    check("the cache keeps each plug-in's format", cached.iter().filter(|d| d.format == PluginFormat::Clap).count() == clap);
    list
}

fn find(list: &[PluginDescriptor], name: &str, format: PluginFormat) -> Option<PluginDescriptor> {
    list.iter().find(|d| d.name == name && d.format == format && d.path.to_string_lossy().contains("lsp-plugins")).cloned()
}

/// The compressor in both formats on two tracks playing the same audio.
fn compressor(clap: &PluginDescriptor, vst: Option<&PluginDescriptor>, tone: &Path, scratch: &Path) {
    let list: Vec<PluginDescriptor> = std::iter::once(clap.clone()).chain(vst.cloned()).collect();
    let mut s = session(&list, scratch.join("Compressor.marbble"));
    let len = daw_engine::audio_file::decode::decode(tone).unwrap().channels[0].len() as i64;
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(tone, a, 0).unwrap();
    let uid = s.set_plugin(a, 0, &clap.id).unwrap();
    check("the CLAP compressor loads", s.missing_plugins().is_empty() && !s.plugin_params(uid).is_empty());
    let dry = {
        s.set_bypass(a, 0, true);
        let r = render_track(&mut s, a, len);
        s.set_bypass(a, 0, false);
        r
    };
    let wet = render_track(&mut s, a, len);
    check(
        &format!("the audio comes through it (peak {:.1} dB, dry {:.1} dB)", db(peak(&wet, (0, len))), db(peak(&dry, (0, len)))),
        peak(&wet, (0, len)) > 0.01,
    );

    if let Some(v) = vst {
        let b = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        s.import_audio(tone, b, 0).unwrap();
        let vuid = s.set_plugin(b, 0, &v.id).unwrap();
        let theirs = render_track(&mut s, b, len);
        let d = diff(&wet, &theirs);
        check(&format!("at its defaults it renders what its VST3 does (largest difference {d:.2e})"), d < 1e-5);
        // The same plain values through both formats' own parameters.
        let names = ["Attack threshold", "Ratio", "Makeup gain"];
        let same: Vec<(String, f64, f64)> = names
            .iter()
            .map(|n| {
                let c = s.vst_param(uid, param(&s, uid, n)).unwrap();
                let v = s.vst_param(vuid, param(&s, vuid, n)).unwrap();
                (n.to_string(), c, v)
            })
            .collect();
        println!("     defaults, normalized, CLAP vs VST3: {same:?}");
        let (t, vt) = (param(&s, uid, "Attack threshold"), param(&s, vuid, "Attack threshold"));
        s.edit_vst_param(uid, t, 0.2);
        s.edit_vst_param(vuid, vt, 0.2);
        pump(&mut s, 100);
        let ours = render_track(&mut s, a, len);
        let theirs = render_track(&mut s, b, len);
        let d = diff(&ours, &theirs);
        let moved = diff(&ours, &wet);
        check(&format!("an edit reaches it, and renders as the same edit on the VST3 does (moved {moved:.2e}, apart {d:.2e})"), moved > 1e-3 && d < 1e-5);
        s.remove_plugin(b, 0);
    }

    // Automation, normalized, through the parameter's range.
    let gain = param(&s, uid, "Makeup gain");
    let g0 = s.vst_param(uid, gain).unwrap();
    s.edit_vst_param(uid, param(&s, uid, "Attack threshold"), s.plugin_params(uid).iter().find(|p| p.name == "Attack threshold").unwrap().default);
    pump(&mut s, 100);
    let before = render_track(&mut s, a, len);
    let up = (g0 + 0.25).min(1.0);
    let pts = vec![
        AutoPoint { pos: 0, value: g0 as f32, curve: 0.0 },
        AutoPoint { pos: len / 2, value: g0 as f32, curve: 0.0 },
        AutoPoint { pos: len / 2 + 4800, value: up as f32, curve: 0.0 },
    ];
    s.set_automation_lane(a, AutoParam::Plugin { uid, param: gain, name: String::new() }, pts);
    let auto = render_track(&mut s, a, len);
    let first = diff(&[before[0][..(len / 2) as usize].to_vec()], &[auto[0][..(len / 2) as usize].to_vec()]);
    let (pb, pa) = (peak(&before, (len * 3 / 4, len)), peak(&auto, (len * 3 / 4, len)));
    check(
        &format!("automation plays: untouched before it moves ({first:.2e}), louder after ({:.1} -> {:.1} dB)", db(pb), db(pa)),
        first < 1e-6 && db(pa) - db(pb) > 1.0,
    );
    check(&format!("and the plug-in ends at the lane's value ({:?} for {up:.3})", s.vst_param(uid, gain)), near(s.vst_param(uid, gain), up));
    s.set_automation_lane(a, AutoParam::Plugin { uid, param: gain, name: String::new() }, Vec::new());

    // Saved and reopened.
    s.edit_vst_param(uid, gain, up);
    pump(&mut s, 200);
    let edited = render_track(&mut s, a, len);
    let path = s.save(None).unwrap();
    let state = s.copy_plugin(a, 0).unwrap().state;
    let mut back = session(&list, scratch.join("Reopened.marbble"));
    back.open(&path).unwrap();
    let again = render_track(&mut back, a, len);
    let d = diff(&edited, &again);
    check(
        &format!("saved and reopened, it has its state ({:?}) and renders the same ({d:.2e})", back.vst_param(uid, gain)),
        near(back.vst_param(uid, gain), up) && d < 1e-6 && back.missing_plugins().is_empty(),
    );
    drop(back);

    // Opened where the plug-in isn't installed.
    let json = std::fs::read_to_string(path.join("project.json")).unwrap();
    let gone = json.replace(&clap.id, "/nowhere/Gone.clap|com.example.gone");
    let elsewhere = scratch.join("Elsewhere.marbble");
    std::fs::create_dir_all(&elsewhere).unwrap();
    copy_dir(&path, &elsewhere);
    std::fs::write(elsewhere.join("project.json"), &gone).unwrap();
    let mut missing = session(&list, scratch.join("Missing.marbble"));
    missing.open(&elsewhere).unwrap();
    let names: Vec<String> = missing.missing_plugins().iter().map(|p| p.name.clone()).collect();
    let r = render_track(&mut missing, a, len);
    check(
        &format!("a CLAP plug-in that isn't installed is missing ({names:?}), and the track plays dry ({:.2e})", diff(&r, &dry)),
        names.len() == 1 && diff(&r, &dry) < 1e-6,
    );
    let saved = missing.save(None).unwrap();
    let slot = missing.project.track(a).and_then(|t| t.inserts[0].clone());
    check(
        "saved there, the slot keeps its plug-in and its state",
        slot.is_some_and(|p| p.plugin_id == "/nowhere/Gone.clap|com.example.gone" && p.state == state) && saved.join("project.json").exists(),
    );
}

fn copy_dir(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        let dest = to.join(e.file_name());
        if p.is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
            copy_dir(&p, &dest);
        } else {
            std::fs::copy(&p, &dest).unwrap();
        }
    }
}

/// The sampler in both formats, given the same sample and notes.
/// Gives back the session, with the CLAP sampler still playing its note.
fn sampler(clap: &PluginDescriptor, vst: Option<&PluginDescriptor>, scratch: &Path) -> Option<Session> {
    let list: Vec<PluginDescriptor> = std::iter::once(clap.clone()).chain(vst.cloned()).collect();
    let mut s = session(&list, scratch.join("Sampler.marbble"));
    let rate = s.project.sample_rate;
    let (one, two) = (scratch.join("one.wav"), scratch.join("two.wav"));
    sine(&one, 1000.0, rate);
    sine(&two, 2000.0, rate);
    let note = rate as i64 / 2;
    let len = rate as i64 * 2;
    let add = |s: &mut Session, d: &PluginDescriptor| -> Option<Id> {
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let r = s.create_midi_region(t, 0, len).unwrap();
        let notes = vec![Note { start: note, length: note, pitch: NOTE_ONE, velocity: 100, muted: false }];
        s.set_midi_content(vec![MidiContent { region: r, notes, cc: vec![], start: None, length: None }]);
        // It loads the sample on a thread of its own, and now and then loses it (issue #37).
        for _ in 0..5 {
            s.set_plugin(t, usize::MAX, &d.id).unwrap();
            load_samples(s, t, &one, &two);
            if wait_for_samples(s, t, (note, len)) {
                return Some(t);
            }
            println!("     the plug-in lost a sample while loading; loading it again");
        }
        None
    };
    let Some(t) = add(&mut s, clap) else {
        check("the CLAP sampler loads its sample", false);
        return None;
    };
    let out = render_track(&mut s, t, len);
    check(
        &format!("the CLAP sampler plays the note ({:.1} dB) and nothing before it ({:.1} dB)", db(peak(&out, (note, len))), db(peak(&out, (0, note)))),
        peak(&out, (note, len)) > 0.1 && peak(&out, (0, note - 1)) < 1e-6,
    );
    if let Some(v) = vst {
        if let Some(vt) = add(&mut s, v) {
            let theirs = render_track(&mut s, vt, len);
            let d = diff(&out, &theirs);
            check(&format!("and plays what its VST3 does (largest difference {d:.2e})"), d < 1e-4);
            s.delete_tracks(&[vt]);
        }
    }
    Some(s)
}

/// Blocks through `Engine::process` on this thread, as the device's callback runs them, with the
/// CLAP compressor automated and the CLAP sampler playing.
fn no_allocations(mut s: Session, comp: &PluginDescriptor, tone: &Path) {
    s.set_plugins(vec![comp.clone()]);
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(tone, a, 0).unwrap();
    let uid = s.set_plugin(a, 0, &comp.id).unwrap();
    let gain = param(&s, uid, "Makeup gain");
    let len = s.project.sample_rate as i64 * 2;
    let pts = (0..20).map(|i| AutoPoint { pos: i * len / 20, value: if i % 2 == 0 { 0.2 } else { 0.6 }, curve: 0.0 }).collect();
    s.set_automation_lane(a, AutoParam::Plugin { uid, param: gain, name: String::new() }, pts);
    s.set_cycle(true, 0, len);
    pump(&mut s, 200);
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    let mut out = vec![0.0f32; BLOCK * 2];
    s.locate(0);
    s.play();
    for _ in 0..200 {
        e.process(&[], 0, &mut out, 2);
    }
    let mut loudest = 0.0f32;
    ALLOCS.store(0, Ordering::Relaxed);
    for _ in 0..2000 {
        COUNTING.with(|c| c.set(true));
        e.process(&[], 0, &mut out, 2);
        COUNTING.with(|c| c.set(false));
        loudest = out.iter().fold(loudest, |m, x| m.max(x.abs()));
    }
    let n = ALLOCS.load(Ordering::Relaxed);
    drop(e);
    s.stop();
    check(
        &format!("2000 blocks with both CLAP plug-ins playing, automated, in a cycle: {n} allocations (peak {:.1} dB)", db(loudest)),
        n == 0 && loudest > 0.01,
    );
}

/// The largest change from one sample to the next.
fn largest_step(a: &[f32]) -> f32 {
    a.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// A latency change, which CLAP allows only while a plug-in is deactivated: LSP's Limiter asks to
/// be restarted when its lookahead changes. The engine plays on a thread of its own, as the device
/// would, the limiter's track panned left and a steady sine on another track panned right, while
/// the main thread idles as the app does: the restart happens there, delay compensation follows,
/// the other track plays on untouched and the limiter's own fades out and back in. Then the knob
/// dragged, a restart asked for while the last is still fading in, which mustn't click either.
/// Then the same change right before a render, which must already play with it.
fn restart(limiter: &PluginDescriptor, scratch: &Path) {
    let mut s = session(std::slice::from_ref(limiter), scratch.join("Restart.marbble"));
    let rate = s.project.sample_rate;
    // 400 Hz: a whole number of periods in the delay compensation's move, so that its glide
    // doesn't dip the other track.
    let steady_path = scratch.join("steady.wav");
    let steady: Vec<f32> = (0..rate * 10).map(|i| 0.2 * (std::f32::consts::TAU * 400.0 * i as f32 / rate as f32).sin()).collect();
    daw_engine::audio_file::decode::write_wav(&steady_path, rate, &[steady.clone(), steady]).unwrap();
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    for (t, pan) in [(a, -1.0), (b, 1.0)] {
        s.import_audio(&steady_path, t, 0).unwrap();
        s.update_track(t, TrackPatch { pan: Some(pan), ..Default::default() });
    }
    let uid = s.set_plugin(a, 0, &limiter.id).unwrap();
    let params = s.plugin_params(uid);
    let look = params.iter().find(|p| p.name.to_lowercase().contains("lookahead")).map(|p| p.id);
    let Some(look) = look else {
        check(&format!("the limiter has a lookahead ({:?})", params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()), false);
        return;
    };
    pump(&mut s, 100);
    let before = s.pdc_total();

    s.locate(0);
    s.play();
    let engine = s.engine_for_checks();
    let stop = Arc::new(AtomicBool::new(false));
    let played = Arc::new(AtomicUsize::new(0));
    let worker = {
        let (engine, stop, played) = (engine.clone(), stop.clone(), played.clone());
        std::thread::spawn(move || {
            let mut guard = engine.lock();
            let mut buf = vec![0.0f32; BLOCK * 2];
            let mut out = Vec::with_capacity(48000 * 20);
            let (mut longest, started) = (Duration::ZERO, Instant::now());
            while !stop.load(Ordering::Relaxed) && out.len() + BLOCK < out.capacity() {
                let warm = started.elapsed() > Duration::from_millis(50);
                let t = Instant::now();
                COUNTING.with(|c| c.set(warm));
                guard.process(&[], 0, &mut buf, 2);
                COUNTING.with(|c| c.set(false));
                if warm {
                    longest = longest.max(t.elapsed());
                }
                out.extend_from_slice(&buf);
                played.store(out.len() / 2, Ordering::Relaxed);
                // About real time, so a block is under way when the main thread restarts it.
                std::thread::sleep(Duration::from_micros(4000));
            }
            (out, longest)
        })
    };
    pump(&mut s, 300);
    ALLOCS.store(0, Ordering::Relaxed);
    s.edit_vst_param(uid, look, 1.0);
    let t = Instant::now();
    while s.pdc_total() == before && t.elapsed() < Duration::from_secs(3) {
        s.idle();
        std::thread::sleep(Duration::from_millis(10));
    }
    let took = t.elapsed();
    pump(&mut s, 400);
    // Dragged: a new lookahead every 15 ms, each restart asked for while the last fades in.
    let drag_from = played.load(Ordering::Relaxed);
    let mut seen = std::collections::BTreeSet::new();
    for v in [0.6, 0.9, 0.4, 0.8, 0.5, 0.7, 1.0] {
        s.edit_vst_param(uid, look, v);
        for _ in 0..3 {
            s.idle();
            seen.insert(s.pdc_total());
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pump(&mut s, 400);
    stop.store(true, Ordering::Relaxed);
    let (out, longest) = worker.join().unwrap();
    s.stop();
    let allocs = ALLOCS.load(Ordering::Relaxed);
    let after = s.pdc_total();
    check(
        &format!("a new lookahead restarts the plug-in and delay compensation follows ({before} -> {after} frames, {took:.0?} after the edit)"),
        after > before,
    );
    let (left, right): (Vec<f32>, Vec<f32>) = out.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
    // From when the engine was warm until the drag; the sine's own largest step.
    let (dragged_l, dragged_r) = (&left[drag_from..], &right[drag_from..]);
    let (left, right) = (&left[4800..drag_from], &right[4800..drag_from]);
    let own = 0.2 * std::f32::consts::TAU * 400.0 / rate as f32;
    let quietest = |x: &[f32]| x.chunks(480).filter(|c| c.len() == 480).map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs()))).fold(1.0f32, f32::min);
    println!(
        "     {} frames played, the longest block took {longest:.1?}; the other track's largest step {:.4}, its quietest 10 ms {:.3}; the limiter's track's largest step {:.4} (the sine's own {own:.4})",
        left.len(),
        largest_step(right),
        quietest(right),
        largest_step(left)
    );
    check("the other track plays on through it: no gap, no click", quietest(right) > 0.15 && largest_step(right) < 1.5 * own);
    check("the limiter's track fades out and back in rather than clicking", largest_step(left) < 1.5 * own);
    let end = &left[left.len() - 4800..];
    check(&format!("and plays again (peak {:.3} at the end)", end.iter().fold(0.0f32, |m, v| m.max(v.abs()))), end.iter().any(|v| v.abs() > 0.15));
    let end = &dragged_l[dragged_l.len() - 4800..];
    println!(
        "     dragged: delay compensation went through {seen:?}; the limiter's track's largest step {:.4}, the other's {:.4}",
        largest_step(dragged_l),
        largest_step(dragged_r)
    );
    check(
        "dragging the lookahead, restarts asked for while the last fades in don't click either",
        seen.len() > 2 && largest_step(dragged_l) < 1.5 * own && largest_step(dragged_r) < 1.5 * own && end.iter().any(|v| v.abs() > 0.15),
    );
    check(&format!("the engine's thread allocated nothing meanwhile ({allocs} allocations or frees)"), allocs == 0);

    // Right before a render: it plays with the lookahead it was just given.
    s.edit_vst_param(uid, look, 0.0);
    let r = render_track(&mut s, a, rate as i64);
    let back = s.pdc_total();
    check(
        &format!("changed just before a render, the render has it ({after} -> {back} frames)"),
        back < after && peak(&r, (rate as i64 / 2, rate as i64)) > 0.1,
    );
}

/// The app renders on a thread of its own, where no restart can be done, and the main thread
/// can't answer the plug-in while the render holds the session. A restart asked for just before
/// (the processor parked for it, waiting for the main thread) mustn't leave the plug-in out of
/// the render; and with `begin_renders` on the main thread first, as the app does, the restart
/// is done and its latency is in the render's delay compensation. At its lowest threshold the
/// limiter brings the sine up (0.2 to about 0.4), so whether it's in the render shows in the
/// level.
fn render_elsewhere(limiter: &PluginDescriptor, scratch: &Path) {
    let mut s = session(std::slice::from_ref(limiter), scratch.join("Elsewhere.marbble"));
    let rate = s.project.sample_rate;
    let steady_path = scratch.join("steady.wav");
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(&steady_path, a, 0).unwrap();
    let uid = s.set_plugin(a, 0, &limiter.id).unwrap();
    let id = |s: &Session, name: &str| s.plugin_params(uid).iter().find(|p| p.name.to_lowercase().contains(name)).map(|p| p.id).unwrap();
    let (look, threshold) = (id(&s, "lookahead"), id(&s, "threshold"));
    s.edit_vst_param(uid, threshold, 0.0);
    pump(&mut s, 200);
    let len = rate as i64;
    let level = |r: &[Vec<f32>]| peak(r, (len / 2, len));
    let limited = level(&render_track(&mut s, a, len));
    let elsewhere = |s: &mut Session| std::thread::scope(|sc| sc.spawn(|| render_track(s, a, len)).join().unwrap());

    for (v, begun) in [(0.8, false), (0.3, true)] {
        let before = s.pdc_total();
        s.edit_vst_param(uid, look, v);
        // The device's blocks take the edit; the plug-in asks for a restart from
        // `on_main_thread`, which this idle answers: the restart is asked for, not yet done.
        std::thread::sleep(Duration::from_millis(40));
        s.idle();
        std::thread::sleep(Duration::from_millis(40));
        if begun {
            s.begin_renders();
        }
        let r = elsewhere(&mut s);
        let lagged = s.pdc_total();
        if begun {
            s.end_renders();
        }
        let what = if begun { "after begin_renders on the main thread" } else { "with nothing done on the main thread" };
        check(
            &format!(
                "a render on another thread right after a restart was asked for, {what}, has the plug-in in it (peak {:.3}, through the limiter {limited:.3}, dry 0.200; delay compensation {before} -> {lagged})",
                level(&r)
            ),
            (level(&r) - limited).abs() < 0.02 && (limited - 0.2).abs() > 0.05 && (!begun || lagged != before),
        );
        pump(&mut s, 200);
    }
    check(&format!("and the restart left waiting is done at the next idle ({} frames)", s.pdc_total()), s.pdc_total() > 0 && s.tick().warning.is_none());
}

fn main() {
    // The scanner's child processes run this binary too.
    let args: Vec<String> = std::env::args().collect();
    if let Some(path) = plugin_db::scan_child_path(&args) {
        std::process::exit(path.map(|p| plugin_db::scan_child_main(p.as_ref())).unwrap_or(2));
    }
    let tone = PathBuf::from(args.get(1).expect("audio file"));
    let scratch = PathBuf::from(args.get(2).expect("scratch dir"));
    if !clap_host::scan::find_files(&clap_host::scan::default_search_paths()).iter().any(|f| f.to_string_lossy().contains("lsp-plugins")) {
        println!("SKIP LSP's CLAP plug-ins aren't installed: nothing measured");
        return;
    }
    // The scan's cache and report go here, not into the real settings folder.
    std::env::set_var("XDG_CONFIG_HOME", scratch.join("config"));
    let list = scan();
    let Some(comp) = find(&list, COMPRESSOR, PluginFormat::Clap) else {
        check(&format!("the scan finds LSP's CLAP {COMPRESSOR}"), false);
        std::process::exit(1);
    };
    check(
        &format!("the scan lists it as a CLAP effect: {:?} by {:?} in {:?}", comp.name, comp.vendor, comp.category()),
        comp.id.ends_with("|in.lsp-plug.compressor_stereo") && !comp.is_instrument && comp.category() == "Dynamics",
    );
    let vst_comp = find(&list, COMPRESSOR, PluginFormat::Vst3);
    if vst_comp.is_none() {
        println!("SKIP the comparisons with LSP's VST3s: not installed");
    }
    compressor(&comp, vst_comp.as_ref(), &tone, &scratch);
    match find(&list, SAMPLER, PluginFormat::Clap) {
        Some(d) => {
            check(
                &format!("the scan lists the CLAP sampler as an instrument ({:?})", d.subcategories),
                d.is_instrument && d.subcategories == "Instrument|Sampler",
            );
            if let Some(s) = sampler(&d, find(&list, SAMPLER, PluginFormat::Vst3).as_ref(), &scratch) {
                no_allocations(s, &comp, &tone);
            }
        }
        None => println!("SKIP LSP's CLAP {SAMPLER} isn't installed"),
    }
    match find(&list, LIMITER, PluginFormat::Clap) {
        Some(d) => {
            restart(&d, &scratch);
            render_elsewhere(&d, &scratch);
        }
        None => println!("SKIP LSP's CLAP {LIMITER} isn't installed"),
    }
    std::process::exit(FAILED.load(Ordering::Relaxed) as i32);
}
