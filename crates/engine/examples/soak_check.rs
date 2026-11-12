//! Feature interaction soak (silent ALSA null output): builds the all-features project
//! (`common/all_features.rs`: VCAs, folder automation with curves, a pattern region, a MIDI
//! take folder, region parameters, an arrangement move, a marquee delete, Strip Silence, the
//! Linear Phase EQ, track alternatives…), then makes a few hundred edits drawn at random (fixed
//! seed, so every run makes the same ones) from the session's public edit methods, each one
//! undo step as the app makes them. After each edit the project must hold together (see
//! `invariants`); every so often the edit is undone and redone (bit-identical renders) and the
//! project saved and reopened (same render). At the end, undoing every edit must give back the
//! first render and document exactly, and redoing them all the last.
//! Plays on four threads throughout (`threads`, 1 for the callback alone): the null device's
//! callback runs every block through the worker pool while the edits land, renders go through the
//! pool too, and every so often one is compared with the serial loop's, bit for bit.
//! usage: soak_check <scratch dir> [edits] [seed] [threads] [tracks]
//! A seed makes the same edits every run: each choice comes from the seed and the document,
//! never the playhead or the clock. `edits.log` in the scratch folder has a line for each edit
//! (what it drew, the tracks and document it left, and renders), so two runs can be compared
//! line for line; at the end a second process makes the first 60 edits again and must log the
//! same (SOAK_REPLAY=<edits> for more, 0 for none; none by default in the large mode, which
//! would grow the project again). SOAK_LIVE_TOUCH=1 lets a built-in effect's knob moved in
//! Touch write its pass at the live playhead, as the app does, rather than writing seeded
//! points into its lane; such a run can't be replayed.
//! With `tracks`, the large mode: first duplicating tracks (one undo step each) up to that many,
//! which must render the same twice; the edits then duplicate no further, and a reopened project
//! must render the same twice too. A few hundred tracks take minutes per render (#112). Without
//! it, the edits duplicate up to 512 tracks.
//! To find the edit that broke a save and reopen, set SOAK_REOPEN_EVERY=1 (and
//! SOAK_REOPEN_FROM=<edit> to start checking there). When undoing them all doesn't give the
//! first document back, it names the edit whose undo went wrong; SOAK_DUMP=1 also writes the
//! documents before and after it, and what its undo gave, into the scratch folder.
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use daw_engine::audio_io::AudioConfig;
use daw_engine::dsp::graph::{GraphError, TaskGraph};
use daw_engine::dsp::ParallelTest;
use daw_engine::edit::arrange_cmds::SectionEdit;
use daw_engine::edit::fades::DragMode;
use daw_engine::edit::pattern::Step;
use daw_engine::edit::strip_silence::StripSettings;
use daw_engine::model::{AutoMode, AutoParam, Id, Note, Project, RegionContent, TrackKind, MASTER_ID};
use daw_engine::session::{MidiContent, RegionMove, RegionParamsPatch, TrackPatch, VcaPatch};
use daw_engine::Session;

#[path = "common/all_features.rs"]
mod all_features;
use all_features::*;

static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// xorshift64*: small, and the same sequence everywhere. Keeps what it hands out since the
/// last `take_drawn`, for the edit log.
struct Rng {
    state: u64,
    drawn: String,
}

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng { state: seed, drawn: String::new() }
    }
    fn next(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn note(&mut self, x: impl std::fmt::Display) {
        use std::fmt::Write;
        let _ = write!(self.drawn, " {x}");
    }
    fn below(&mut self, n: usize) -> usize {
        let x = (self.next() % n.max(1) as u64) as usize;
        self.note(x);
        x
    }
    fn range(&mut self, a: i64, b: i64) -> i64 {
        let x = a + (self.next() % (b - a).max(1) as u64) as i64;
        self.note(x);
        x
    }
    /// (Logged as one letter each, run together: `some` asks this once per track.)
    fn chance(&mut self, percent: u64) -> bool {
        let x = self.next() % 100 < percent;
        if !self.drawn.ends_with(['y', 'n']) {
            self.drawn.push(' ');
        }
        self.drawn.push(if x { 'y' } else { 'n' });
        x
    }
    fn pick<T: Clone>(&mut self, v: &[T]) -> Option<T> {
        (!v.is_empty()).then(|| v[self.below(v.len())].clone())
    }
    fn unit(&mut self) -> f32 {
        let x = (self.next() % 10_000) as f32 / 10_000.0;
        self.note(x);
        x
    }
    fn take_drawn(&mut self) -> String {
        std::mem::take(&mut self.drawn)
    }
}

fn render(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, LEN, 0, None, &mut |_| {}).unwrap()
}

/// A render by its hash, for the edit log.
fn hash_render(a: &[Vec<f32>]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    for x in a {
        x.len().hash(&mut h);
        x.iter().for_each(|v| v.to_bits().hash(&mut h));
    }
    h.finish()
}

fn same(a: &[Vec<f32>], b: &[Vec<f32>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits()))
}

fn first_diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> String {
    for (x, y) in a.iter().zip(b) {
        if let Some(i) = x.iter().zip(y).position(|(p, q)| p.to_bits() != q.to_bits()) {
            let worst = x.iter().zip(y).map(|(p, q)| (p - q).abs()).fold(0.0f32, f32::max);
            return format!("from frame {i} ({:.2} bars): {} vs {}, by up to {worst:.2e}", i as f64 / BAR as f64, x[i], y[i]);
        }
    }
    "in length".into()
}

fn doc(s: &Session) -> serde_json::Value {
    let mut v = serde_json::to_value(&s.project).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("drag_mode");
    o.remove("alternative");
    v
}

/// A document by its hash, and its text when `keep` (SOAK_DUMP), to compare and show later.
fn print(v: &serde_json::Value, keep: bool) -> (u64, Option<String>) {
    use std::hash::{Hash, Hasher};
    let text = v.to_string();
    let mut h = std::hash::DefaultHasher::new();
    text.hash(&mut h);
    (h.finish(), keep.then_some(text))
}

