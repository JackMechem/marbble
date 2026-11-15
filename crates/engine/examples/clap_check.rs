//! CLAP hosting (#558) with real plug-ins, against the same plug-ins' VST3s in the same session:
//! LSP's Compressor Stereo and Multi-Sampler x12 DirectOut. Checks the scan (out of process, as
//! the app does it, with this binary as the scanner), that the CLAP compressor renders what its
//! VST3 does, that edits and automation reach it, that its state survives saving and reopening,
//! that a project whose CLAP plug-in isn't installed keeps the slot and its state as one with a
//! missing VST3 does, that the CLAP sampler plays notes as its VST3 does, and that no block with
//! CLAP plug-ins playing allocates. Prints SKIP without LSP's CLAP plug-ins.
//! usage: clap_check <audio file> <scratch dir>
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::library::plugin_db;
use daw_engine::model::{AutoParam, AutoPoint, Id, InputSel, Note, TrackKind};
use daw_engine::plugin_api::{PluginDescriptor, PluginFormat};
use daw_engine::session::MidiContent;
use daw_engine::Session;

#[path = "common/multi_sampler.rs"]
mod multi_sampler;
use multi_sampler::{load_samples, peak, sine, wait_for_samples, NOTE_ONE};

const COMPRESSOR: &str = "Compressor Stereo";
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

fn main() {
    // The scanner's child processes run this binary too.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == plugin_db::SCAN_ARG) {
        std::process::exit(args.get(i + 1).map(|p| plugin_db::scan_child_main(p.as_ref())).unwrap_or(2));
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
    std::process::exit(FAILED.load(Ordering::Relaxed) as i32);
}
