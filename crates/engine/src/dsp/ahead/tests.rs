//! Render ahead plays exactly what playing live does: the same blocks at the same positions,
//! through cycle wraps, locates and fader moves; an edit is heard in the block it comes in; and the
//! render threads really render.

use super::*;
use crate::dsp::tests::rig;
use crate::fx::{Effect, EffectInfo, FxContext, Key};
use std::sync::atomic::{AtomicBool, AtomicU64};

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

#[test]
fn a_track_kept_live_comes_back_at_once_and_stays_until_let_go() {
    let mut r = rig();
    let threads = AheadThreads::start(2, r.status.clone());
    r.tx.push(Cmd::Ahead(Some(threads.shared()), 0.1)).ok().unwrap();
    for id in 1..=2 {
        let mut t = TrackDsp::new(id, TrackKind::Audio, params(0.5), Arc::default());
        t.set_regions(sine());
        t.set_insert(0, Some(counter(100 + id)));
        t.prepare_ahead();
        r.tx.push(Cmd::AddTrack(Box::new(t), usize::MAX)).ok().unwrap();
    }
    r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    let mut buf = vec![0.0f32; FRAMES * 2];
    let mut play = |r: &mut crate::dsp::tests::Rig, k: usize| {
        for _ in 0..k {
            r.engine.process(&[], 0, &mut buf, 2);
        }
    };
    let lent = |r: &crate::dsp::tests::Rig, i: usize| r.engine.tracks[i].ahead.as_ref().is_some_and(|a| a.lent());
    play(&mut r, 10);
    assert!(lent(&r, 0) && lent(&r, 1));
    let mut keep = [None; KEEP_LIVE];
    keep[0] = Some(1);
    r.tx.push(Cmd::KeepLive(keep)).ok().unwrap();
    // At once, or a callback later if a render thread was in it then (#471).
    play(&mut r, 1);
    for _ in 0..4 {
        if lent(&r, 0) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            play(&mut r, 1);
        }
    }
    assert!(!lent(&r, 0) && lent(&r, 1), "the selected track came back, the other stayed ahead");
    play(&mut r, 20);
    assert!(!lent(&r, 0), "and it stays live");
    r.tx.push(Cmd::KeepLive([None; KEEP_LIVE])).ok().unwrap();
    play(&mut r, 2);
    assert!(lent(&r, 0), "let go, it goes ahead again");
    r.tx.push(Cmd::Stop).ok().unwrap();
    play(&mut r, 2);
    drop(threads);
}

