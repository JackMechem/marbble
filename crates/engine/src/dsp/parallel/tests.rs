//! Parallel strips against the serial loop: bit-identical output, routing, sends, PDC, plug-ins that stop responding.

use super::*;
use crate::dsp::tests::{rig, Rig};
use crate::fx::{Effect, EffectInfo, FxContext, Key};

const SR: f64 = 48000.0;
const BLOCK: usize = 256;

fn params(output: Option<Id>, sends: &[(Id, f32, bool)]) -> TrackParams {
    let mut s = [None; SEND_SLOTS];
    for (k, &(bus, gain, pre_fader)) in sends.iter().enumerate() {
        s[k] = Some(SendParams { bus, gain, pre_fader });
    }
    TrackParams {
        volume: 0.7,
        pan: 0.25,
        mute: false,
        solo: false,
        armed: false,
        monitor: false,
        input: InputSel::None,
        output,
        sends: s,
        stack_gain: 1.0,
        stack_mute: false,
        folder: None,
        own_gain: 0.7,
        own_mute: false,
        bus_mute: false,
        vca_gain: 1.0,
        vca_mute: false,
        off: false,
        instrument_output: None,
        midi: Default::default(),
    }
}

/// An effect that makes its own signal (a sine at a pitch of its own) on top of what comes in,
/// adds the key into it, and takes a little time, so that strips overlap.
struct Tone {
    phase: f32,
    hz: f32,
    /// Where `reset` puts the phase back.
    start: f32,
}

impl Effect for Tone {
    fn set_param(&mut self, _: u32, _: f32) {}
    fn param(&self, _: u32) -> f32 {
        0.0
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, _: &FxContext) {
        for i in 0..l.len() {
            self.phase = (self.phase + self.hz / SR as f32) % 1.0;
            let s = (self.phase * std::f32::consts::TAU).sin() * 0.1;
            let k = key.map_or(0.0, |(a, b)| 0.25 * (a[i] + b[i]));
            l[i] += s + k;
            r[i] += s * 0.5 - k;
        }
    }
    fn reset(&mut self) {
        self.phase = self.start;
    }
}

fn tone(uid: Id, hz: f32, key: Option<Id>, pre: bool) -> InsertDsp {
    // `EffectInfo::new` is a plain function, so the pitch comes through the uid's slot below.
    let info = EffectInfo {
        id: "test:tone",
        name: "Tone",
        category: "Utility",
        params: &[],
        meters: &[],
        sidechain: true,
        new: |_| Box::new(Tone { phase: 0.0, hz: 0.0, start: 0.0 }),
    };
    let mut b = BuiltinDsp::new(&info, SR as f32, &[]);
    b.replace_effect_for_tests(Box::new(Tone { phase: uid as f32 * 0.01, hz, start: uid as f32 * 0.01 }));
    InsertDsp { uid, bypass: false, plugin: Plugin::Builtin(Box::new(b)), key: KeyRoute::new(key, pre) }
}

fn strip(id: Id, kind: TrackKind, p: TrackParams, fx: Vec<InsertDsp>) -> Box<TrackDsp> {
    let mut t = Box::new(TrackDsp::new(id, kind, p, Arc::new(Meter::default())));
    for (k, f) in fx.into_iter().enumerate() {
        t.set_insert(k, Some(f));
    }
    t
}

/// Sends, an output into an aux, a summing stack, keys taken before and after the fader, a
/// send with a gain changing under it, and several feeders per bus: order matters everywhere.
fn project(e: &mut Rig) -> TaskGraph {
    let keyed = strip(4, TrackKind::Audio, params(None, &[(6, 0.3, false)]), vec![tone(40, 310.0, Some(1), false), tone(41, 390.0, Some(2), true)]);
    let tracks = vec![
        strip(1, TrackKind::Audio, params(Some(5), &[(6, 0.5, true), (7, 0.25, false), (6, 0.125, false)]), vec![tone(10, 110.0, None, false)]),
        strip(2, TrackKind::Audio, params(Some(5), &[(6, 0.4, false)]), vec![tone(20, 170.0, None, false)]),
        strip(3, TrackKind::Audio, params(None, &[(7, 0.6, true)]), vec![tone(30, 230.0, None, false)]),
        keyed,
        strip(5, TrackKind::Summing, params(None, &[(7, 0.2, false)]), vec![tone(50, 470.0, None, false)]),
        strip(6, TrackKind::Aux, params(None, &[]), vec![tone(60, 530.0, None, false)]),
        strip(7, TrackKind::Aux, params(None, &[]), vec![tone(70, 610.0, None, false)]),
    ];
    let ids = tracks.iter().map(|t| t.id).collect();
    e.engine.replace_graph(SR, tracks, (0..INSERT_SLOTS).map(|_| None).collect());
    let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)];
    TaskGraph::new(ids, &edges, |_| true).unwrap()
}

