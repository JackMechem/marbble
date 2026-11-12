//! Render ahead plays exactly what playing live does: the same blocks at the same positions,
//! through cycle wraps, locates and fader moves; an edit is heard in the block it comes in; and the
//! render threads really render.

use super::*;
use crate::dsp::tests::rig;
use crate::fx::{Effect, EffectInfo, FxContext, Key};
use std::sync::atomic::AtomicU64;

const SR: f64 = 48000.0;
/// Frames per callback: not a divisor of the cycle, so wraps cut blocks.
const FRAMES: usize = 300;

/// Adds a ramp that counts the frames it has processed, so a block skipped, repeated or played
/// out of order shows in its output; reset, it starts again. Counts the blocks a render thread ran.
struct Counter(u64);
static ON_RENDER_THREAD: AtomicU64 = AtomicU64::new(0);

impl Effect for Counter {
    fn set_param(&mut self, _: u32, _: f32) {}
    fn param(&self, _: u32) -> f32 {
        0.0
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _: Key, _: &FxContext) {
        if std::thread::current().name().is_some_and(|n| n.starts_with("marbble-ahead")) {
            ON_RENDER_THREAD.fetch_add(1, Ordering::Relaxed);
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let ramp = (self.0 % 997) as f32 * 1e-4;
            *a = *a * 0.5 + ramp;
            *b = *b * 0.5 - ramp;
            self.0 += 1;
        }
    }
    fn reset(&mut self) {
        self.0 = 0;
    }
}

fn counter(uid: Id) -> InsertDsp {
    let info =
        EffectInfo { id: "test:counter", name: "Counter", category: "Utility", params: &[], meters: &[], sidechain: false, new: |_| Box::new(Counter(0)) };
    InsertDsp::new(uid, false, Plugin::Builtin(Box::new(BuiltinDsp::new(&info, SR as f32, &[]))), KeyRoute::default())
}

