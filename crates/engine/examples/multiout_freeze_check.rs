//! Freezing and bouncing in place a multi-output instrument, measured with a real plug-in: LSP's
//! Multi-Sampler x12 DirectOut (`common/multi_sampler.rs`), with an aux on each of its first two
//! outputs. Frozen, the mix must match the live one sample for sample: the instrument plays its
//! freeze file, each aux its output's render, through the aux's own (latent) plug-in and fader.
//! Undo, redo, unfreezing and reopening bring the right state back. Bounce in Place keeps the
//! auxes playing live, or with Instrument Multi-Output takes them into the bounce and mutes them;
//! either way the mix sounds the same. Export Region takes the main output. Prints SKIP when the
//! plug-in isn't installed.
//! usage: multiout_freeze_check <scratch dir>
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, Note, TrackKind};
use daw_engine::session::{BipOptions, BipSource, BitDepth, MidiContent, RenderOptions, TrackPatch};
use daw_engine::Session;

#[path = "common/multi_sampler.rs"]
mod multi_sampler;
use multi_sampler::{find_plugin, load_samples, peak, sine, wait_for_samples, NOTE_ONE, PLUGIN};

/// When the two instruments play (each has its sample's 0.2 s at 48 kHz in here).
const A: (i64, i64) = (0, 12_000);
const B: (i64, i64) = (24_000, 36_000);
const LEN: i64 = 48_000;
/// Renders that should be the same: the frozen instrument plays a render of the live one.
const SAME: f32 = 1e-3;

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn mix(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, LEN, 0, None, &mut |_| {}).unwrap()
}

/// One strip by itself, through its fader.
fn strip(s: &mut Session, id: Id) -> Vec<Vec<f32>> {
    let iso = s.isolate_track(id, true, true);
    s.render(0, LEN, 0, Some(iso), &mut |_| {}).unwrap()
}

/// The largest sample difference between two renders.
fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
}

fn clip_file(s: &Session, clip: Option<Id>) -> Option<PathBuf> {
    let c = s.project.clips.iter().find(|c| Some(c.id) == clip)?;
    let p = Path::new(&c.path);
    Some(if p.is_absolute() { p.to_path_buf() } else { s.dir.join(p) })
}

/// The freeze files of the instrument and its two auxes.
fn freeze_files(s: &Session, inst: Id, auxes: [Id; 2]) -> Vec<Option<PathBuf>> {
    let mut out = vec![clip_file(s, s.project.track(inst).and_then(|t| t.frozen).map(|f| f.clip))];
    out.extend(auxes.iter().map(|a| clip_file(s, s.project.track(*a).and_then(|t| t.frozen_output).map(|f| f.clip))));
    out
}

fn all_there(files: &[Option<PathBuf>]) -> bool {
    files.iter().all(|f| f.as_ref().is_some_and(|f| f.is_file()))
}