fn play(e: &mut Rig, blocks: usize) -> Vec<f32> {
    e.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    let mut all = Vec::new();
    let mut out = vec![0.0f32; BLOCK * 2];
    for _ in 0..blocks {
        e.engine.process(&[], 0, &mut out, 2);
        all.extend_from_slice(&out);
    }
    all
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|s| s.to_bits()).collect()
}

fn pooled(threads: usize, blocks: usize) -> (Vec<f32>, u64) {
    let mut e = rig();
    let g = project(&mut e);
    e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads, ..Default::default() })))).ok().unwrap();
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
    let out = play(&mut e, blocks);
    (out, e.status.parallel_blocks.load(Ordering::Relaxed))
}

#[test]
fn the_pool_plays_what_the_serial_loop_plays_bit_for_bit() {
    let mut e = rig();
    project(&mut e);
    let serial = play(&mut e, 200);
    assert!(serial.iter().any(|s| s.abs() > 0.01));
    assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 0);
    for threads in [1, 2, 3, 4, 8] {
        let (out, blocks) = pooled(threads, 200);
        assert_eq!(blocks, 200, "{threads} threads: every block went through the pool");
        assert!(bits(&out) == bits(&serial), "{threads} threads differ from the serial loop");
    }
}

#[test]
fn a_render_after_a_new_send_does_not_depend_on_how_long_it_played_live_first() {
    let renders: Vec<Vec<Vec<f32>>> = [1, 40]
        .into_iter()
        .map(|blocks| {
            let mut e = rig();
            project(&mut e);
            // Track 3 starts sending to aux 6, after the fader and before it.
            e.tx.push(Cmd::Params(3, params(None, &[(7, 0.6, true), (6, 0.5, false), (6, 0.3, true)]))).ok().unwrap();
            let mut out = vec![0.0f32; BLOCK * 2];
            for _ in 0..blocks {
                e.engine.process(&[], 0, &mut out, 2);
            }
            e.engine.render(0, 4 * BLOCK as i64, 0, None, &mut |_| {})
        })
        .collect();
    assert!(renders[0][0].iter().any(|s| s.abs() > 0.01));
    assert!(renders[0].iter().zip(&renders[1]).all(|(a, b)| bits(a) == bits(b)), "the sends start at their levels");
}

#[test]
fn without_a_graph_that_fits_it_plays_serially() {
    let mut e = rig();
    project(&mut e);
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
    play(&mut e, 2);
    assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 0, "no graph");
    // One missing route of each kind.
    for missing in [(1, 5), (1, 7), (2, 6), (1, 4), (2, 4)] {
        let mut r = rig();
        let full = project(&mut r);
        let edges: Vec<(Id, Id)> =
            [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)].into_iter().filter(|x| *x != missing).collect();
        let ids = (0..full.len()).map(|i| full.id(i)).collect();
        r.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(ids, &edges, |_| true).unwrap())))).ok().unwrap();
        r.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
        r.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        play(&mut r, 2);
        assert_eq!(r.status.parallel_blocks.load(Ordering::Relaxed), 0, "without the route {missing:?}");
    }
}

#[test]
fn a_route_the_graph_lacks_goes_serial_at_once_and_a_new_graph_brings_it_back() {
    let mut e = rig();
    let g = project(&mut e);
    e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
    play(&mut e, 2);
    assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 2);
    // Track 3 starts sending to aux 6.
    e.tx.push(Cmd::Params(3, params(None, &[(7, 0.6, true), (6, 0.5, false)]))).ok().unwrap();
    play(&mut e, 2);
    assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 2, "those two blocks played serially");
    let ids = vec![1, 2, 3, 4, 5, 6, 7];
    let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4), (3, 6)];
    e.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(ids, &edges, |_| true).unwrap())))).ok().unwrap();
    play(&mut e, 2);
    assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 4);
}

#[test]
fn a_light_project_stays_on_the_callback_until_the_load_climbs() {
    let mut p = Parallel::default();
    p.weigh(0.05, 0.05);
    assert!(!p.engaged);
    p.weigh(0.2, 0.06);
    assert!(p.engaged, "one heavy block is enough");
    p.weigh(0.01, 0.2);
    assert!(p.engaged, "the average is still up");
    p.weigh(0.01, 0.1);
    assert!(!p.engaged, "no workers: under the mark means under it");
}

/// A panic in a strip, once it's told to, on whichever thread has it.
struct Bomb(Arc<std::sync::atomic::AtomicBool>);