/// Holds the render threads in its block while its flag is set, as a plug-in that's slow or hung
/// would; on the callback's thread it passes the audio on. Each test holds with a flag of its own.
struct Holder(&'static AtomicBool);
static HOLD: AtomicBool = AtomicBool::new(false);

impl Effect for Holder {
    fn set_param(&mut self, _: u32, _: f32) {}
    fn param(&self, _: u32) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: &mut [f32], _: Key, _: &FxContext) {
        if std::thread::current().name().is_some_and(|n| n.starts_with("marbble-ahead")) {
            while self.0.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
    fn reset(&mut self) {}
}

#[test]
fn a_render_thread_held_in_a_strip_never_holds_up_the_callback() {
    // On a miss the callback used to wait half a block per strip, then for the render thread to
    // come out of the strip (up to half a second), then play the block again live (#466). Now the
    // strip plays silence while its render thread is in it, and comes back once it's out.
    let info =
        EffectInfo { id: "test:holder", name: "Holder", category: "Utility", params: &[], meters: &[], sidechain: false, new: |_| Box::new(Holder(&HOLD)) };
    let mut r = rig();
    let threads = AheadThreads::start(1, r.status.clone());
    r.tx.push(Cmd::Ahead(Some(threads.shared()), 0.05)).ok().unwrap();
    let mut t = TrackDsp::new(1, TrackKind::Audio, params(0.5), Arc::default());
    t.set_regions(sine());
    t.set_insert(0, Some(InsertDsp::new(101, false, Plugin::Builtin(Box::new(BuiltinDsp::new(&info, SR as f32, &[]))), KeyRoute::default())));
    t.prepare_ahead();
    r.tx.push(Cmd::AddTrack(Box::new(t), usize::MAX)).ok().unwrap();
    r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    // A buffer of 1024 frames: 21 ms to make each in.
    let mut buf = vec![0.0f32; MAX_BLOCK * 2];
    let lent = |r: &crate::dsp::tests::Rig| r.engine.tracks.first().and_then(|t| t.ahead.as_ref()).as_ref().is_some_and(|a| a.lent());
    let lost = |r: &crate::dsp::tests::Rig| r.engine.tracks.first().and_then(|t| t.ahead.as_ref()).as_ref().is_some_and(|a| a.lost());
    let mut k = 0;
    while !lent(&r) && k < 200 {
        r.engine.process(&[], 0, &mut buf, 2);
        k += 1;
    }
    assert!(lent(&r), "it went ahead");

    HOLD.store(true, Ordering::SeqCst);
    let mut slowest = std::time::Duration::ZERO;
    let mut silent = 0;
    for _ in 0..12 {
        let at = std::time::Instant::now();
        r.engine.process(&[], 0, &mut buf, 2);
        slowest = slowest.max(at.elapsed());
        silent += buf.iter().all(|s| *s == 0.0) as usize;
    }
    // Half a buffer's wait at most, with room for a loaded machine; not the half second it was.
    assert!(slowest < std::time::Duration::from_millis(100), "a callback took {slowest:?}");
    assert!(silent > 0, "the strip played silence while its render thread was held");
    assert!(r.status.ahead_misses.load(Ordering::Relaxed) > 0);
    assert!(lent(&r) && !lost(&r), "it waits for its render thread without being given up");

    HOLD.store(false, Ordering::SeqCst);
    // Callbacks a buffer apart, as a device makes them: the render thread comes out meanwhile.
    for _ in 0..4 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        r.engine.process(&[], 0, &mut buf, 2);
    }
    assert!(!lost(&r));
    assert!(buf.iter().any(|s| s.abs() > 0.01), "it plays again");
    r.tx.push(Cmd::Stop).ok().unwrap();
    r.engine.process(&[], 0, &mut buf, 2);
    drop(threads);
}

/// Takes most of a buffer on the callback's side, as a heavy plug-in played live does (spinning:
/// a sleep overshoots by milliseconds on some systems); quick on a render thread.
struct Heavy;

impl Effect for Heavy {
    fn set_param(&mut self, _: u32, _: f32) {}
    fn param(&self, _: u32) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: &mut [f32], _: Key, _: &FxContext) {
        if !std::thread::current().name().is_some_and(|n| n.starts_with("marbble-ahead")) {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(14);
            while std::time::Instant::now() < until {
                std::hint::spin_loop();
            }
        }
    }
    fn reset(&mut self) {}
}

#[test]
fn a_strip_that_takes_most_of_the_buffer_live_still_goes_ahead() {
    // Started mid-song, twelve amp models took 75 % of each buffer live, and strips were lent only
    // from a callback that took at most 60 %: they never went ahead (#466).
    let info = EffectInfo { id: "test:heavy", name: "Heavy", category: "Utility", params: &[], meters: &[], sidechain: false, new: |_| Box::new(Heavy) };
    let mut r = rig();
    let threads = AheadThreads::start(1, r.status.clone());
    r.tx.push(Cmd::Ahead(Some(threads.shared()), 0.2)).ok().unwrap();
    let mut t = TrackDsp::new(1, TrackKind::Audio, params(0.5), Arc::default());
    t.set_regions(sine());
    t.set_insert(0, Some(InsertDsp::new(101, false, Plugin::Builtin(Box::new(BuiltinDsp::new(&info, SR as f32, &[]))), KeyRoute::default())));
    t.prepare_ahead();
    r.tx.push(Cmd::AddTrack(Box::new(t), usize::MAX)).ok().unwrap();
    r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    // Four blocks of 1024 frames a callback: 85 ms a buffer, 56 of them in the plug-in while it
    // plays live, two thirds, with room either side of 60 % and 90 % for a busy machine.
    let mut buf = vec![0.0f32; 4 * MAX_BLOCK * 2];
    let lent = |r: &crate::dsp::tests::Rig| r.engine.tracks.first().and_then(|t| t.ahead.as_ref()).is_some_and(|a| a.lent());
    let mut k = 0;
    while !lent(&r) && k < 40 {
        r.engine.process(&[], 0, &mut buf, 2);
        k += 1;
    }
    assert!(lent(&r), "a strip taking two thirds of the buffer live went ahead");
    r.tx.push(Cmd::Stop).ok().unwrap();
    r.engine.process(&[], 0, &mut buf, 2);
    drop(threads);
}