/// What every edit must leave true. Returns the first problem.
fn invariants(p: &Project) -> Option<String> {
    let mut ids = std::collections::HashSet::new();
    for t in &p.tracks {
        if !ids.insert(t.id) {
            return Some(format!("track id {} twice", t.id));
        }
        if let Some(v) = t.vca {
            if !p.vcas.iter().any(|x| x.id == v) {
                return Some(format!("track {} on a VCA that's gone", t.name));
            }
        }
        for s in t.inserts.iter().flatten() {
            if let Some(k) = s.sidechain {
                if p.track(k).is_none() {
                    return Some(format!("{} keyed from a track that's gone", s.name));
                }
            }
        }
        for lane in &t.automation {
            if lane.points.windows(2).any(|w| w[1].pos < w[0].pos) {
                return Some(format!("{}: automation points out of order", t.name));
            }
        }
        for r in t.regions.iter().chain(t.alternatives.iter().flat_map(|a| a.regions.iter())) {
            if !ids.insert(r.id) {
                return Some(format!("region id {} twice", r.id));
            }
            if r.length <= 0 || r.start < 0 {
                return Some(format!("region {} on {} at {} with length {}", r.name, t.name, r.start, r.length));
            }
            match (&r.content, t.kind) {
                (RegionContent::Audio { clip, fade_in, fade_out, .. }, TrackKind::Audio) => {
                    if r.takes.is_none() && !p.clips.iter().any(|c| c.id == *clip) {
                        return Some(format!("region {} plays clip {clip}, which isn't in the project", r.name));
                    }
                    if *fade_in < 0 || *fade_out < 0 {
                        return Some(format!("region {}: negative fade", r.name));
                    }
                }
                (RegionContent::Midi { notes, .. }, TrackKind::Instrument) => {
                    if let Some(n) = notes.iter().find(|n| n.length <= 0 || n.pitch > 127 || n.velocity > 127) {
                        return Some(format!("region {}: bad note {n:?}", r.name));
                    }
                }
                _ => return Some(format!("region {} doesn't belong on {:?} track {}", r.name, t.kind, t.name)),
            }
            if r.pattern.is_some() && !matches!(r.content, RegionContent::Midi { .. }) {
                return Some(format!("pattern region {} isn't MIDI", r.name));
            }
            if let Some(f) = &r.takes {
                if f.takes.is_empty() {
                    return Some(format!("take folder {} without takes", r.name));
                }
                if f.comp.windows(2).any(|w| w[1].start < w[0].end) || f.comp.iter().any(|c| c.end <= c.start) {
                    return Some(format!("take folder {}: comp out of order {:?}", r.name, f.comp));
                }
                if f.comp.iter().any(|c| f.take(c.take).is_none()) {
                    return Some(format!("take folder {}: comp names a take that's gone", r.name));
                }
                if !f.comps.is_empty() && (f.current >= f.comps.len() || f.comps[f.current].comp != f.comp) {
                    return Some(format!("take folder {}: comp {} of {} out of step", r.name, f.current, f.comps.len()));
                }
            }
        }
    }
    for v in &p.vcas {
        if v.parent.is_some_and(|q| !p.vcas.iter().any(|x| x.id == q)) {
            return Some(format!("{} nested in a VCA that's gone", v.name));
        }
        // Following the parents ends (the edits refuse loops).
        let mut next = v.parent;
        for _ in 0..=p.vcas.len() {
            next = next.and_then(|q| p.vcas.iter().find(|x| x.id == q)).and_then(|x| x.parent);
        }
        if next.is_some() {
            return Some(format!("{} is in a loop of VCAs", v.name));
        }
    }
    let a = &p.arrangement;
    if a.iter().any(|s| s.length <= 0 || s.start < 0) || a.windows(2).any(|w| w[1].start != w[0].end()) {
        return Some(format!("arrangement sections don't tile: {:?}", a.iter().map(|s| (s.start, s.length)).collect::<Vec<_>>()));
    }
    for lane in &p.master.automation {
        if lane.points.windows(2).any(|w| w[1].pos < w[0].pos) {
            return Some("Stereo Out automation out of order".into());
        }
    }
    None
}

/// Delay compensation must follow the plug-ins: every latent plug-in it counts is in the
/// project, on the strip it counts it on, and every Linear Phase EQ (always latent) is counted.
fn pdc_problem(s: &Session) -> Option<String> {
    let report = s.pdc_report();
    let slots = |track: Id| -> Vec<&daw_engine::model::PluginSlot> {
        if track == MASTER_ID {
            s.project.master.inserts.iter().flatten().collect()
        } else {
            s.project.track(track).map(|t| t.inserts.iter().flatten().chain(t.instrument.iter()).filter(|_| t.frozen.is_none()).collect()).unwrap_or_default()
        }
    };
    for st in &report.strips {
        for (uid, _) in &st.slots {
            match slots(st.track).iter().find(|x| x.uid == *uid) {
                None => return Some(format!("delay compensation counts plug-in {uid} on track {}, which isn't there", st.track)),
                // The built-in synth has no latency.
                Some(x) if x.plugin_id == daw_engine::session::BUILTIN_SYNTH => {
                    return Some(format!("delay compensation gives the synth {uid} on track {} a latency", st.track))
                }
                _ => {}
            }
        }
    }
    for t in s.project.tracks.iter().map(|t| t.id).chain([MASTER_ID]) {
        // A track that's off takes no part.
        if s.project.is_off(t) {
            continue;
        }
        for x in slots(t).into_iter().filter(|x| x.plugin_id == "builtin:linear-phase-eq") {
            if !report.strips.iter().any(|st| st.track == t && st.slots.iter().any(|(u, _)| *u == x.uid)) {
                return Some(format!("delay compensation leaves out the Linear Phase EQ {} on track {t}", x.uid));
            }
        }
    }
    None
}

/// The regions of the project, with their tracks.
fn regions(s: &Session) -> Vec<(Id, Id)> {
    s.project.tracks.iter().flat_map(|t| t.regions.iter().map(move |r| (t.id, r.id))).collect()
}

fn regions_where(s: &Session, f: impl Fn(&daw_engine::model::Region) -> bool) -> Vec<Id> {
    s.project.tracks.iter().flat_map(|t| t.regions.iter()).filter(|r| f(r)).map(|r| r.id).collect()
}