impl Effect for Bomb {
    fn set_param(&mut self, _: u32, _: f32) {}
    fn param(&self, _: u32) -> f32 {
        0.0
    }
    fn process(&mut self, l: &mut [f32], _: &mut [f32], _: Key, _: &FxContext) {
        if self.0.swap(false, Ordering::SeqCst) {
            panic!("expected: a plug-in blowing up in an engine test");
        }
        l.fill(0.5);
    }
    fn reset(&mut self) {}
}

#[test]
fn a_panicking_strip_silences_its_block_and_only_that() {
    let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut e = rig();
    let g = project(&mut e);
    let info = EffectInfo {
        id: "test:bomb",
        name: "Bomb",
        category: "Utility",
        params: &[],
        meters: &[],
        sidechain: false,
        new: |_| Box::new(Tone { phase: 0.0, hz: 0.0, start: 0.0 }),
    };
    let mut b = BuiltinDsp::new(&info, SR as f32, &[]);
    b.replace_effect_for_tests(Box::new(Bomb(armed.clone())));
    e.tx.push(Cmd::Insert {
        track: 3,
        slot: 1,
        insert: Some(InsertDsp { uid: 99, bypass: false, plugin: Plugin::Builtin(Box::new(b)), key: KeyRoute::default() }),
    })
    .ok()
    .unwrap();
    e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
    let mut out = vec![0.0f32; BLOCK * 2];
    for _ in 0..5 {
        e.engine.process(&[], 0, &mut out, 2);
    }
    assert!(out.iter().any(|s| s.abs() > 0.01));
    armed.store(true, Ordering::SeqCst);
    e.engine.process(&[], 0, &mut out, 2);
    assert!(out.iter().all(|&s| s == 0.0), "silence for the block");
    assert_eq!(e.status.engine_errors.load(Ordering::Relaxed), 1, "and it's counted");
    // Reporting a panic can take long enough to count as an overload (with a backtrace on
    // Windows it does), and the engine then stays quiet for `SHED_SECS`.
    let blocks = (2.0 * SHED_SECS * SR / BLOCK as f64) as usize;
    let heard = (0..blocks).any(|_| {
        e.engine.process(&[], 0, &mut out, 2);
        out.iter().any(|s| s.abs() > 0.01)
    });
    assert!(heard, "the pool isn't poisoned");
    assert_eq!(e.status.engine_errors.load(Ordering::Relaxed), 1);
}

/// A rig whose track 2 has Tar on worker threads, on a pool of `threads`.
fn tarred(threads: usize, hold: &Arc<AtomicBool>) -> Rig {
    let mut e = rig();
    let g = project(&mut e);
    e.tx.push(Cmd::Insert { track: 2, slot: 1, insert: Some(tar_for_checks(99, SR as f32, hold.clone())) }).ok().unwrap();
    e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads, ..Default::default() })))).ok().unwrap();
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: false })).ok().unwrap();
    e
}

/// Plays until a worker gets track 2's strip and is left in Tar.
/// Not until any overload: on a busy machine a block can be counted as one for running late, or
/// be given up on with a worker kept off the CPU, and the block then stays open for a while.
/// Should those give-ups have retired every worker before one got the strip, the pool is started
/// afresh.
fn until_stuck(e: &mut Rig, out: &mut [f32]) {
    let started = Instant::now();
    while e.status.left_events.load(Ordering::Relaxed) == 0 || e.engine.parallel.stalled {
        assert!(started.elapsed().as_secs() < 60, "no worker ever got the strip");
        e.engine.process(&[], 0, out, 2);
        let w = e.engine.parallel.workers.as_ref().unwrap();
        if e.status.left_events.load(Ordering::Relaxed) == 0 && !e.engine.parallel.stalled && !w.pool.has_workers() {
            let threads = w.threads();
            e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads, ..Default::default() })))).ok().unwrap();
            e.engine.process(&[], 0, out, 2);
        }
    }
}

/// Plays until a block given up on stays open.
fn until_stalled(e: &mut Rig, out: &mut [f32]) {
    let started = Instant::now();
    while !e.engine.parallel.stalled {
        assert!(started.elapsed().as_secs() < 60, "no block was given up on");
        e.engine.process(&[], 0, out, 2);
    }
}

/// Track 2 and what it feeds, per `project`'s edges.
const LEFT_OUT: [Id; 5] = [2, 4, 5, 6, 7];