/// Takes `SLOW_FOR` a block on a render thread while its flag is set, as a heavy plug-in on a
/// loaded machine does; nothing on the callback's thread. Each test slows with a flag of its own.
struct Slow(&'static AtomicBool);
static SLOW: AtomicBool = AtomicBool::new(false);
const SLOW_FOR: std::time::Duration = std::time::Duration::from_millis(40);

impl Effect for Slow {
    fn set_param(&mut self, _: u32, _: f32) {}
    fn param(&self, _: u32) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: &mut [f32], _: Key, _: &FxContext) {
        if self.0.load(Ordering::SeqCst) && std::thread::current().name().is_some_and(|n| n.starts_with("marbble-ahead")) {
            std::thread::sleep(SLOW_FOR);
        }
    }
    fn reset(&mut self) {}
}

/// A rig playing `tracks` audio tracks of a sine through `info`'s effect, rendered ahead on as many
/// render threads, `lead` seconds ahead, 1024 frames a callback, until every strip is lent.
fn lent_rig(info: &'static EffectInfo, tracks: Id, lead: f64) -> (crate::dsp::tests::Rig, AheadThreads, Vec<f32>) {
    let mut r = rig();
    let threads = AheadThreads::start(tracks as usize, r.status.clone());
    r.tx.push(Cmd::Ahead(Some(threads.shared()), lead)).ok().unwrap();
    for id in 1..=tracks {
        let mut t = TrackDsp::new(id, TrackKind::Audio, params(0.5), Arc::default());
        t.set_regions(sine());
        t.set_insert(0, Some(InsertDsp::new(100 + id, false, Plugin::Builtin(Box::new(BuiltinDsp::new(info, SR as f32, &[]))), KeyRoute::default())));
        t.prepare_ahead();
        r.tx.push(Cmd::AddTrack(Box::new(t), usize::MAX)).ok().unwrap();
    }
    r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    let mut buf = vec![0.0f32; MAX_BLOCK * 2];
    let mut k = 0;
    while !all_lent(&r) && k < 200 {
        r.engine.process(&[], 0, &mut buf, 2);
        k += 1;
    }
    assert!(all_lent(&r), "they went ahead");
    (r, threads, buf)
}

/// Lets go of a flag as a test ends, passed or not, before its render threads are joined.
struct Release(&'static AtomicBool);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn all_lent(r: &crate::dsp::tests::Rig) -> bool {
    !r.engine.tracks.is_empty() && r.engine.tracks.iter().all(|t| t.ahead.as_ref().is_some_and(|a| a.lent()))
}

/// The longest of `k` callbacks, a buffer apart as a device makes them, and whether the playhead
/// was at `at` (or up to `k` buffers past it) by the end.
fn callbacks(r: &mut crate::dsp::tests::Rig, buf: &mut [f32], k: usize, at: i64) -> (std::time::Duration, bool) {
    let mut slowest = std::time::Duration::ZERO;
    for _ in 0..k {
        let t = std::time::Instant::now();
        r.engine.process(&[], 0, buf, 2);
        slowest = slowest.max(t.elapsed());
        std::thread::sleep(std::time::Duration::from_millis(21));
    }
    (slowest, (at..at + (k * MAX_BLOCK) as i64).contains(&r.engine.pos))
}