fn tracks_of(s: &Session, f: impl Fn(&daw_engine::model::Track) -> bool) -> Vec<Id> {
    s.project.tracks.iter().filter(|t| f(t)).map(|t| t.id).collect()
}

/// How far Repeat Regions may put copies. Repeating regions spread along the timeline
/// multiplies its length, so unchecked a few hundred edits made a project days long, whose
/// pattern regions joined into hundreds of thousands of notes and filled the memory (#434).
const REPEAT_UP_TO: i64 = 16 * LEN;

/// How many tracks Duplicate Tracks may make, outside the large mode. Each duplication can double
/// a selection, so a long run grew thousands of tracks and filled the memory undoing them (#486).
const TRACKS_UP_TO: usize = 512;

/// Where the last copy would end if `ids` were repeated `times` times.
fn repeat_end(s: &Session, ids: &[Id], times: usize) -> i64 {
    let rs: Vec<_> = s.project.tracks.iter().flat_map(|t| t.regions.iter()).filter(|r| ids.contains(&r.id)).collect();
    let (Some(lo), Some(hi)) = (rs.iter().map(|r| r.start).min(), rs.iter().map(|r| r.end()).max()) else { return 0 };
    hi + (hi - lo) * times as i64
}

fn random_points(g: &mut Rng, lo: f32, hi: f32) -> Vec<daw_engine::model::AutoPoint> {
    let n = 1 + g.below(5);
    let mut pos: Vec<i64> = (0..n).map(|_| g.range(0, LEN)).collect();
    pos.sort();
    pos.into_iter().map(|p| pt(p, lo + (hi - lo) * g.unit(), if g.chance(50) { g.unit() * 2.0 - 1.0 } else { 0.0 })).collect()
}

/// How the edits are made, beyond the seed.
struct Opts {
    /// Duplicating stops at this many tracks (the large mode).
    max_tracks: Option<usize>,
    /// A built-in effect's knob moved in Touch writes its pass at the live playhead, as in the
    /// app (SOAK_LIVE_TOUCH), so the run can't be replayed.
    live_touch: bool,
}