#[test]
fn a_plug_in_that_never_returns_costs_its_track_and_what_it_feeds_and_the_rest_plays_on() {
    // Only a worker may be left in a plug-in: a task on the calling thread isn't given up on (as a
    // plug-in that never returns can't be). So everything is made to wait for a worker: a
    // two-thread pool whose caller finds nothing left to take is the sure way to get there, and
    // the test tries until the worker has the strip.
    let hold = Arc::new(AtomicBool::new(true));
    let mut e = tarred(2, &hold);
    let mut out = vec![1.0f32; BLOCK * 2];
    until_stuck(&mut e, &mut out);
    assert!(out.iter().all(|&s| s == 0.0), "the block given up on is silent");
    assert!(!e.engine.parallel.stalled, "and over");
    let st = &e.status;
    assert!(st.overload_stuck.load(Ordering::Relaxed) && st.overload_left_out.load(Ordering::Relaxed));
    let left = (st.left_events.load(Ordering::Relaxed), st.left_track.load(Ordering::Relaxed), st.left_plugin.load(Ordering::Relaxed));
    assert_eq!(left, (1, 2, 99), "the message names it");
    assert_eq!(ids(&st.held()), [(2, 99)]);
    assert_eq!(st.left_out.load(Ordering::Relaxed), 5);
    for t in &e.engine.tracks {
        assert_eq!(t.left_out, LEFT_OUT.contains(&t.id), "track {}", t.id);
    }
    // A stand-in has taken the strip's place.
    let two = e.engine.tracks.iter().find(|t| t.id == 2).unwrap();
    assert!(two.inserts.iter().all(Option::is_none));
    // Commands don't wait for the thread still in Tar, the one meant for its track included.
    let done = e.status.cmds_done.load(Ordering::Relaxed);
    e.tx.push(Cmd::Params(2, params(Some(5), &[]))).ok().unwrap();
    e.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert_eq!(e.status.cmds_done.load(Ordering::Relaxed), done + 2);
    // From here on a block running late on a busy machine mustn't be given up on or shed.
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
    // Past the overload's silence, tracks 1 and 3 play, serially now (the worker was the only one).
    let started = Instant::now();
    while e.engine.shedding > 0 {
        assert!(started.elapsed().as_secs() < 20);
        e.engine.process(&[], 0, &mut out, 2);
    }
    for t in &e.engine.tracks {
        t.meter.take();
    }
    let pooled = e.status.parallel_blocks.load(Ordering::Relaxed);
    let mut heard = false;
    for _ in 0..20 {
        e.engine.process(&[], 0, &mut out, 2);
        heard |= out.iter().any(|s| s.abs() > 0.01);
    }
    assert!(heard, "the rest plays on");
    assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), pooled);
    for t in &e.engine.tracks {
        let (l, r) = t.meter.take();
        assert_eq!(l.max(r) > 0.0, !LEFT_OUT.contains(&t.id), "track {}'s meter", t.id);
    }
    let w = e.engine.parallel.workers.as_ref().unwrap();
    assert_eq!((w.playing(), w.health().report().stuck), (1, 1));
    // Tar returns at last: the strip it held comes back as garbage, its plug-in still in it, and
    // nothing else changes.
    hold.store(false, Ordering::SeqCst);
    let started = Instant::now();
    let held = loop {
        assert!(started.elapsed().as_secs() < 20, "the strip never came back");
        e.engine.process(&[], 0, &mut out, 2);
        match e.garbage.pop() {
            Ok(Garbage::Track(t)) => break t,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(1)),
        }
    };
    assert_eq!((held.id, held.inserts[1].as_ref().map(|i| i.uid)), (2, Some(99)));
    assert!(e.status.held().is_empty());
    let back = e.status.take_back();
    assert_eq!(ids(&back), [(2, 99)], "for the session to bring back");
    assert!(e.status.take_back().is_empty());
    assert!(e.engine.tracks.iter().all(|t| t.left_out == LEFT_OUT.contains(&t.id)), "until it brings them back");
    assert_eq!(e.engine.parallel.workers.as_ref().unwrap().health().report().stuck, 1);
    // The session sends the track a new strip, and the rest comes back with it.
    let fresh = strip(2, TrackKind::Audio, params(Some(5), &[(6, 0.4, false)]), vec![tone(20, 170.0, None, false)]);
    e.tx.push(Cmd::Back { token: back[0].token, track: Some(fresh), still_out: Vec::new() }).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert!(e.engine.tracks.iter().all(|t| !t.left_out));
    assert_eq!(e.status.left_out.load(Ordering::Relaxed), 0);
    for t in &e.engine.tracks {
        t.meter.take();
    }
    for _ in 0..20 {
        e.engine.process(&[], 0, &mut out, 2);
    }
    for t in &e.engine.tracks {
        let (l, r) = t.meter.take();
        assert!(l.max(r) > 0.0, "track {} plays again", t.id);
    }
}

use test_hooks::{after, gate, hold_in_task};