/// Waits (up to two seconds) until a render thread is in every lent strip's block: a fixed sleep
/// isn't enough on a slow runner, where the render thread may not have started it yet.
fn until_rendering(r: &crate::dsp::tests::Rig) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let busy = |r: &crate::dsp::tests::Rig| r.engine.tracks.iter().filter_map(|t| t.ahead.as_ref()).all(|a| a.busy.load(Ordering::SeqCst));
    while !busy(r) && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(busy(r), "the render threads went into the strips' blocks");
}

#[test]
fn a_locate_never_waits_for_the_render_threads() {
    // A command that changed lent strips waited on the audio thread for each one's render thread
    // to come out, one after another: a locate with four strips mid-render held the callback for
    // a render or more (#471). Now it takes them all out and applies once they're home.
    static INFO: EffectInfo =
        EffectInfo { id: "test:slow", name: "Slow", category: "Utility", params: &[], meters: &[], sidechain: false, new: |_| Box::new(Slow(&SLOW)) };
    let (mut r, threads, mut buf) = lent_rig(&INFO, 4, 0.2);
    // Rings full, then every render thread in a slow block.
    std::thread::sleep(std::time::Duration::from_millis(30));
    let _release = Release(&SLOW);
    SLOW.store(true, Ordering::SeqCst);
    r.engine.process(&[], 0, &mut buf, 2);
    until_rendering(&r);

    let to = 200_000;
    r.tx.push(Cmd::Locate(to)).ok().unwrap();
    let (slowest, located) = callbacks(&mut r, &mut buf, 6, to);
    SLOW.store(false, Ordering::SeqCst);
    // Well under one slow render, with room for a loaded machine.
    assert!(slowest < SLOW_FOR / 2, "a callback took {slowest:?}");
    assert!(located, "the locate applied once the render threads were out: at {}", r.engine.pos);
    assert!(r.engine.tracks.iter().all(|t| t.ahead.as_ref().is_some_and(|a| !a.lost())));
    r.tx.push(Cmd::Stop).ok().unwrap();
    r.engine.process(&[], 0, &mut buf, 2);
    drop(threads);
}

static HUNG: AtomicBool = AtomicBool::new(false);

#[test]
fn a_command_for_a_strip_whose_render_thread_hangs_gives_it_up_without_holding_the_callback() {
    // It used to wait half a second on the audio thread, then lose the strip.
    static INFO: EffectInfo =
        EffectInfo { id: "test:hung", name: "Hung", category: "Utility", params: &[], meters: &[], sidechain: false, new: |_| Box::new(Holder(&HUNG)) };
    let (mut r, threads, mut buf) = lent_rig(&INFO, 1, 0.05);
    let _release = Release(&HUNG);
    HUNG.store(true, Ordering::SeqCst);
    r.engine.process(&[], 0, &mut buf, 2);
    until_rendering(&r);

    let to = 100_000;
    r.tx.push(Cmd::Locate(to)).ok().unwrap();
    let started = std::time::Instant::now();
    // Callbacks a buffer apart for half the give-up time (by the clock: sleeps overshoot on some
    // runners, and ten of them could pass it).
    let mut slowest = std::time::Duration::ZERO;
    while started.elapsed() < RECLAIM_WAIT / 2 {
        let (s, _) = callbacks(&mut r, &mut buf, 1, to);
        slowest = slowest.max(s);
        // Played on from near the start, the playhead is far short of the locate's position.
        assert!(r.engine.pos < to || started.elapsed() >= RECLAIM_WAIT, "the locate waits for the strip meanwhile");
    }
    let (s, located) = callbacks(&mut r, &mut buf, 30, to);
    let slowest = slowest.max(s);
    assert!(started.elapsed() > RECLAIM_WAIT);
    assert!(slowest < std::time::Duration::from_millis(50), "a callback took {slowest:?}");
    assert!(located, "given up on, the strip let the locate through: at {}", r.engine.pos);
    assert!(r.engine.tracks[0].ahead.as_ref().is_some_and(|a| a.lost()), "the strip is lost");
    HUNG.store(false, Ordering::SeqCst);
    r.tx.push(Cmd::Stop).ok().unwrap();
    r.engine.process(&[], 0, &mut buf, 2);
    drop(threads);
}

static HUNG_TEMPO: AtomicBool = AtomicBool::new(false);

