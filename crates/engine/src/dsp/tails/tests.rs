//! When a plug-in's ring-out stops: quiet, at a noise floor, at the limit or out of time. The
//! plug-ins here are stand-ins that return each block's peak, as LSP's Multi-Sampler plays a
//! sample to its end after the note-off, a reverb decays or a tape model hisses.

use std::cell::Cell;

use super::*;

const BLOCK: usize = 1024;
const SEC: u64 = 48_000;
const HOLD: u64 = SEC / 10;
const RULE: Rule = Rule { window: SEC / 4, limit: 10 * SEC };

/// Frames in seconds.
fn secs(frames: u64) -> f64 {
    frames as f64 / SEC as f64
}

/// Runs `source` (a block's peak from its first frame) with all the time in the world.
fn ring(source: impl Fn(u64) -> f32) -> (u64, End) {
    ring_timed(source, &|| false)
}

fn ring_timed(source: impl Fn(u64) -> f32, out_of_time: &dyn Fn() -> bool) -> (u64, End) {
    let mut at = 0u64;
    let mut note_off = false;
    let out = ring_out(&RULE, BLOCK, HOLD, out_of_time, |first| {
        note_off |= first && at == 0;
        let p = source(at);
        at += BLOCK as u64;
        p
    });
    assert!(note_off, "the first block carries the notes' end");
    out
}

/// Noise at about `level`: each block's peak a little different, as hiss's is.
fn hiss(level: f32) -> impl Fn(u64) -> f32 {
    move |at| {
        let x = (at / BLOCK as u64).wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 40;
        level * (0.8 + 0.2 * (x % 1000) as f32 / 1000.0)
    }
}

/// From 0.5, falling `db_per_sec`.
fn decay(db_per_sec: f64) -> impl Fn(u64) -> f32 {
    move |at| 0.5 * 10f64.powf(-db_per_sec * secs(at) / 20.0) as f32
}

#[test]
fn a_quiet_plugin_costs_its_hold() {
    let (ran, end) = ring(|_| 0.0);
    assert_eq!(end, End::Quiet);
    assert!((HOLD..HOLD + BLOCK as u64).contains(&ran), "{ran}");
}

#[test]
fn a_voice_that_outlasts_its_note_off_plays_to_its_end() {
    // Three seconds of sample after the note-off, at a steady level: not a noise floor.
    let (ran, end) = ring(|at| if at < 3 * SEC { 0.5 } else { 0.0 });
    assert_eq!(end, End::Quiet);
    assert!(ran >= 3 * SEC + HOLD && ran < 3 * SEC + HOLD + 2 * BLOCK as u64, "{ran}");
}

#[test]
fn silences_shorter_than_the_hold_dont_end_it() {
    // An echo every quarter second, with silences of 50 ms between.
    let (ran, end) = ring(|at| if at < 2 * SEC && at % (SEC / 4) >= SEC / 20 { 0.3 } else { 0.0 });
    assert_eq!(end, End::Quiet);
    assert!(ran >= 2 * SEC, "{ran}");
}

#[test]
fn a_steady_noise_floor_stops_it_promptly() {
    // From a limiter's dither to CHOWTapeModel's Degrade.
    for level in [1e-5, 1e-4, 1e-3, 1e-2, 0.025] {
        let (ran, end) = ring(hiss(level));
        assert_eq!(end, End::Floor, "{level}");
        // Three windows: two comparisons.
        assert!(ran <= 3 * RULE.window + BLOCK as u64, "{level}: {}", secs(ran));
    }
}

#[test]
fn a_tail_falling_into_a_noise_floor_plays_down_to_it() {
    // A reverb decaying at 60 dB a second into hiss at −80 dB.
    let floor = hiss(1e-4);
    let tail = decay(60.0);
    let (ran, end) = ring(|at| tail(at).max(floor(at)));
    assert_eq!(end, End::Floor);
    // 0.5 is −6 dB: 74 dB to fall, about 1.23 s.
    // Then three windows to tell it from the floor.
    assert!((1.2..2.3).contains(&secs(ran)), "{}", secs(ran));
}

#[test]
fn a_reverb_tail_that_wavers_plays_on() {
    // 12 dB a second, each block's peak up to 1 dB off: quiet after 9.5 s.
    let tail = decay(12.0);
    let waver = hiss(1.0);
    let (ran, end) = ring(|at| tail(at) * (0.94 + 0.06 * waver(at)));
    assert_eq!(end, End::Quiet);
    assert!(secs(ran) > 9.0, "{}", secs(ran));
}

#[test]
fn a_slowly_decaying_tail_runs_until_quiet_or_the_limit() {
    // 20 dB a second: −6 dB to −120 dB in 5.7 s.
    let (ran, end) = ring(decay(20.0));
    assert_eq!(end, End::Quiet);
    assert!((5.7..5.9).contains(&secs(ran)), "{}", secs(ran));
    // 5 dB a second would take 23 s.
    let (ran, end) = ring(decay(5.0));
    assert_eq!(end, End::Limit);
    assert!(ran >= RULE.limit && ran < RULE.limit + BLOCK as u64);
}

#[test]
fn a_plugin_that_never_goes_quiet_stops_at_the_limit() {
    let (ran, end) = ring(|_| 0.5);
    assert_eq!(end, End::Limit);
    assert!((RULE.limit..RULE.limit + BLOCK as u64).contains(&ran), "{ran}");
}

/// Moves a clock on `step`, as one block takes that long, and says whether the budget is spent.
fn tick(clock: &Cell<Duration>, step: Duration) -> bool {
    clock.set(clock.get() + step);
    clock.get() >= BUDGET
}

#[test]
fn several_hissing_plugins_stay_well_inside_the_budget() {
    // Twelve heavy ones, each block (21 ms of sound) taking 0.5 ms. Played out to the limit, they
    // would take 235 ms each, 2.8 s together.
    let clock = Cell::new(Duration::ZERO);
    for k in 0..12 {
        let (_, end) = ring_timed(hiss(5e-5 * (k + 1) as f32), &|| tick(&clock, Duration::from_micros(500)));
        assert_eq!(end, End::Floor, "plug-in {k}");
    }
    assert!(clock.get() < BUDGET / 2, "{:?}", clock.get());
}

#[test]
fn the_budget_cuts_a_ring_out_short() {
    let clock = Cell::new(Duration::ZERO);
    let (ran, end) = ring_timed(|_| 0.5, &|| tick(&clock, Duration::from_millis(5)));
    assert_eq!(end, End::OutOfTime);
    assert!(ran < RULE.limit && clock.get() >= BUDGET);
}

#[test]
fn nan_is_sound() {
    assert_eq!(peak(&[0.0, f32::NAN, 0.1]), f32::INFINITY);
    assert_eq!(peak(&[0.0, -0.25]), 0.25);
}