/// Plays until the engine is over the block it gave up on.
fn until_unstalled(e: &mut Rig, out: &mut [f32]) {
    let started = Instant::now();
    while e.engine.parallel.stalled {
        assert!(started.elapsed().as_secs() < 20, "the block given up on never ended");
        e.engine.process(&[], 0, out, 2);
    }
}

/// Issue #8 again, from inside the task: given up on while in its strip's own work, the worker
/// is retired and the block stays open; when the strip then goes into a plug-in that doesn't
/// return, the worker is left there after all, and the rest plays on.
#[test]
fn a_worker_given_up_on_in_its_task_is_left_once_it_is_stuck_in_a_plug_in() {
    let hold = Arc::new(AtomicBool::new(true));
    let mut e = rig();
    let g = project(&mut e);
    // Track 8 plays straight to Stereo Out, with Tar.
    let eight = strip(8, TrackKind::Audio, params(None, &[]), vec![tone(80, 290.0, None, false), tar_for_checks(98, SR as f32, hold.clone())]);
    e.tx.push(Cmd::AddTrack(eight, 7)).ok().unwrap();
    let order = (0..g.len()).map(|i| g.id(i)).chain([8]).collect();
    let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)];
    e.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(order, &edges, |_| true).unwrap())))).ok().unwrap();
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 2, ..Default::default() })))).ok().unwrap();
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: false })).ok().unwrap();
    hold_in_task(8, true);
    let mut out = vec![0.0f32; BLOCK * 2];
    until_stalled(&mut e, &mut out);
    assert!(e.engine.parallel.stalled && !e.status.overload_left_out.load(Ordering::Relaxed), "held up in its task");
    let w = e.engine.parallel.workers.as_ref().unwrap();
    assert_eq!(w.health().report().stuck, 1, "and retired");
    // On into Tar, which doesn't return: once it has been in there for most of the limit, the
    // worker is left in it, and the block ends without it.
    hold_in_task(8, false);
    until_unstalled(&mut e, &mut out);
    let st = &e.status;
    assert_eq!((st.left_events.load(Ordering::Relaxed), st.left_track.load(Ordering::Relaxed), st.left_plugin.load(Ordering::Relaxed)), (1, 8, 98));
    assert_eq!(ids(&st.held()), [(8, 98)]);
    assert!(e.engine.tracks.iter().all(|t| t.left_out == (t.id == 8)));
    let done = e.status.cmds_done.load(Ordering::Relaxed);
    e.tx.push(Cmd::Stop).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert_eq!(e.status.cmds_done.load(Ordering::Relaxed), done + 1, "commands go through");
    hold.store(false, Ordering::SeqCst);
    let started = Instant::now();
    while !e.status.held().is_empty() {
        assert!(started.elapsed().as_secs() < 20, "the strip never came back");
        e.engine.process(&[], 0, &mut out, 2);
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Track ids for `stalled_with_tar`, each test's own: the caller's gate, a strip for each of the
/// two workers to wait in, the track with Tar, and the one a worker is held up in.
struct Stall {
    gate: Id,
    waits: [Id; 2],
    tar: Id,
    held: Id,
}

impl Stall {
    fn new(base: Id) -> Stall {
        Stall { gate: base, waits: [base + 1, base + 2], tar: base + 3, held: base + 4 }
    }

    /// Lets everything go.
    fn end(&self, hold: &AtomicBool) {
        gate(self.gate, &[]);
        for w in self.waits {
            after(w, None);
        }
        hold_in_task(self.held, false);
        hold.store(false, Ordering::SeqCst);
    }
}

/// A block on three threads that stays open with one worker left in Tar and the other held up
/// in its own task, set up by synchronisation alone. `project`'s tracks come after five of
/// `ids`', each to Stereo Out: the caller's first task is its gate, and each worker's first is
/// a strip it waits in until the caller is in the gate, so no worker takes the gate; the caller
/// waits there until the workers have the Tar track and the held one, so it takes neither.
fn stalled_with_tar(ids: &Stall, hold: &Arc<AtomicBool>) -> Rig {
    let mut e = rig();
    project(&mut e);
    let tar = strip(ids.tar, TrackKind::Audio, params(None, &[]), vec![tone(92, 270.0, None, false), tar_for_checks(96, SR as f32, hold.clone())]);
    let plain = |id: Id| strip(id, TrackKind::Audio, params(None, &[]), vec![tone(93, 350.0, None, false)]);
    for t in [plain(ids.held), tar, plain(ids.waits[1]), plain(ids.waits[0]), plain(ids.gate)] {
        e.tx.push(Cmd::AddTrack(t, 0)).ok().unwrap();
    }
    let order = [ids.gate, ids.waits[0], ids.waits[1], ids.tar, ids.held].into_iter().chain(1..=7).collect();
    let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)];
    e.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(order, &edges, |_| true).unwrap())))).ok().unwrap();
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 3, ..Default::default() })))).ok().unwrap();
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: false })).ok().unwrap();
    gate(ids.gate, &[ids.tar, ids.held]);
    for w in ids.waits {
        after(w, Some(ids.gate));
    }
    hold_in_task(ids.held, true);
    let mut out = vec![0.0f32; BLOCK * 2];
    until_stalled(&mut e, &mut out);
    assert!(e.engine.parallel.stalled, "the worker in its own task holds the block open");
    // Once in Tar for long enough, the other worker is left there, from the callback if not at
    // the give-up already.
    let started = Instant::now();
    while e.engine.parallel.workers.as_ref().unwrap().fresh.is_empty() {
        assert!(started.elapsed().as_secs() < 60, "the worker in Tar was never left");
        e.engine.process(&[], 0, &mut out, 2);
    }
    assert!(e.engine.parallel.stalled);
    e
}