/// One random edit, made as the app makes it: a checkpoint, then the edit (an error abandons
/// the checkpoint). A move recorded as automation takes none, as in the app: its pass makes the
/// step. Returns its name, and whether it left an undo step.
///
/// Every choice comes from `g` and the document, never from the playhead or the clock, so a
/// seed makes the same edits every time (the edit log shows it).
fn edit(s: &mut Session, g: &mut Rng, w: &World, o: &Opts) -> (String, bool) {
    let all = regions(s);
    let audio = regions_where(s, |r| matches!(r.content, RegionContent::Audio { .. }));
    let midi = regions_where(s, |r| matches!(r.content, RegionContent::Midi { .. }));
    let folders = regions_where(s, |r| r.takes.is_some());
    let patterns = regions_where(s, |r| r.pattern.is_some());
    let tracks = tracks_of(s, |_| true);
    let leaves = tracks_of(s, |t| !t.kind.is_stack());
    let instruments = tracks_of(s, |t| t.kind == TrackKind::Instrument);
    let strips = tracks_of(s, |t| t.kind != TrackKind::Folder);
    let region = g.pick(&all).map(|x| x.1).unwrap_or(0);
    let some = |g: &mut Rng, v: &[Id]| -> Vec<Id> { v.iter().copied().filter(|_| g.chance(40)).collect() };
    let (a, b) = {
        let x = g.range(0, LEN);
        let y = g.range(0, LEN);
        (x.min(y), x.max(y).max(x.min(y) + 1000))
    };
    let kind = g.below(64);
    let plugins = matches!(kind, 40..=42 | 50 | 51);
    let label = format!("edit {kind}");
    s.checkpoint(&label, None, plugins);
    let mut checkpointed = true;
    let r: anyhow::Result<()> = (|| {
        match kind {
            0 => s.move_region(region, g.range(0, LEN), None),
            1 => {
                let st = g.range(0, LEN);
                s.resize_region(region, st, g.range(100, 2 * BAR));
            }
            2 => s.delete_regions(&[region]),
            3 => s.split_at(g.range(0, LEN), &[region]),
            4 => s.set_region_muted(region, g.chance(50)),
            5 => {
                let end = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.id == region).map(|r| r.end()).unwrap_or(0);
                s.set_region_loop(region, if g.chance(70) { Some(end + g.range(1, 2 * BAR)) } else { None });
            }
            6 => {
                let ids = some(g, &all.iter().map(|x| x.1).collect::<Vec<_>>());
                let times = 1 + g.below(2);
                if repeat_end(s, &ids, times) <= REPEAT_UP_TO {
                    s.repeat_regions(&ids, times);
                }
            }
            7 => s.set_region_gains(&[(region, -12.0 + 18.0 * g.unit())]),
            8 => s.set_region_fades(&[region], Some(g.range(0, 20_000)), Some(g.range(0, 20_000))),
            9 => s.transpose_regions(&[region], g.range(-5, 6) as i32),
            10 => s.quantize_regions(&[region], BAR / 8),
            11 => {
                let p = RegionParamsPatch {
                    quantize: g.chance(30).then(|| [0u16, 8, 12, 16][g.below(4)]),
                    swing: g.chance(20).then(|| 30 + g.below(50) as u8),
                    transpose: g.chance(30).then(|| g.range(-12, 12) as i16),
                    velocity: g.chance(30).then(|| g.range(-40, 40) as i16),
                    dynamics: g.chance(20).then(|| [0u16, 50, 100, 200][g.below(4)]),
                    gate: g.chance(20).then(|| [50u16, 100, 150][g.below(3)]),
                    delay: g.chance(40).then(|| g.range(-200, 200) as f64),
                    delay_ms: g.chance(15).then(|| g.chance(50)),
                };
                s.set_region_params(&[region], &p);
            }
            12 => s.apply_region_params(&[region]),
            13 => s.marquee_delete(&some(g, &tracks), a, b.min(a + 2 * BAR)),
            14 => {
                s.marquee_split(&some(g, &tracks), a, b.min(a + 2 * BAR));
            }
            15 => {
                let e = [SectionEdit::InsertSilence, SectionEdit::Cut, SectionEdit::Repeat][g.below(3)];
                s.section_edit(e, a, b.min(a + BAR), None)?;
            }
            16 => {
                let secs: Vec<Id> = s.project.arrangement.iter().map(|x| x.id).collect();
                if let Some(id) = g.pick(&secs) {
                    let before = if g.chance(30) { None } else { g.pick(&secs) };
                    s.move_arrangement_marker(id, before, g.chance(30))?;
                }
            }
            17 => {
                let secs: Vec<Id> = s.project.arrangement.iter().map(|x| x.id).collect();
                if let Some(id) = g.pick(&secs) {
                    s.delete_arrangement_marker(id, g.chance(50))?;
                }
            }
            18 => {
                let id = s.add_arrangement_marker(None);
                s.resize_arrangement_marker(id, g.range(BAR / 2, 2 * BAR));
            }
            19 => {
                let ids = some(g, &audio);
                s.strip_silence(&ids, &StripSettings { threshold: 0.005 + 0.05 * g.unit(), min_time: 0.05, pre_attack: 0.005, post_release: 0.01 });
            }
            20 => {
                s.set_snap_zero_crossings(g.chance(60));
                s.split_at(g.range(0, LEN), &[region]);
            }
            21 => s.crossfade_regions(&some(g, &audio), g.range(100, 10_000), None),
            22 => s.set_loop_crossfade(&[region], g.chance(70)),
            23 => {
                // Two or more audio regions on one track.
                let t = g.pick(&tracks_of(s, |t| t.kind == TrackKind::Audio)).unwrap_or(0);
                let ids: Vec<Id> = s.project.track(t).map(|t| t.regions.iter().filter(|r| r.takes.is_none()).map(|r| r.id).collect()).unwrap_or_default();
                if ids.len() >= 2 {
                    s.pack_take_folder(&ids[..2 + g.below(ids.len() - 1)])?;
                }
            }
            24 => {
                if let Some(f) = g.pick(&folders) {
                    match g.below(6) {
                        0 => s.create_comp(f),
                        1 => s.duplicate_comp(f),
                        2 => s.select_comp(f, g.below(3)),
                        3 => s.delete_comp(f, g.below(3)),
                        4 => s.rename_comp(f, g.below(3), "Soak".into()),
                        _ => {
                            let takes: Vec<Id> = s
                                .project
                                .tracks
                                .iter()
                                .flat_map(|t| t.regions.iter())
                                .find(|r| r.id == f)
                                .map(|r| r.takes.as_ref().unwrap().takes.iter().map(|t| t.id).collect())
                                .unwrap_or_default();
                            s.select_take(f, g.pick(&takes).unwrap_or(0));
                        }
                    }
                }
            }
            25 => {
                if let Some(f) = g.pick(&folders) {
                    let r = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.id == f).unwrap().clone();
                    let takes: Vec<Id> = r.takes.as_ref().unwrap().takes.iter().map(|t| t.id).collect();
                    let x = g.range(r.start, r.end());
                    s.set_comp_range(f, if g.chance(80) { g.pick(&takes) } else { None }, x, (x + g.range(1000, BAR)).min(r.end()));
                }
            }
            26 => {
                if let Some(f) = g.pick(&folders) {
                    match g.below(4) {
                        0 => {
                            s.flatten_take_folder(f);
                        }
                        1 => {
                            s.unpack_take_folder(f);
                        }
                        2 => s.delete_unused_takes(f),
                        _ => {
                            let take = s
                                .project
                                .tracks
                                .iter()
                                .flat_map(|t| t.regions.iter())
                                .find(|r| r.id == f)
                                .and_then(|r| r.takes.as_ref()?.takes.first().map(|t| t.id));
                            s.delete_take(f, take.unwrap_or(0));
                        }
                    }
                }
            }
            27 => {
                if let Some(t) = g.pick(&instruments) {
                    s.create_pattern_region(t, g.range(0, LEN - BAR), g.range(BAR / 2, 2 * BAR))?;
                }
            }
            28 => {
                if let Some(id) = g.pick(&patterns) {
                    let mut p = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.id == id).unwrap().pattern.clone().unwrap();
                    if let Some(row) = p.rows.get_mut(g.below(4)) {
                        let k = g.below(16);
                        if row.steps.len() <= k {
                            row.steps.resize(k + 1, Step::default());
                        }
                        row.steps[k].on = !row.steps[k].on;
                    }
                    p.swing = (g.below(70)) as u8;
                    if g.chance(20) {
                        p.length = [8u16, 12, 16, 32][g.below(4)];
                    }
                    s.set_pattern(id, p)?;
                }
            }
            29 => {
                let ids = some(g, &patterns);
                s.convert_patterns_to_midi(&ids);
            }
            30 => {
                let ids = some(g, &midi);
                s.convert_midi_to_patterns(&ids);
            }
            31 => {
                let v = s.create_vca(&some(g, &strips));
                s.update_vca(v, VcaPatch { level_db: Some(-20.0 + 26.0 * g.unit()), ..Default::default() });
            }
            32 => {
                let vcas: Vec<Id> = s.project.vcas.iter().map(|v| v.id).collect();
                if let Some(v) = g.pick(&vcas) {
                    match g.below(4) {
                        0 => s.update_vca(v, VcaPatch { level_db: Some(-30.0 + 36.0 * g.unit()), ..Default::default() }),
                        1 => s.update_vca(v, VcaPatch { mute: Some(g.chance(50)), ..Default::default() }),
                        2 => s.delete_vca(v),
                        _ => s.set_vca_parent(v, if g.chance(70) { g.pick(&vcas) } else { None })?,
                    }
                }
            }
            33 => {
                let vcas: Vec<Id> = s.project.vcas.iter().map(|v| v.id).collect();
                let v = if g.chance(80) { g.pick(&vcas) } else { None };
                s.assign_vca(&some(g, &tracks), v)?;
            }
            34 => {
                // Stereo Out and the first folder stack more often than the rest.
                let t = match g.below(8) {
                    0 => MASTER_ID,
                    1 if s.project.track(w.folder).is_some() => w.folder,
                    _ => g.pick(&tracks).unwrap_or(MASTER_ID),
                };
                let (param, lo, hi) = match g.below(3) {
                    0 => (AutoParam::Volume, -30.0, 6.0),
                    1 => (AutoParam::Pan, -1.0, 1.0),
                    _ => (AutoParam::Mute, 0.0, 1.0),
                };
                s.set_automation_lane(t, param, random_points(g, lo, hi));
            }
            35 => {
                let t = if g.chance(15) { MASTER_ID } else { g.pick(&tracks).unwrap_or(MASTER_ID) };
                let plugs: Vec<(Id, u32)> = s
                    .project
                    .track(t)
                    .map(|t| t.inserts.iter().flatten().filter(|x| x.plugin_id.starts_with("builtin:")).map(|x| (x.uid, 0)).collect())
                    .unwrap_or_default();
                if let Some((uid, param)) = g.pick(&plugs) {
                    s.set_automation_lane(t, AutoParam::Plugin { uid, param, name: "P".into() }, random_points(g, 0.0, 1.0));
                } else {
                    s.clear_automation(t, None);
                }
            }
            36 => {
                let t = if g.chance(20) { MASTER_ID } else { g.pick(&tracks).unwrap_or(MASTER_ID) };
                s.set_auto_mode(t, [AutoMode::Read, AutoMode::Off, AutoMode::Read, AutoMode::Touch][g.below(4)]);
            }
            37 => {
                if let Some(t) = g.pick(&leaves) {
                    let names: Vec<String> = s.project.track(t).unwrap().alternatives.iter().map(|a| a.name.clone()).collect();
                    match g.below(4) {
                        0 => {
                            s.new_track_alternative(t, g.chance(60))?;
                        }
                        1 => {
                            if let Some(n) = g.pick(&names) {
                                s.switch_track_alternative(t, &n)?;
                            }
                        }
                        2 => {
                            if let Some(n) = g.pick(&names) {
                                s.delete_track_alternative(t, &n)?;
                            }
                        }
                        _ => {
                            if let Some(n) = g.pick(&names) {
                                s.rename_track_alternative(t, &n, &format!("{n}x"))?;
                            }
                        }
                    }
                }
            }
            38 => {
                let ids = some(g, &all.iter().map(|x| x.1).collect::<Vec<_>>());
                match g.below(9) {
                    0 => s.remove_gaps(&ids),
                    1 => s.remove_overlaps(&ids),
                    2 => s.trim_to_next_region(&ids),
                    3 => s.fill_within_locators(&ids, a, b),
                    4 => s.resize_to_same_length(&ids),
                    5 => s.trim_regions_to(&ids, g.range(0, LEN), g.chance(50)),
                    6 => {
                        s.crop_regions(&ids, a, b)?;
                    }
                    7 => s.delete_and_shuffle(&ids),
                    _ => s.move_regions_to(&ids, g.range(0, LEN)),
                }
            }
            39 => {
                s.set_drag_mode([DragMode::Overlap, DragMode::NoOverlap, DragMode::ShuffleR, DragMode::ShuffleL, DragMode::XFade][g.below(5)]);
                match g.below(4) {
                    0 => s.drag_nudge_regions(&[region], g.range(-BAR, BAR), Some(g.chance(50))),
                    1 => s.drag_delete_regions(&[region]),
                    2 => {
                        let st = g.range(0, LEN);
                        s.drag_resize_region(region, st, g.range(1000, 2 * BAR));
                    }
                    _ => {
                        if let Some((t, id)) = g.pick(&all) {
                            s.drag_move_regions(&[RegionMove { id, start: g.range(0, LEN), track: Some(t) }], Some(g.chance(50)));
                        }
                    }
                }
            }
            40 => {
                let fx = [
                    "builtin:chorus",
                    "builtin:flanger",
                    "builtin:phaser",
                    "builtin:tremolo",
                    "builtin:overdrive",
                    "builtin:distortion",
                    "builtin:gain",
                    "builtin:linear-phase-eq",
                    "builtin:compressor",
                ];
                if let Some(t) = g.pick(&strips) {
                    s.set_plugin(t, 4 + g.below(4), fx[g.below(fx.len())])?;
                }
            }
            41 => {
                if let Some(t) = g.pick(&strips) {
                    s.remove_plugin(t, g.below(8));
                }
            }
            42 => {
                if let Some(t) = g.pick(&tracks) {
                    if s.project.tracks.len() > 3 {
                        s.delete_track(t);
                    }
                }
            }
            43 => {
                if let Some(t) = g.pick(&strips) {
                    let slots: Vec<(usize, String)> =
                        s.project.track(t).unwrap().inserts.iter().enumerate().filter_map(|(k, x)| x.as_ref().map(|x| (k, x.plugin_id.clone()))).collect();
                    if let Some((k, id)) = g.pick(&slots) {
                        if let Some(info) = daw_engine::fx::find(&id) {
                            let p = &info.params[g.below(info.params.len())];
                            let value = p.min + (p.max - p.min) * g.unit();
                            let touch = s.project.track(t).is_some_and(|t| t.auto_mode.writes());
                            if touch && !o.live_touch {
                                // Playing in Touch, the move would write a pass at the playhead,
                                // wherever the engine has got to: the lane gets seeded points
                                // instead.
                                let uid = s.project.track(t).unwrap().inserts[k].as_ref().unwrap().uid;
                                s.set_automation_lane(t, AutoParam::Plugin { uid, param: p.id, name: p.name.into() }, random_points(g, 0.0, 1.0));
                            } else {
                                // In Touch, the pass goes into its lane, as one step, straight away.
                                if s.automation_writing(t) {
                                    s.abandon_checkpoint();
                                    checkpointed = false;
                                }
                                s.set_builtin_param(t, k, p.id, value)?;
                            }
                        }
                    }
                }
            }
            44 => {
                if let Some(t) = g.pick(&tracks) {
                    s.update_track(
                        t,
                        TrackPatch {
                            volume_db: Some(-20.0 + 26.0 * g.unit()),
                            mute: g.chance(20).then(|| g.chance(30)),
                            pan: Some(g.unit() * 2.0 - 1.0),
                            off: g.chance(20).then(|| g.chance(30)),
                            ..Default::default()
                        },
                    );
                }
            }
            45 => {
                let ids = some(g, &leaves);
                s.create_stack(&ids, g.chance(40));
            }
            46 => {
                let stacks = tracks_of(s, |t| t.kind.is_stack());
                if let Some(t) = g.pick(&stacks) {
                    s.flatten_stack(t);
                }
            }
            47 => {
                let mut ids = some(g, &leaves);
                ids.truncate(o.max_tracks.unwrap_or(TRACKS_UP_TO).saturating_sub(tracks.len()));
                s.duplicate_tracks(&ids, g.chance(60));
            }
            48 => {
                if let Some(id) = g.pick(&midi) {
                    let r = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.id == id).unwrap().clone();
                    let RegionContent::Midi { notes, cc } = &r.content else { unreachable!() };
                    let mut notes = notes.to_vec();
                    match g.below(3) {
                        0 => notes.push(Note {
                            start: g.range(0, r.length),
                            length: g.range(100, BAR),
                            pitch: 40 + g.below(40) as u8,
                            velocity: 1 + g.below(126) as u8,
                            muted: false,
                        }),
                        1 if !notes.is_empty() => {
                            notes.remove(g.below(notes.len()));
                        }
                        _ => {
                            for n in notes.iter_mut() {
                                if g.chance(30) {
                                    n.start = (n.start + g.range(-2000, 2000)).max(0);
                                }
                            }
                        }
                    }
                    s.set_midi_content(vec![MidiContent { region: id, notes, cc: cc.to_vec(), start: None, length: None }]);
                }
            }
            49 => {
                if let Some(t) = g.pick(&instruments) {
                    s.create_midi_region(t, g.range(0, LEN - BAR), g.range(BAR / 4, 2 * BAR))?;
                }
            }
            50 => {
                if let Some(t) = g.pick(&strips) {
                    let (from, to) = (g.below(8), g.below(8));
                    s.move_insert(t, from, to)?;
                }
            }
            51 => {
                let comps: Vec<(Id, usize)> = s
                    .project
                    .tracks
                    .iter()
                    .flat_map(|t| {
                        t.inserts
                            .iter()
                            .enumerate()
                            .filter(|(_, x)| x.as_ref().is_some_and(|x| x.plugin_id == "builtin:compressor"))
                            .map(move |(k, _)| (t.id, k))
                    })
                    .collect();
                if let Some((t, k)) = g.pick(&comps) {
                    s.set_sidechain(t, k, g.pick(&strips))?;
                    s.set_sidechain_pre(t, k, g.chance(50))?;
                }
            }
            52 => {
                let ids = some(g, &all.iter().map(|x| x.1).collect::<Vec<_>>());
                s.drag_nudge_musical(&ids, g.range(0, LEN), g.range(0, LEN), Some(false));
            }
            53 => s.set_tempo_at(g.range(BAR, LEN), 80.0 + 80.0 * g.unit() as f64, g.chance(50)),
            54 => {
                let ids = some(g, &midi);
                if ids.len() >= 2 {
                    s.join_regions(&ids)?;
                }
            }
            55 => s.set_region_fade_shape(&[region], Some(g.unit() * 2.0 - 1.0), Some(g.unit() * 2.0 - 1.0), None, None),
            56 => s.nudge_regions(&[region], g.range(-BAR / 4, BAR / 4)),
            57 => {
                let ids = some(g, &all.iter().map(|x| x.1).collect::<Vec<_>>());
                let items: Vec<(Id, daw_engine::model::Region)> =
                    s.marquee_copy(&tracks, a, b.min(a + BAR)).into_iter().filter_map(|(k, r)| s.project.tracks.get(k).map(|t| (t.id, r))).collect();
                if g.chance(50) {
                    s.paste_replace(&items, g.range(0, BAR));
                } else {
                    s.paste_regions(&items, g.range(0, BAR));
                }
                let _ = ids;
            }
            58 => {
                let ids = some(g, &audio);
                s.split_regions_at(&ids, &[g.range(0, LEN), g.range(0, LEN)]);
            }
            59 => s.set_cycle(true, a, b),
            60 => {
                let lanes = vec![(g.pick(&tracks).unwrap_or(MASTER_ID), vec![AutoParam::Pan, AutoParam::Volume])];
                s.set_auto_lanes(lanes);
            }
            61 => {
                s.regions_to_new_track(&[region]);
            }
            62 => {
                let ids = some(g, &tracks);
                s.move_tracks(ids, &[]);
            }
            _ => s.set_master_volume(-12.0 + 12.0 * g.unit()),
        }
        Ok(())
    })();
    let ok = r.is_ok();
    if !ok && checkpointed {
        s.abandon_checkpoint();
    }
    (format!("{label}{}", r.err().map(|e| format!(" (refused: {e})")).unwrap_or_default()), ok)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let count: usize = args.next().map(|x| x.parse().unwrap()).unwrap_or(300);
    let seed: u64 = args.next().map(|x| x.parse().unwrap()).unwrap_or(0x5eed_f00d);
    let threads: u32 = args.next().map(|x| x.parse().unwrap()).unwrap_or(4);
    let grow: Option<usize> = args.next().map(|x| x.parse().unwrap());
    let opts = Opts { max_tracks: grow, live_touch: std::env::var_os("SOAK_LIVE_TOUCH").is_some() };
    // How many of the edits a second process makes again, which must come out the same.
    let replay: usize =
        std::env::var("SOAK_REPLAY").ok().and_then(|v| v.parse().ok()).unwrap_or(if grow.is_none() && !opts.live_touch { 60 } else { 0 }).min(count);
    // One line for each edit: what it drew, and the document (and render) it made. Written as
    // it goes, to follow a long run.
    let mut log = std::io::LineWriter::new(std::fs::File::create(scratch.join("edits.log")).expect("edit log"));
    let mut note = |line: String| writeln!(log, "{line}").unwrap();
    // Patient: a loaded machine may keep a worker from a block for as long as it likes.
    let pool_on = |s: &mut Session| {
        if threads > 1 {
            s.set_threads_for_checks(threads, ParallelTest { always: true, renders: true, patient: true });
        }
    };
    let started = std::time::Instant::now();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg.clone(), Vec::new());
    s.prefs.general.undo_limit = 100_000;
    s.dir = scratch.join("Soak.marbble");
    let mut w = base(&mut s, &scratch);
    for (name, step) in steps() {
        s.checkpoint(name, None, true);
        step(&mut s, &mut w);
    }
    pool_on(&mut s);
    s.set_cycle(true, 0, LEN);
    s.play();
    s.idle();
    check(&format!("the all-features project holds together ({:?})", invariants(&s.project)), invariants(&s.project).is_none());
    let first = render(&mut s);
    let first_doc = doc(&s);
    note(format!("seed {seed:#x}, {threads} threads, up to {grow:?} tracks"));
    note(format!("the all-features project: doc {:016x}, render {:016x}", print(&first_doc, false).0, hash_render(&first)));

    // How often the project is saved and reopened (SOAK_REOPEN_EVERY, for hunting a failure
    // down to its edit).
    let every: usize = std::env::var("SOAK_REOPEN_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(50).max(1);
    let from: usize = std::env::var("SOAK_REOPEN_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut g = Rng::new(seed);
    let mut depth = 0usize;
    let mut redo = 0usize;
    let mut broken = None;
    let mut checked = 0;
    let mut compared = 0;
    // The document at each depth of the undo stack, with the edit that made it, to name the
    // edit whose undo doesn't give back the document before it (SOAK_DUMP writes the three
    // documents into the scratch folder).
    let dump = std::env::var_os("SOAK_DUMP").is_some();
    let mut docs = vec![(print(&first_doc, dump), "the all-features project".to_string())];

    // The large mode: duplicating tracks, one undo step each, up to `grow` (the edits then
    // duplicate no further). #112 showed only in a project of hundreds of tracks.
    if let Some(n) = grow {
        let grew = std::time::Instant::now();
        let mut k = 0;
        while s.project.tracks.len() < n {
            let mut ids: Vec<Id> = tracks_of(&s, |t| !t.kind.is_stack()).into_iter().filter(|_| g.chance(50)).collect();
            ids.truncate(n - s.project.tracks.len());
            if ids.is_empty() {
                continue;
            }
            s.checkpoint("grow", None, false);
            s.duplicate_tracks(&ids, g.chance(60));
            depth += 1;
            let v = print(&doc(&s), dump);
            note(format!("grow {k}:{} -> {} tracks, doc {:016x}", g.take_drawn(), s.project.tracks.len(), v.0));
            docs.push((v, format!("growing {k}")));
            k += 1;
        }
        s.idle();
        let problem = invariants(&s.project).or_else(|| pdc_problem(&s));
        check(&format!("the project grew to {} tracks in {k} duplications ({:?})", s.project.tracks.len(), problem), problem.is_none());
        let a = render(&mut s);
        let b = render(&mut s);
        check(
            &format!(
                "the large project renders the same twice ({}; {:.0} s to grow and render)",
                if same(&a, &b) { "bit-identical".into() } else { first_diff(&a, &b) },
                grew.elapsed().as_secs_f64()
            ),
            same(&a, &b),
        );
        note(format!("grown: render {:016x}", hash_render(&a)));
    }
    for i in 0..count {
        // Now and then an undo or redo of earlier edits, as a user would.
        let undid = if depth > 0 && g.chance(8) {
            s.undo();
            depth -= 1;
            redo += 1;
            Some("undo")
        } else if redo > 0 && g.chance(30) {
            s.redo();
            depth += 1;
            redo -= 1;
            Some("redo")
        } else {
            None
        };
        if let Some(what) = undid {
            note(format!("{i:>4} {what}:{} -> doc {:016x}", g.take_drawn(), print(&doc(&s), false).0));
            if let Some(problem) = invariants(&s.project).or_else(|| pdc_problem(&s)) {
                check(&format!("after {what} {i} the project holds together: {problem}"), false);
                broken = Some(i);
                break;
            }
            continue;
        }
        let before = (i % 20 == 0).then(|| (render(&mut s), doc(&s)));
        let made = catch_unwind(AssertUnwindSafe(|| edit(&mut s, &mut g, &w, &opts)));
        let (name, stepped) = match made {
            Ok(x) => x,
            Err(_) => {
                check(&format!("edit {i} doesn't panic"), false);
                broken = Some(i);
                break;
            }
        };
        let now = print(&doc(&s), dump);
        note(format!("{i:>4} {name}:{} -> {} tracks, doc {:016x}", g.take_drawn(), s.project.tracks.len(), now.0));
        if stepped {
            depth += 1;
            redo = 0;
            docs.truncate(depth);
            docs.push((now, format!("edit {i} ({name})")));
        }
        if i % 7 == 0 {
            s.idle();
        }
        // The session's graph must keep describing what the engine does, or it plays serially for
        // good. A project too big for a graph plays serially anyway (Duplicate Tracks grows a long
        // run's past it).
        let fits = !matches!(TaskGraph::of_project(&s.project), Err(GraphError::TooBig(_)));
        if threads > 1 && fits && !s.pool_usable_for_checks() {
            check(&format!("after edit {i} ({name}) the task graph fits the engine's routes"), false);
            broken = Some(i);
            break;
        }
        if let Some(problem) = invariants(&s.project).or_else(|| pdc_problem(&s)) {
            check(&format!("after edit {i} ({name}) the project holds together: {problem}"), false);
            broken = Some(i);
            break;
        }
        if let Some((before, before_doc)) = before {
            // Undo and redo this edit, with renders.
            let after = match catch_unwind(AssertUnwindSafe(|| render(&mut s))) {
                Ok(a) => a,
                Err(_) => {
                    check(&format!("rendering after edit {i} ({name}) doesn't panic"), false);
                    broken = Some(i);
                    break;
                }
            };
            let after_doc = doc(&s);
            note(format!("{i:>4}   render {:016x}", hash_render(&after)));
            if stepped {
                s.undo();
                let undone = render(&mut s);
                let ok = same(&undone, &before) && doc(&s) == before_doc;
                let document = |same: bool| if same { "the same document" } else { "another document" };
                if !ok {
                    check(
                        &format!("undo of edit {i} ({name}) renders as before ({}; {})", first_diff(&undone, &before), document(doc(&s) == before_doc)),
                        false,
                    );
                }
                s.redo();
                let redone = render(&mut s);
                if !same(&redone, &after) || doc(&s) != after_doc {
                    check(&format!("redo of edit {i} ({name}) renders as after ({}; {})", first_diff(&redone, &after), document(doc(&s) == after_doc)), false);
                }
                checked += 1;
            }
        }
        if threads > 1 && i % 40 == 0 {
            let pooled = render(&mut s);
            s.set_threads_for_checks(1, ParallelTest::default());
            let serial = render(&mut s);
            pool_on(&mut s);
            if !same(&pooled, &serial) {
                check(&format!("after edit {i} ({name}) the pool renders as the serial loop does ({})", first_diff(&pooled, &serial)), false);
            }
            compared += 1;
        }
        if i % every == every - 1 && i >= from {
            let now = render(&mut s);
            let dir = scratch.join(format!("Soak{i}.marbble"));
            let saved = s.save(Some(dir)).unwrap();
            // Saving under a new name renames the project.
            docs[depth].0 = print(&doc(&s), dump);
            let mut t = Session::new(cfg.clone(), Vec::new());
            pool_on(&mut t);
            t.open(&saved).unwrap();
            let reopened = render(&mut t);
            note(format!("{i:>4}   saved and reopened: render {:016x}", hash_render(&reopened)));
            // In the large mode, again: #112's first render of a reopened project differed from the next.
            if grow.is_some() {
                let again = render(&mut t);
                check(
                    &format!(
                        "after edit {i} ({name}), the reopened project renders the same twice ({})",
                        if same(&reopened, &again) { "bit-identical".into() } else { first_diff(&reopened, &again) }
                    ),
                    same(&reopened, &again),
                );
            }
            check(
                &format!(
                    "after edit {i} ({name}), the project saves and reopens with the same render ({})",
                    if same(&reopened, &now) { "bit-identical".into() } else { first_diff(&reopened, &now) }
                ),
                same(&reopened, &now),
            );
            drop(t);
        }
    }
    check(&format!("{count} random edits made without a panic or a broken project (seed {seed:#x})"), broken.is_none());
    println!("  {depth} undo steps deep; {checked} edits undone and redone with renders");
    if threads > 1 {
        s.stop();
        check(&format!("{compared} renders on {threads} threads matched the serial loop's"), compared > 0);
        let pooled = s.status.parallel_blocks.load(std::sync::atomic::Ordering::Relaxed);
        check(
            &format!("the pool played throughout ({pooled} blocks), and no strip panicked"),
            pooled > 0 && s.status.engine_errors.load(std::sync::atomic::Ordering::Relaxed) == 0,
        );
    }

    let last = render(&mut s);
    let last_doc = doc(&s);
    let mut diverged = None;
    for k in (0..depth).rev() {
        s.undo();
        let now = print(&doc(&s), dump);
        if diverged.is_none() && now.0 != docs[k].0 .0 {
            diverged = Some(k);
            let pretty = |x: &Option<String>| serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(x.as_deref().unwrap()).unwrap()).unwrap();
            if dump {
                std::fs::write(scratch.join("undo-expected.json"), pretty(&docs[k].0 .1)).unwrap();
                std::fs::write(scratch.join("undo-got.json"), pretty(&now.1)).unwrap();
                std::fs::write(scratch.join("undo-after.json"), pretty(&docs[k + 1].0 .1)).unwrap();
            }
        }
    }
    if let Some(k) = diverged {
        check(&format!("undoing {} gives back the document before it (after {})", docs[k + 1].1, docs[k].1), false);
    }
    let back = render(&mut s);
    check(
        &format!("undoing every edit gives the first render back ({})", if same(&back, &first) { "bit-identical".into() } else { first_diff(&back, &first) }),
        same(&back, &first),
    );
    check("and the first document", doc(&s) == first_doc);
    if doc(&s) != first_doc && dump {
        std::fs::write(scratch.join("first-expected.json"), serde_json::to_string_pretty(&first_doc).unwrap()).unwrap();
        std::fs::write(scratch.join("first-got.json"), serde_json::to_string_pretty(&doc(&s)).unwrap()).unwrap();
    }
    for _ in 0..depth {
        s.redo();
    }
    let again = render(&mut s);
    check(
        &format!("redoing them all gives the last render ({})", if same(&again, &last) { "bit-identical".into() } else { first_diff(&again, &last) }),
        same(&again, &last),
    );
    check("and the last document", doc(&s) == last_doc);
    let dropped = s.status.cmds_dropped.load(std::sync::atomic::Ordering::Relaxed);
    check(&format!("no command for the engine was dropped ({dropped})"), dropped == 0);
    // (Linux only.)
    if let Some(peak) =
        std::fs::read_to_string("/proc/self/status").ok().and_then(|x| x.lines().find(|l| l.starts_with("VmHWM:")).map(|l| l[6..].trim().to_string()))
    {
        println!("  {:.0} s, at most {peak} in memory", started.elapsed().as_secs_f64());
    }
    log.flush().unwrap();
    drop(log);
    drop(s);

    // A second process makes the first edits again, which must draw the same, and make the same
    // documents and renders, line for line.
    if replay > 0 {
        let dir = scratch.join("replay");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .arg(&dir)
            .args([replay.to_string(), seed.to_string(), threads.to_string()])
            .args(grow.map(|n| n.to_string()))
            .env("SOAK_REPLAY", "0")
            .output()
            .expect("replay");
        std::fs::write(dir.join("output.log"), &out.stdout).unwrap();
        let mine = std::fs::read_to_string(scratch.join("edits.log")).unwrap();
        let theirs = std::fs::read_to_string(dir.join("edits.log")).unwrap_or_default();
        let (mine, theirs): (Vec<&str>, Vec<&str>) = (mine.lines().collect(), theirs.lines().collect());
        let differs = (0..theirs.len()).find(|&k| mine.get(k) != theirs.get(k));
        check(
            &format!(
                "a second process makes the first {replay} edits the same, line for line in the edit log ({})",
                match differs {
                    None => format!("{} lines", theirs.len()),
                    Some(k) => format!("line {} differs: {:?} then {:?}", k + 1, mine.get(k).unwrap_or(&""), theirs[k]),
                }
            ),
            theirs.len() > 2 && differs.is_none(),
        );
    }

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
