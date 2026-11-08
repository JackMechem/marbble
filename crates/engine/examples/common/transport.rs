//! Waiting for the engine's transport in the headless checks. The engine takes Play and Stop on
//! its own thread, and `play()` is ignored until it has really stopped, so a fixed sleep can be
//! too short on a slow machine (GitHub's macOS runners) and a check then reads silence.
use std::time::{Duration, Instant};

use daw_engine::Session;

/// Waits until the engine is playing (or stopped), for at most two seconds.
pub fn wait_for(s: &Session, playing: bool) {
    let start = Instant::now();
    while s.is_playing() != playing && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
}