fn params(volume: f32) -> TrackParams {
    TrackParams {
        volume,
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

/// A 220 Hz sine over 10 s.
fn sine() -> Arc<Vec<RegionDsp>> {
    let len = SR as usize * 10;
    let x: Vec<f32> = (0..len).map(|i| 0.4 * (std::f32::consts::TAU * 220.0 * i as f32 / SR as f32).sin()).collect();
    let clip = Arc::new(Clip::from_channels("sine.wav".into(), SR as u32, vec![x.clone(), x]));
    Arc::new(vec![RegionDsp::Audio {
        start: 0,
        length: len as i64,
        offset: 0,
        gain: 1.0,
        clip,
        fade_in: (0, 0),
        fade_out: (len as i64, 0),
        laws: Default::default(),
    }])
}

/// What happens partway through a run, at the callback it's given for.
#[derive(Clone, Copy)]
enum Event {
    Locate(i64),
    Fader(Id, f32),
    Bypass(Id, Id, bool),
}

/// Plays `callbacks` callbacks of three tracks, each with a counter, with a cycle, sending each
/// event before its callback, render ahead on or off. The output's left channel.
fn run(ahead: bool, callbacks: usize, events: &[(usize, Event)]) -> Vec<f32> {
    let mut r = rig();
    let _threads = ahead.then(|| {
        let t = AheadThreads::start(2, r.status.clone());
        r.tx.push(Cmd::Ahead(Some(t.shared()), 0.1)).ok().unwrap();
        t
    });
    r.tx.push(Cmd::ParallelTest(crate::dsp::ParallelTest { patient: true, ..Default::default() })).ok().unwrap();
    for id in 1..=3 {
        let mut t = TrackDsp::new(id, TrackKind::Audio, params(0.5), Arc::default());
        t.set_regions(sine());
        t.set_insert(0, Some(counter(100 + id)));
        t.prepare_ahead();
        r.tx.push(Cmd::AddTrack(Box::new(t), usize::MAX)).ok().unwrap();
    }
    // A cycle of 0.3 s: wraps cut blocks in two.
    r.tx.push(Cmd::Cycle { enabled: true, start: 4000, end: 4000 + 14_437 }).ok().unwrap();
    r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    let mut out = Vec::new();
    let mut buf = vec![0.0f32; FRAMES * 2];
    for k in 0..callbacks {
        for (_, e) in events.iter().filter(|(at, _)| *at == k) {
            let cmd = match *e {
                Event::Locate(p) => Cmd::Locate(p),
                Event::Fader(id, v) => Cmd::Params(id, params(v)),
                Event::Bypass(track, uid, bypass) => Cmd::Bypass { track, uid, bypass },
            };
            r.tx.push(cmd).ok().unwrap();
        }
        r.engine.process(&[], 0, &mut buf, 2);
        out.extend(buf.chunks(2).map(|f| f[0]));
    }
    out
}

fn first_difference(a: &[f32], b: &[f32]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x.to_bits() != y.to_bits())
}

#[test]
fn ahead_plays_exactly_what_live_does_through_cycle_wraps() {
    let before = ON_RENDER_THREAD.load(Ordering::Relaxed);
    let live = run(false, 400, &[]);
    let ahead = run(true, 400, &[]);
    assert_eq!(first_difference(&live, &ahead), None);
    assert!(ON_RENDER_THREAD.load(Ordering::Relaxed) - before > 300, "the render threads played the counters");
    assert!(live.iter().any(|s| s.abs() > 0.1));
}

#[test]
fn a_locate_and_fader_moves_play_as_live() {
    let events = [(120, Event::Fader(2, 0.25)), (150, Event::Locate(30_000)), (151, Event::Fader(1, 0.9)), (260, Event::Locate(1_000))];
    let live = run(false, 350, &events);
    let ahead = run(true, 350, &events);
    assert_eq!(first_difference(&live, &ahead), None);
}

#[test]
fn an_edit_is_heard_in_the_block_it_comes_in() {
    // Bypassing the counter changes the output from that block on, as live: up to there the two
    // runs are the same, and the block the edit lands in already plays without it.
    let at = 200;
    let events = [(at, Event::Bypass(1, 101, true)), (at, Event::Bypass(2, 102, true)), (at, Event::Bypass(3, 103, true))];
    let live = run(false, 260, &events);
    let ahead = run(true, 260, &events);
    assert_eq!(first_difference(&live[..at * FRAMES], &ahead[..at * FRAMES]), None);
    // With every counter bypassed, both play the sines alone: the same from the edit on.
    assert_eq!(first_difference(&live[at * FRAMES..], &ahead[at * FRAMES..]), None);
}

#[test]
fn courses_cut_blocks_as_the_callback_does() {
    let mut c = Course::new(0, 1500, (true, 0, 2000));
    let mut seen = Vec::new();
    for _ in 0..6 {
        let m = c.next();
        seen.push((m.pos, m.n, c.advance(m.n)));
    }
    // 1500 frames: 1024 then 476. The next callback reaches the cycle's end after 500, wraps, and
    // plays the 1000 left; the one after reaches it after 1000, and plays the 500 left.
    assert_eq!(seen, [(0, 1024, false), (1024, 476, false), (1500, 500, true), (0, 1000, false), (1000, 1000, true), (0, 500, false)]);
}

#[test]
fn removing_a_lent_track_while_playing_is_safe() {
    let mut r = rig();
    let threads = AheadThreads::start(2, r.status.clone());
    r.tx.push(Cmd::Ahead(Some(threads.shared()), 0.1)).ok().unwrap();
    let mut t = TrackDsp::new(1, TrackKind::Audio, params(0.5), Arc::default());
    t.set_regions(sine());
    t.set_insert(0, Some(counter(101)));
    t.prepare_ahead();
    r.tx.push(Cmd::AddTrack(Box::new(t), usize::MAX)).ok().unwrap();
    r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    let mut buf = vec![0.0f32; FRAMES * 2];
    for _ in 0..50 {
        r.engine.process(&[], 0, &mut buf, 2);
    }
    assert!(r.engine.tracks[0].ahead.as_ref().is_some_and(|a| a.lent()), "it went ahead");
    r.tx.push(Cmd::RemoveTrack(1)).ok().unwrap();
    for _ in 0..20 {
        r.engine.process(&[], 0, &mut buf, 2);
    }
    assert!(r.engine.tracks.is_empty());
    while r.garbage.pop().is_ok() {}
    drop(threads);
}
