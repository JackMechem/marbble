//! Playing and measuring in the headless checks. The engine takes Play and Stop on its own
//! thread, and `play()` is ignored until it has really stopped, so a fixed sleep can be too
//! short on a slow machine (GitHub's macOS runners) and a check then reads silence. Even once
//! the transport runs, such a machine can play a window late, or fall behind far enough that
//! the tick stops the transport as an overload (as the app does): `play_until` measures until
//! the level a check expects shows up, within a bound, rather than trusting one window.
// Each check uses only some of these.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use daw_engine::model::Id;
use daw_engine::session::Tick;
use daw_engine::Session;

/// How long `play_until` waits for the level it expects.
const PATIENCE: Duration = Duration::from_secs(3);

/// Waits until the engine is playing (or stopped), for at most two seconds.
pub fn wait_for(s: &Session, playing: bool) {
    let start = Instant::now();
    while s.is_playing() != playing && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Starts playing from `from` and lets it settle for 150 ms, the meters taken in after.
fn start(s: &mut Session, from: i64) {
    s.locate(from);
    s.play();
    wait_for(s, true);
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
}

/// Plays from `from` (in the cycle the check set) and returns the first quarter-second window
/// whose tick `ready` accepts, or after three seconds the last one, leaving the transport
/// playing. `ready` says what must have shown up: the sound the check expects. What must stay
/// quiet, and how levels compare, the check asserts on the window returned, as before. A
/// transport the tick stopped (an overload) starts again, and the tick's warnings are printed.
pub fn play_until(s: &mut Session, from: i64, ready: impl Fn(&Tick) -> bool) -> Tick {
    start(s, from);
    let begun = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let t = s.tick();
        if let Some(w) = &t.warning {
            println!("  (tick warning: {w})");
        }
        if ready(&t) {
            return t;
        }
        if begun.elapsed() >= PATIENCE {
            println!("  (the level the check expects didn't show up within {PATIENCE:?}; playing: {})", s.is_playing());
            return t;
        }
        if !s.is_playing() {
            start(s, from);
        }
    }
}

/// Reads a tick every 100 ms until `ready` accepts one, or after three seconds returns the last,
/// printing their warnings. Unlike [`play_until`] it never starts the transport: for checks that
/// something else (a bounce) left it as it was. The tick returned carries the first warning any
/// of them gave, as an overload's says why the transport stopped.
pub fn tick_until(s: &mut Session, ready: impl Fn(&Tick) -> bool) -> Tick {
    let begun = Instant::now();
    let mut warning = None;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let mut t = s.tick();
        if let Some(w) = &t.warning {
            println!("  (tick warning: {w})");
        }
        warning = warning.or_else(|| t.warning.clone());
        if ready(&t) || begun.elapsed() >= PATIENCE {
            t.warning = warning;
            return t;
        }
    }
}

/// One quarter-second window from `from`, however it sounds: for the checks that something is
/// silent, which waiting for sound would only slow down.
pub fn play_window(s: &mut Session, from: i64) -> Tick {
    play_until(s, from, |_| true)
}

/// Plays from `from` until `ready` accepts the peaks of `ids` and Stereo Out (see
/// [`play_until`]), stops, and returns them. [`once`] measures one window, for silence.
pub fn measure(s: &mut Session, from: i64, ids: &[Id], ready: impl Fn(&[f32], f32) -> bool) -> (Vec<f32>, f32) {
    let t = play_until(s, from, |t| {
        let (lv, master) = levels(t, ids);
        ready(&lv, master)
    });
    stop(s);
    levels(&t, ids)
}

/// For [`measure`]: one window, whatever it holds.
pub fn once(_: &[f32], _: f32) -> bool {
    true
}

/// Each track's peak in `ids` (-1 when it has no meter) and Stereo Out's, in a tick.
pub fn levels(t: &Tick, ids: &[Id]) -> (Vec<f32>, f32) {
    let lv = ids.iter().map(|id| t.meters.iter().find(|m| m.0 == *id).map(|m| m.1.max(m.2)).unwrap_or(-1.0)).collect();
    (lv, t.master.0.max(t.master.1))
}

/// Stops and waits for the engine to have stopped (play() is ignored until it has).
pub fn stop(s: &mut Session) {
    s.stop();
    wait_for(s, false);
}
