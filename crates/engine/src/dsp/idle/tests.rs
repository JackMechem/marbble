//! Plug-ins fed silence: when they fall asleep, what wakes them, and what keeps them awake.

use super::*;
use crate::dsp::tests::{rig, Rig};
use crate::fx::{Effect, EffectInfo, FxContext, Key};
use std::sync::atomic::AtomicUsize;

const SR: f64 = 48000.0;
const BLOCK: usize = 480;
/// A second in frames, a whole number of blocks.
const SEC: usize = 48000;

/// Passes its input through and counts its blocks, in statics, as `EffectInfo::new` is a plain
/// function. Each test has its own `Probe` number, as tests may run in parallel. `TAIL` says how
/// long it rings on, and `HISS` makes it add a little noise of its own.
struct Probe(usize);
static BLOCKS: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
static KEY: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static TAIL: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static HISS: [AtomicBool; 8] = [const { AtomicBool::new(false) }; 8];

impl Effect for Probe {
    fn set_param(&mut self, _: u32, _: f32) {}
    fn param(&self, _: u32) -> f32 {
        0.0
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, _: &FxContext) {
        BLOCKS[self.0].fetch_add(1, Ordering::Relaxed);
        if let Some((k, _)) = key {
            KEY[self.0].store(peak(k).to_bits(), Ordering::Relaxed);
        }
        if HISS[self.0].load(Ordering::Relaxed) {
            for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let noise = if i % 2 == 0 { 1e-4 } else { -1e-4 };
                *a += noise;
                *b += noise;
            }
        }
    }
    fn reset(&mut self) {}
    fn tail_secs(&self) -> f32 {
        f32::from_bits(TAIL[self.0].load(Ordering::Relaxed))
    }
}

fn probe_info(k: usize) -> EffectInfo {
    let new: fn(f32) -> Box<dyn Effect> = match k {
        0 => |_| Box::new(Probe(0)),
        1 => |_| Box::new(Probe(1)),
        2 => |_| Box::new(Probe(2)),
        3 => |_| Box::new(Probe(3)),
        4 => |_| Box::new(Probe(4)),
        5 => |_| Box::new(Probe(5)),
        6 => |_| Box::new(Probe(6)),
        _ => |_| Box::new(Probe(7)),
    };
    EffectInfo { id: "test:probe", name: "Probe", category: "Utility", params: &[], meters: &[], sidechain: true, new }
}

/// Probe `k`, ringing on for `tail` seconds.
fn probe(k: usize, tail: f32, key: Option<Id>) -> InsertDsp {
    TAIL[k].store(tail.to_bits(), Ordering::Relaxed);
    InsertDsp::new(900 + k as Id, false, Plugin::Builtin(Box::new(BuiltinDsp::new(&probe_info(k), SR as f32, &[]))), KeyRoute::new(key, false))
}

fn blocks(k: usize) -> usize {
    BLOCKS[k].load(Ordering::Relaxed)
}

fn params() -> TrackParams {
    TrackParams {
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        armed: false,
        monitor: false,
        input: InputSel::None,
        output: None,
        sends: [None; SEND_SLOTS],
        stack_gain: 1.0,
        stack_mute: false,
        folder: None,
        own_gain: 1.0,
        own_mute: false,
        bus_mute: false,
        vca_gain: 1.0,
        vca_mute: false,
        off: false,
        instrument_output: None,
        midi: Default::default(),
    }
}

impl Rig {
    fn order(&mut self, c: Cmd) {
        self.tx.push(c).ok().unwrap();
    }

    /// An audio track playing a 440 Hz sine over each (start, length) in frames.
    fn track_with(&mut self, id: Id, tones: &[(usize, usize)], ins: InsertDsp) {
        let mut t = TrackDsp::new(id, TrackKind::Audio, params(), Arc::new(Meter::default()));
        t.set_regions(Arc::new(tones.iter().map(|&(s, n)| sine(s, n)).collect()));
        self.order(Cmd::AddTrack(Box::new(t), usize::MAX));
        self.order(Cmd::Insert { track: id, slot: 0, insert: Some(ins) });
    }

    /// Plays `frames` in blocks, returning the left channel.
    fn play(&mut self, frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let mut buf = vec![0.0f32; BLOCK * 2];
        for _ in 0..frames.div_ceil(BLOCK) {
            self.engine.process(&[], 0, &mut buf, 2);
            out.extend(buf.chunks(2).map(|f| f[0]));
        }
        out
    }

    /// The blocks probe `k` ran over the next `frames`.
    fn ran(&mut self, k: usize, frames: usize) -> usize {
        let before = blocks(k);
        self.play(frames);
        blocks(k) - before
    }
}