/// Two workers held up at the give-up, one in Tar for long enough, one in its own task: the
/// block stays open after all, as the caller can't leave the second. Once that one is through,
/// the first's strip is left out, from the callback or from `settle`, and the session hears of
/// it then.
#[test]
fn a_strip_left_in_a_block_that_stays_open_is_left_out_once_it_ends() {
    for (settle, ids) in [(false, Stall::new(310)), (true, Stall::new(320))] {
        let hold = Arc::new(AtomicBool::new(true));
        let mut e = stalled_with_tar(&ids, &hold);
        assert!(!e.status.overload_left_out.load(Ordering::Relaxed), "the session stops the transport for a stalled block");
        assert_eq!(e.status.left_events.load(Ordering::Relaxed), 0, "nothing is left out yet");
        hold_in_task(ids.held, false);
        if settle {
            e.engine.settle();
        } else {
            let mut out = vec![0.0f32; BLOCK * 2];
            until_unstalled(&mut e, &mut out);
        }
        assert!(!e.engine.parallel.stalled);
        assert_eq!((e.status.left_events.load(Ordering::Relaxed), e.status.left_track.load(Ordering::Relaxed)), (1, ids.tar), "now it's said");
        assert!(e.engine.tracks.iter().all(|t| t.left_out == (t.id == ids.tar)), "settle: {settle}");
        ids.end(&hold);
    }
}