#[test]
fn a_strip_lost_across_a_tempo_change_keeps_the_map_its_render_thread_reads() {
    // A new tempo map replaced the old one, which went out with the garbage, while a strip lost to
    // a hung plug-in had its render thread still in it: once out, that thread read the freed map
    // for the rest of its block (#485).
    static INFO: EffectInfo = EffectInfo {
        id: "test:hung-tempo",
        name: "Hung",
        category: "Utility",
        params: &[],
        meters: &[],
        sidechain: false,
        new: |_| Box::new(Holder(&HUNG_TEMPO)),
    };
    let (mut r, threads, mut buf) = lent_rig(&INFO, 1, 0.05);
    let _release = Release(&HUNG_TEMPO);
    HUNG_TEMPO.store(true, Ordering::SeqCst);
    r.engine.process(&[], 0, &mut buf, 2);
    until_rendering(&r);

    let old = Arc::downgrade(&r.engine.tempo);
    r.tx.push(Cmd::Tempo(Arc::new(TempoMap::constant(SR, 90.0, 4, 4)))).ok().unwrap();
    let lost = |r: &crate::dsp::tests::Rig| r.engine.tracks[0].ahead.as_ref().is_some_and(|a| a.lost());
    let started = std::time::Instant::now();
    while !(lost(&r) && r.engine.tempo.bpm_at(0.0) == 90.0) && started.elapsed() < 4 * RECLAIM_WAIT {
        callbacks(&mut r, &mut buf, 1, 0);
    }
    assert!(lost(&r), "the strip is lost");
    assert_eq!(r.engine.tempo.bpm_at(0.0), 90.0, "the new tempo applied");
    // What the engine let go of is dropped, as the session's thread does.
    while r.garbage.pop().is_ok() {}
    assert!(old.upgrade().is_some(), "the map the render thread was lent with outlives the change");

    // Out of its plug-in, the render thread finishes its block with that map.
    HUNG_TEMPO.store(false, Ordering::SeqCst);
    let busy = |r: &crate::dsp::tests::Rig| r.engine.tracks[0].ahead.as_ref().is_some_and(|a| a.busy.load(Ordering::SeqCst));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while busy(&r) && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!busy(&r), "the render thread came out");
    r.tx.push(Cmd::Stop).ok().unwrap();
    r.engine.process(&[], 0, &mut buf, 2);
    drop(threads);
}

static SLOW_RENDER: AtomicBool = AtomicBool::new(false);

#[test]
fn a_command_waiting_for_its_strips_when_a_render_starts_is_applied_before_it() {
    // A locate waiting for its strips' render threads was applied partway through the render
    // that followed, and undone as it ended (#471 review): the bounce played from elsewhere, and
    // the playhead, Stop and the metronome were lost for live play.
    static INFO: EffectInfo = EffectInfo {
        id: "test:slow-render",
        name: "Slow",
        category: "Utility",
        params: &[],
        meters: &[],
        sidechain: false,
        new: |_| Box::new(Slow(&SLOW_RENDER)),
    };
    let (mut r, threads, mut buf) = lent_rig(&INFO, 4, 0.2);
    std::thread::sleep(std::time::Duration::from_millis(30));
    let _release = Release(&SLOW_RENDER);
    SLOW_RENDER.store(true, Ordering::SeqCst);
    r.engine.process(&[], 0, &mut buf, 2);
    until_rendering(&r);

    let to = 200_000;
    for cmd in [Cmd::Locate(to), Cmd::Metronome { on: true, count_in: false }, Cmd::Stop] {
        r.tx.push(cmd).ok().unwrap();
    }
    let first = r.engine.render(0, 4800, 0, None, &mut |_| {});
    SLOW_RENDER.store(false, Ordering::SeqCst);
    assert_eq!((r.engine.pos, r.engine.playing, r.engine.metronome), (to, false, true), "as commanded before the render");
    let again = r.engine.render(0, 4800, 0, None, &mut |_| {});
    assert!(first[0].iter().any(|s| s.abs() > 0.01));
    assert_eq!(first_difference(&first[0], &again[0]), None, "the render played what a clean one does");
    drop(threads);
}
