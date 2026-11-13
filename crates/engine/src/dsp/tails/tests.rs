//! How long a plug-in is fed silence before a render: until it has been quiet for its tail.
//! The plug-ins here are stand-ins that ring for a while, as LSP's Multi-Sampler plays a sample
//! to its end after the note-off.

use super::*;

const BLOCK: usize = 512;
const SEC: u64 = 48_000;
const HOLD: u64 = SEC / 10;
const LIMIT: u64 = 10 * SEC;

/// A voice that sounds for `rings` frames after its note-off, with silences of `gap` frames
/// every `every` (an echo, say), and nothing after. Returns what `ring_out` ran and the frames
/// the voice had left.
fn voice(rings: u64, every: u64, gap: u64) -> (u64, u64) {
    let mut at = 0u64;
    let mut note_off = false;
    let ran = ring_out(BLOCK, HOLD, LIMIT, |first| {
        note_off |= first;
        let sounds = (at..at + BLOCK as u64).any(|f| f < rings && (every == 0 || f % every >= gap));
        at += BLOCK as u64;
        sounds
    });
    assert!(note_off, "the first block carries the notes' end");
    (ran, rings.saturating_sub(at))
}

#[test]
fn a_quiet_plugin_costs_its_hold() {
    let (ran, _) = voice(0, 0, 0);
    assert!((HOLD..HOLD + BLOCK as u64).contains(&ran), "{ran}");
}

#[test]
fn a_voice_that_outlasts_its_note_off_plays_to_its_end() {
    // Three seconds of sample after the note-off.
    let (ran, left) = voice(3 * SEC, 0, 0);
    assert_eq!(left, 0);
    assert!(ran >= 3 * SEC + HOLD && ran < 3 * SEC + HOLD + 2 * BLOCK as u64, "{ran}");
}

#[test]
fn silences_shorter_than_the_hold_dont_end_it() {
    let (_, left) = voice(2 * SEC, SEC / 4, HOLD / 2);
    assert_eq!(left, 0);
}

#[test]
fn a_plugin_that_never_goes_quiet_stops_at_the_limit() {
    let ran = ring_out(BLOCK, HOLD, LIMIT, |_| true);
    assert!((LIMIT..LIMIT + BLOCK as u64).contains(&ran), "{ran}");
}

#[test]
fn nan_is_sound() {
    assert!(loud(&[0.0, f32::NAN]));
    assert!(loud(&[0.0, 2e-6]));
    assert!(!loud(&[0.0, -5e-7]));
}