#[test]
fn two_strips_can_be_left_out_and_come_back_one_after_the_other() {
    let hold = Arc::new(AtomicBool::new(true));
    let three = Arc::new(AtomicBool::new(true));
    let mut e = tarred(4, &hold);
    e.tx.push(Cmd::Insert { track: 3, slot: 1, insert: Some(tar_for_checks(97, SR as f32, three.clone())) }).ok().unwrap();
    let mut out = vec![0.0f32; BLOCK * 2];
    let started = Instant::now();
    while e.status.held().len() < 2 {
        assert!(started.elapsed().as_secs() < 30, "only {:?} left", e.status.held());
        e.engine.process(&[], 0, &mut out, 2);
    }
    until_unstalled(&mut e, &mut out);
    let both = [2, 3, 4, 5, 6, 7];
    assert!(e.engine.tracks.iter().all(|t| t.left_out == both.contains(&t.id)));
    // Both workers in Tar, and on a busy enough machine any given up on for being kept off the CPU.
    assert!(e.engine.parallel.workers.as_ref().unwrap().health().report().stuck >= 2);
    // Track 3's Tar returns first: 3 comes back, but 7 is still fed by 2.
    three.store(false, Ordering::SeqCst);
    let started = Instant::now();
    while e.status.held().len() > 1 {
        assert!(started.elapsed().as_secs() < 20);
        e.engine.process(&[], 0, &mut out, 2);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(ids(&e.status.held()), [(2, 99)]);
    let back = e.status.take_back();
    assert_eq!(ids(&back), [(3, 97)]);
    let fresh = strip(3, TrackKind::Audio, params(None, &[(7, 0.6, true)]), vec![tone(30, 230.0, None, false)]);
    e.tx.push(Cmd::Back { token: back[0].token, track: Some(fresh), still_out: LEFT_OUT.to_vec() }).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert!(e.engine.tracks.iter().all(|t| t.left_out == LEFT_OUT.contains(&t.id)));
    hold.store(false, Ordering::SeqCst);
}

#[test]
fn after_a_worker_is_left_in_a_plug_in_the_others_play_on() {
    let hold = Arc::new(AtomicBool::new(true));
    let mut e = tarred(4, &hold);
    let mut out = vec![0.0f32; BLOCK * 2];
    until_stuck(&mut e, &mut out);
    // From here on a block running late on a busy machine mustn't be given up on or shed.
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
    while e.engine.shedding > 0 {
        e.engine.process(&[], 0, &mut out, 2);
    }
    // The worker in Tar, and on a machine busy enough, one given up on for being kept off the
    // CPU: each is out of the pool.
    let started = Instant::now();
    while e.engine.parallel.workers.as_ref().unwrap().health().report().starting > 0 {
        assert!(started.elapsed().as_secs() < 60, "the workers never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    let w = e.engine.parallel.workers.as_ref().unwrap();
    let stuck = w.health().report().stuck;
    assert!(stuck >= 1 && w.playing() == 4 - stuck as usize, "{stuck} given up on, {} playing", w.playing());
    let playing = w.playing() as u32;
    // Removing the plug-in reaches the stand-in, and the rest plays on the threads left, with
    // Tar still holding the fourth.
    e.tx.push(Cmd::Insert { track: 2, slot: 1, insert: None }).ok().unwrap();
    let before = e.status.parallel_blocks.load(Ordering::Relaxed);
    let got = play(&mut e, 40);
    assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed) - before, 40, "every block on the pool");
    assert_eq!(e.status.threads.load(Ordering::Relaxed), playing);
    assert!(got.iter().any(|s| s.abs() > 0.01));
    assert!(e.engine.tracks.iter().all(|t| t.left_out == LEFT_OUT.contains(&t.id)));
    hold.store(false, Ordering::SeqCst);
}

/// The garbage that has come back so far: strips of which tracks, and how many workers.
fn garbage(e: &mut Rig) -> (Vec<Id>, usize) {
    let (mut tracks, mut workers) = (Vec::new(), 0);
    while let Ok(g) = e.garbage.pop() {
        match g {
            Garbage::Track(t) => tracks.push(t.id),
            Garbage::Workers(_) => workers += 1,
            _ => {}
        }
    }
    (tracks, workers)
}

/// The thread count changes while a thread is left in a plug-in, twice: the workers holding the
/// strip are kept until it's back, and its track then comes back as ever, with what it feeds.
#[test]
fn workers_replaced_while_a_thread_is_left_in_a_plug_in_hand_its_strip_back() {
    let hold = Arc::new(AtomicBool::new(true));
    let mut e = tarred(2, &hold);
    let mut out = vec![0.0f32; BLOCK * 2];
    until_stuck(&mut e, &mut out);
    assert_eq!(ids(&e.status.held()), [(2, 99)]);
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 3, ..Default::default() })))).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert_eq!(garbage(&mut e).1, 0, "the old workers are kept, as a thread of theirs holds a strip");
    // From here on a block running late on a busy machine mustn't be given up on or shed.
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
    let got = play(&mut e, 200);
    assert!(got.iter().any(|s| s.abs() > 0.01), "the new pool plays");
    // Replaced again: those held nothing, and go at once.
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert_eq!(garbage(&mut e).1, 1);
    // A graph the old thread may still hold is let go of without being freed.
    e.tx.push(Cmd::Graph(None)).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert_eq!(ids(&e.status.held()), [(2, 99)], "still listed");
    // Tar returns: the strip comes back, and then the workers that held it.
    hold.store(false, Ordering::SeqCst);
    let (mut tracks, mut workers) = (Vec::new(), 0);
    let started = Instant::now();
    while workers == 0 {
        assert!(started.elapsed().as_secs() < 20, "the strip never came back");
        e.engine.process(&[], 0, &mut out, 2);
        let (t, w) = garbage(&mut e);
        tracks.extend(t);
        workers += w;
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!((tracks, workers), (vec![2], 1));
    assert!(e.status.held().is_empty());
    let back = e.status.take_back();
    assert_eq!(ids(&back), [(2, 99)], "for the session to bring back");
    let fresh = strip(2, TrackKind::Audio, params(Some(5), &[(6, 0.4, false)]), vec![tone(20, 170.0, None, false)]);
    e.tx.push(Cmd::Back { token: back[0].token, track: Some(fresh), still_out: Vec::new() }).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert!(e.engine.tracks.iter().all(|t| !t.left_out && !t.stand_in));
    assert_eq!(e.status.left_out.load(Ordering::Relaxed), 0);
    for t in &e.engine.tracks {
        t.meter.take();
    }
    play(&mut e, 20);
    for t in &e.engine.tracks {
        let (l, r) = t.meter.take();
        assert!(l.max(r) > 0.0, "track {} plays again", t.id);
    }
}