/// The sampler is loaded again (an undo or an unfreeze): waits for both its notes, making it load
/// once more through `again` when it lost a sample (issue #37).
fn plays_again(s: &mut Session, inst: Id, again: &mut dyn FnMut(&mut Session)) -> bool {
    for _ in 0..5 {
        if wait_for_samples(s, inst, A) && wait_for_samples(s, inst, B) {
            return true;
        }
        println!("     the plug-in lost a sample while loading; loading it again");
        again(s);
    }
    false
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let Some(desc) = find_plugin() else {
        println!("SKIP {PLUGIN} (lsp-plugins) isn't installed: nothing measured");
        return;
    };
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg.clone(), vec![desc.clone()]);
    s.dir = scratch.join("MultiOutFreeze.marbble");
    std::fs::create_dir_all(&s.dir).unwrap();
    let rate = s.project.sample_rate;
    let (one, two) = (scratch.join("one.wav"), scratch.join("two.wav"));
    sine(&one, 1000.0, rate);
    sine(&two, 2000.0, rate);

    // The instrument with a latent plug-in of its own, an aux on each of its first two outputs,
    // the first with a latent plug-in and lower, the second panned.
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.update_track(inst, TrackPatch { name: Some("Drums".into()), ..Default::default() });
    s.set_plugin(inst, usize::MAX, &desc.id).unwrap();
    let r = s.create_midi_region(inst, 0, LEN).unwrap();
    let note = |start: i64, pitch: u8| Note { start, length: 6000, pitch, velocity: 127, muted: false };
    s.set_midi_content(vec![MidiContent { region: r, notes: vec![note(A.0, NOTE_ONE), note(B.0, NOTE_ONE + 2)], cc: vec![], start: None, length: None }]);
    let auxes = [s.add_output_aux(inst).unwrap(), s.add_output_aux(inst).unwrap()];
    s.set_plugin(inst, 0, "builtin:linear-phase-eq").unwrap();
    s.set_plugin(auxes[0], 0, "builtin:linear-phase-eq").unwrap();
    s.update_track(auxes[0], TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    s.update_track(auxes[1], TrackPatch { pan: Some(0.5), ..Default::default() });
    load_samples(&mut s, inst, &one, &two);
    let loaded = plays_again(&mut s, inst, &mut |s| {
        s.set_plugin(inst, usize::MAX, &desc.id).unwrap();
        load_samples(s, inst, &one, &two);
    });
    check("the sampler loads its samples", loaded);
    if !loaded {
        std::process::exit(1);
    }
    let live = mix(&mut s);
    let live_auxes = [strip(&mut s, auxes[0]), strip(&mut s, auxes[1])];
    let aux_peaks = [peak(&live_auxes[0], A), peak(&live_auxes[1], B)];
    println!(
        "     live: mix peak {:.3}, aux 1 {:.3} (note A), aux 2 {:.3} (note B), latency {}",
        peak(&live, (0, LEN)),
        aux_peaks[0],
        aux_peaks[1],
        s.pdc_total()
    );
    check("live, each aux plays its output", aux_peaks.iter().all(|p| *p > 0.01) && peak(&live_auxes[0], B) < 1e-4 && peak(&live_auxes[1], A) < 1e-4);

    // Freeze.
    s.checkpoint("Freeze Track", None, true);
    let res = s.freeze_tracks(&[inst], &mut |_| {});
    check(&format!("an instrument with output auxes freezes ({:?})", res.as_ref().err().map(|e| e.to_string())), res.is_ok());
    let files = freeze_files(&s, inst, auxes);
    check(&format!("a freeze file for its main output and one per aux ({files:?})"), all_there(&files) && files[1] != files[2]);
    check("its plug-in is unloaded", s.plugin_instances() == 0 && s.instrument_outputs().is_empty());
    let frozen = mix(&mut s);
    let d = diff(&live, &frozen);
    check(&format!("frozen, the mix is the live one (max diff {d:.2e})"), d < SAME);
    for (k, &aux) in auxes.iter().enumerate() {
        let d = diff(&live_auxes[k], &strip(&mut s, aux));
        check(&format!("aux {} plays its output's render through its own strip (max diff {d:.2e})", k + 1), d < SAME);
    }
    s.update_track(auxes[0], TrackPatch { volume_db: Some(-12.0), ..Default::default() });
    let lower = 20.0 * (peak(&strip(&mut s, auxes[0]), A) / aux_peaks[0]).log10();
    s.update_track(auxes[0], TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    check(&format!("the aux's fader stays live ({lower:+.2} dB)"), (lower + 6.0).abs() < 0.05);
    s.update_track(inst, TrackPatch { off: Some(true), ..Default::default() });
    check("a frozen instrument switched off leaves its auxes silent", peak(&mix(&mut s), (0, LEN)) == 0.0);
    s.update_track(inst, TrackPatch { off: Some(false), ..Default::default() });

    // Undo and redo.
    s.undo();
    let back = plays_again(&mut s, inst, &mut |s| {
        s.redo();
        s.undo();
    });
    check("undoing the freeze, the sampler loads its samples again", back);
    let d = diff(&live, &mix(&mut s));
    let outputs = s.instrument_outputs().len();
    check(
        &format!("undo unfreezes: the plug-in and its outputs play live again (max diff {d:.2e}, {outputs} instrument with outputs)"),
        d < SAME && s.project.track(auxes[0]).unwrap().frozen_output.is_none() && outputs == 1,
    );
    s.redo();
    let d = diff(&frozen, &mix(&mut s));
    check(&format!("redo freezes it again (max diff {d:.2e})"), d < 1e-6 && all_there(&freeze_files(&s, inst, auxes)));

    // Saving and reopening a frozen project: nothing to load, the renders play.
    let saved = s.save(Some(scratch.join("MultiOutFreezeSaved.marbble"))).unwrap();
    let mut reopened = Session::new(cfg.clone(), vec![desc.clone()]);
    reopened.open(&saved).unwrap();
    let d = diff(&frozen, &mix(&mut reopened));
    check(&format!("reopened, it is still frozen and sounds the same (max diff {d:.2e})"), d < 1e-6 && reopened.plugin_instances() == 0);
    drop(reopened);

    // Unfreeze, and undo that. Unfreezing leaves the files for Undo and the saved project (#479).
    let files = freeze_files(&s, inst, auxes);
    s.checkpoint("Unfreeze Track", None, true);
    s.unfreeze_track(inst).unwrap();
    let gone: Vec<&PathBuf> = files.iter().flatten().filter(|f| !f.exists()).collect();
    check(&format!("unfreezing keeps every freeze file for Undo (gone: {gone:?})"), gone.is_empty());
    let still: Vec<Id> = auxes.into_iter().filter(|a| s.project.track(*a).unwrap().frozen_output.is_some()).collect();
    check(&format!("unfreezing gives the auxes their live outputs back (still on a render: {still:?})"), still.is_empty());
    let back = plays_again(&mut s, inst, &mut |s| {
        s.undo();
        s.redo();
    });
    check("unfrozen, the sampler loads its samples again", back);
    let d = diff(&live, &mix(&mut s));
    check(&format!("unfrozen, it plays live (max diff {d:.2e})"), d < SAME);
    s.undo();
    let d = diff(&frozen, &mix(&mut s));
    check(&format!("undoing the unfreeze plays the files again (max diff {d:.2e})"), d < 1e-6 && all_there(&freeze_files(&s, inst, auxes)));
    s.redo();
    plays_again(&mut s, inst, &mut |s| {
        s.undo();
        s.redo();
    });

    // Bounce in Place: the main output, with the auxes playing on, or taken along.
    let bounce = |s: &mut Session, outputs: bool| {
        let o = BipOptions { effects: true, fader: true, source: BipSource::Mute, tail: true, outputs };
        s.checkpoint("Bounce in Place", None, false);
        s.bounce_in_place(&[inst], &[], &o, &mut |_| {})
    };
    let made = bounce(&mut s, false);
    let d = diff(&live, &mix(&mut s));
    let auxes_on = auxes.iter().all(|a| !s.project.track(*a).unwrap().mute);
    check(&format!("Bounce in Place takes the main output; the auxes play on (max diff {d:.2e})"), made.is_ok() && d < SAME && auxes_on);
    s.undo();
    let made = bounce(&mut s, true);
    let bounced = made.as_ref().ok().and_then(|m| m.first().copied());
    let d = diff(&live, &mix(&mut s));
    let auxes_muted = auxes.iter().all(|a| s.project.track(*a).unwrap().mute);
    check(&format!("with Instrument Multi-Output the auxes are in the bounce and muted (max diff {d:.2e})"), d < SAME && auxes_muted);
    let alone = bounced.map(|b| strip(&mut s, b)).unwrap_or_default();
    check("the bounce has both notes, each with its aux", peak(&alone, A) > aux_peaks[0] && peak(&alone, B) > aux_peaks[1]);
    s.undo();
    plays_again(&mut s, inst, &mut |s| {
        s.redo();
        s.undo();
    });

    // Export Region: the main output, pre-fader, as before.
    let path = scratch.join("Region.wav");
    let o = RenderOptions { tail_secs: 0.0, bit_depth: BitDepth::Float32, ..Default::default() };
    let exported = s.export_region(r, &path, &o, &mut |_| {});
    let main = {
        let iso = s.isolate_track(inst, false, true);
        s.render(0, LEN, 0, Some(iso), &mut |_| {}).unwrap()
    };
    let file = exported.ok().and_then(|_| daw_engine::audio_file::decode::decode(&path).ok());
    let d = file.map(|f| diff(&main, &f.channels)).unwrap_or(f32::INFINITY);
    check(&format!("Export Region writes the main output (max diff {d:.2e})"), d < SAME);

    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}