fn sine(start: usize, len: usize) -> RegionDsp {
    let x: Vec<f32> = (0..len).map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / SR as f32).sin()).collect();
    let clip = Arc::new(Clip::from_channels("sine.wav".into(), SR as u32, vec![x.clone(), x]));
    RegionDsp::Audio {
        start: start as i64,
        length: len as i64,
        offset: 0,
        gain: 1.0,
        clip,
        fade_in: (start as i64, 0),
        fade_out: ((start + len) as i64, 0),
        laws: Default::default(),
    }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

#[test]
fn a_plugin_fed_silence_sleeps_after_the_hold_and_wakes_with_signal() {
    let mut r = rig();
    r.track_with(1, &[(0, SEC / 2), (4 * SEC, SEC)], probe(0, 0.0, None));
    r.order(Cmd::Play { from: 0 });
    assert_eq!(r.ran(0, SEC / 2), SEC / 2 / BLOCK, "it runs while there's signal");
    assert_eq!(r.ran(0, 2 * SEC), 2 * SEC / BLOCK, "and through the hold");
    assert_eq!(r.ran(0, 3 * SEC / 2), 0, "then sleeps");
    assert_eq!(r.engine.tracks[0].load.take().asleep, 1, "the Diagnostics window hears it's asleep");
    let out = r.play(BLOCK);
    assert_eq!(blocks(0), (SEC / 2 + 2 * SEC) / BLOCK + 1, "the block where signal starts runs it");
    assert!(peak(&out) > 0.0, "and plays");
    assert!(peak(&r.play(BLOCK)) > 0.45);
    assert_eq!(r.engine.tracks[0].load.take().asleep, 0);
}

#[test]
fn it_stays_awake_as_long_as_it_may_ring_on() {
    let mut r = rig();
    r.track_with(1, &[(0, SEC / 2)], probe(1, 3.0, None));
    r.order(Cmd::Play { from: 0 });
    assert_eq!(r.ran(1, SEC / 2 + 3 * SEC), (SEC / 2 + 3 * SEC) / BLOCK, "its 3 s tail is longer than the hold");
    assert_eq!(r.ran(1, SEC), 0);
}

#[test]
fn a_plugin_with_a_tail_that_never_ends_never_sleeps() {
    let mut r = rig();
    r.track_with(1, &[], probe(2, f32::INFINITY, None));
    assert_eq!(r.ran(2, 5 * SEC), 5 * SEC / BLOCK);
}

#[test]
fn a_plugin_making_sound_of_its_own_never_sleeps() {
    let mut r = rig();
    HISS[3].store(true, Ordering::Relaxed);
    r.track_with(1, &[], probe(3, 0.0, None));
    assert_eq!(r.ran(3, 5 * SEC), 5 * SEC / BLOCK);
    assert!(peak(&r.play(BLOCK)) > 0.0, "and is heard");
}

#[test]
fn a_stopped_project_sleeps_too() {
    let mut r = rig();
    r.track_with(1, &[(0, 10 * SEC)], probe(4, 0.0, None));
    assert_eq!(r.ran(4, 2 * SEC), 2 * SEC / BLOCK);
    assert_eq!(r.ran(4, SEC), 0);
    r.order(Cmd::Play { from: 0 });
    assert_eq!(r.ran(4, SEC), SEC / BLOCK, "playing wakes it");
}

#[test]
fn a_side_chain_key_wakes_it() {
    let mut r = rig();
    // The key's source plays first.
    r.track_with(2, &[(3 * SEC, SEC)], probe(6, 0.0, None));
    r.track_with(1, &[], probe(5, 0.0, Some(2)));
    r.order(Cmd::Play { from: 0 });
    r.play(3 * SEC);
    assert_eq!(f32::from_bits(KEY[5].load(Ordering::Relaxed)), 0.0);
    assert_eq!(r.ran(5, SEC / 2), SEC / 2 / BLOCK, "the key wakes it though its input is silent");
    assert!(f32::from_bits(KEY[5].load(Ordering::Relaxed)) > 0.4, "and it hears the key");
}

#[test]
fn a_render_starts_with_every_plugin_awake() {
    let mut r = rig();
    r.track_with(1, &[], probe(7, 0.0, None));
    r.play(3 * SEC);
    let before = blocks(7);
    r.engine.render(0, SEC as i64, 0, None, &mut |_| {});
    assert!(blocks(7) - before >= SEC / MAX_BLOCK, "it runs from the render's start, as in any other render");
}