/// The engine goes while workers it replaced still have a thread left in a plug-in: it doesn't
/// wait for the thread.
#[test]
fn an_engine_with_replaced_workers_still_holding_a_strip_drops_at_once() {
    let hold = Arc::new(AtomicBool::new(true));
    let mut e = tarred(2, &hold);
    let mut out = vec![0.0f32; BLOCK * 2];
    until_stuck(&mut e, &mut out);
    e.tx.push(Cmd::Workers(None)).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    let t = Instant::now();
    drop(e);
    assert!(t.elapsed() < Duration::from_secs(5));
    // Tar returns into what was left behind for it.
    hold.store(false, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(20));
}

/// The track comes back from `Cmd::Back` still fed by a plug-in a thread is left in: it sits
/// out with the rest of those, until that one is back too. A `Cmd::Back` for a stand-in that
/// has gone (its track deleted, then brought back by undo with a strip of its own) leaves the
/// strip the track has now.
#[test]
fn a_track_brought_back_while_its_feeder_is_still_held_sits_out() {
    let mut e = rig();
    project(&mut e);
    let mut out = vec![0.0f32; BLOCK * 2];
    // As `leave_out` would leave them: 1 and 4 are stand-ins (4 waits for 1), and aux 6 and 7
    // sit out with them.
    for t in e.engine.tracks.iter_mut().filter(|t| [1, 4, 6, 7].contains(&t.id)) {
        t.sit_out();
    }
    for (i, token) in [(0, 4), (3, 5)] {
        e.engine.tracks[i].stand_in = true;
        e.engine.tracks[i].held_token = token;
    }
    // Track 4's plug-in returns, but 1 is still held.
    let four = strip(4, TrackKind::Audio, params(None, &[(6, 0.3, false)]), vec![tone(40, 310.0, None, false)]);
    e.tx.push(Cmd::Back { token: 5, track: Some(four), still_out: vec![1, 4, 6, 7] }).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    let four = e.engine.tracks.iter().find(|t| t.id == 4).unwrap();
    assert!(four.left_out && !four.stand_in, "the new strip, sitting out");
    assert_eq!(e.status.left_out.load(Ordering::Relaxed), 4);
    // One for a stand-in that isn't there any more: the new strip goes, and 4's stays.
    let other = strip(4, TrackKind::Audio, params(None, &[]), vec![]);
    e.tx.push(Cmd::Back { token: 5, track: Some(other), still_out: vec![1, 4, 6, 7] }).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    let four = e.engine.tracks.iter().find(|t| t.id == 4).unwrap();
    assert_eq!(four.params.sends[0].map(|s| s.bus), Some(6), "the strip it had");
}

fn ids(v: &[HeldStrip]) -> Vec<(Id, Id)> {
    v.iter().map(|h| (h.track, h.plugin)).collect()
}

/// Removing a stand-in (its track deleted) says so in its entry, for the session: whatever strip
/// the track gets later isn't the one held.
#[test]
fn a_stand_in_removed_with_its_track_is_noted_in_its_entry() {
    let hold = Arc::new(AtomicBool::new(true));
    let mut e = tarred(2, &hold);
    let mut out = vec![0.0f32; BLOCK * 2];
    until_stuck(&mut e, &mut out);
    assert!(!e.status.held()[0].stand_in_gone);
    e.tx.push(Cmd::RemoveTrack(2)).ok().unwrap();
    e.engine.process(&[], 0, &mut out, 2);
    assert!(e.status.held()[0].stand_in_gone);
    hold.store(false, Ordering::SeqCst);
}

/// Each strip's `StripLoad` names the thread that played it: the caller waits in the first strip
/// until a worker has started the second (see `test_hooks::gate`), so they're played apart.
#[test]
fn a_strip_notes_the_thread_that_played_it() {
    let (on_caller, on_worker) = (961, 962);
    let mut e = rig();
    let tracks = vec![
        strip(on_caller, TrackKind::Audio, params(None, &[]), vec![tone(963, 210.0, None, false)]),
        strip(on_worker, TrackKind::Audio, params(None, &[]), vec![tone(964, 290.0, None, false)]),
    ];
    let loads: Vec<Arc<StripLoad>> = tracks.iter().map(|t| t.load.clone()).collect();
    e.engine.replace_graph(SR, tracks, (0..INSERT_SLOTS).map(|_| None).collect());
    e.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(vec![on_caller, on_worker], &[], |_| true).unwrap())))).ok().unwrap();
    e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 2, ..Default::default() })))).ok().unwrap();
    e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
    gate(on_caller, &[on_worker]);
    after(on_worker, Some(on_caller));
    let mut out = vec![0.0f32; BLOCK * 2];
    e.engine.process(&[], 0, &mut out, 2);
    gate(on_caller, &[]);
    after(on_worker, None);
    let (caller, worker) = (loads[0].take(), loads[1].take());
    assert_eq!((caller.thread, worker.thread), (Some(0), Some(1)));
    assert!(caller.share > 0.0 && worker.share > 0.0);
}
