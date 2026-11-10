//! The voice allocator: polyphony, stealing, note priority, the pedal, unison and spread.

use super::*;
use VoiceAction::*;

fn collect<const V: usize>(f: impl FnOnce(&mut VoiceAllocator<V>, &mut dyn FnMut(VoiceAction)), a: &mut VoiceAllocator<V>) -> Vec<VoiceAction> {
    let mut out = Vec::new();
    f(a, &mut |x| out.push(x));
    out
}

fn on<const V: usize>(a: &mut VoiceAllocator<V>, note: u8) -> Vec<VoiceAction> {
    collect(|a, act| a.note_on(note, 0.5, act), a)
}

fn off<const V: usize>(a: &mut VoiceAllocator<V>, note: u8) -> Vec<VoiceAction> {
    collect(|a, act| a.note_off(note, act), a)
}

fn poly<const V: usize>(unison: usize) -> VoiceAllocator<V> {
    let mut a = VoiceAllocator::<V>::new();
    a.set_mode(VoiceMode::Poly, unison, V, |_| {});
    a
}

fn mono(priority: Priority, multi_trigger: bool) -> VoiceAllocator<4> {
    let mut a = VoiceAllocator::<4>::new();
    a.set_mode(VoiceMode::Mono { priority, multi_trigger }, 1, 4, |_| {});
    a
}

#[test]
fn poly_uses_free_voices_then_steals_released_before_held() {
    let mut a = poly::<3>(1);
    assert!(matches!(on(&mut a, 60)[..], [Start { voice: 0, note: 60, stolen: false, .. }]));
    assert!(matches!(on(&mut a, 62)[..], [Start { voice: 1, note: 62, stolen: false, .. }]));
    assert!(matches!(on(&mut a, 64)[..], [Start { voice: 2, note: 64, stolen: false, .. }]));
    // Let go of the middle one: it goes first.
    assert_eq!(off(&mut a, 62), vec![Release { voice: 1 }]);
    assert!(matches!(on(&mut a, 65)[..], [Start { voice: 1, note: 65, stolen: true, .. }]));
    // Then the oldest held.
    assert!(matches!(on(&mut a, 67)[..], [Start { voice: 0, note: 67, stolen: true, .. }]));
    // A note-off for what was stolen does nothing.
    assert_eq!(off(&mut a, 60), vec![]);
    assert_eq!(off(&mut a, 67), vec![Release { voice: 0 }]);
    a.voice_finished(0);
    assert!(matches!(on(&mut a, 70)[..], [Start { voice: 0, stolen: false, .. }]));
}

#[test]
fn sustain_pedal_holds_until_it_lifts() {
    let mut a = poly::<4>(1);
    on(&mut a, 60);
    collect(|a, act| a.set_pedal(true, act), &mut a);
    assert_eq!(off(&mut a, 60), vec![]);
    // Playing it again uses the same voice.
    assert!(matches!(on(&mut a, 60)[..], [Start { voice: 0, .. }]));
    assert_eq!(off(&mut a, 60), vec![]);
    assert_eq!(collect(|a, act| a.set_pedal(false, act), &mut a), vec![Release { voice: 0 }]);
}

#[test]
fn low_note_priority_keeps_the_lowest_and_returns_to_it() {
    let mut a = mono(Priority::Low, false);
    assert!(matches!(on(&mut a, 60)[..], [Start { note: 60, .. }]));
    assert_eq!(on(&mut a, 67), vec![], "a higher key changes nothing");
    assert_eq!(on(&mut a, 55), vec![Glide { voice: 0, note: 55 }]);
    assert_eq!(off(&mut a, 67), vec![]);
    assert_eq!(off(&mut a, 55), vec![Glide { voice: 0, note: 60 }]);
    assert_eq!(off(&mut a, 60), vec![Release { voice: 0 }]);
    assert!(matches!(on(&mut a, 48)[..], [Start { note: 48, .. }]), "all keys were up: a fresh note");
}

#[test]
fn last_and_high_priority_and_multi_trigger() {
    let mut a = mono(Priority::Last, false);
    on(&mut a, 60);
    assert_eq!(on(&mut a, 55), vec![Glide { voice: 0, note: 55 }]);
    assert_eq!(off(&mut a, 55), vec![Glide { voice: 0, note: 60 }]);

    let mut a = mono(Priority::High, false);
    on(&mut a, 60);
    assert_eq!(on(&mut a, 55), vec![]);
    assert_eq!(on(&mut a, 62), vec![Glide { voice: 0, note: 62 }]);

    let mut a = mono(Priority::Last, true);
    on(&mut a, 60);
    assert!(matches!(on(&mut a, 62)[..], [Start { voice: 0, note: 62, stolen: true, .. }]));
}

#[test]
fn pedal_in_mono_keeps_the_note_until_it_lifts() {
    let mut a = mono(Priority::Last, false);
    on(&mut a, 60);
    collect(|a, act| a.set_pedal(true, act), &mut a);
    assert_eq!(off(&mut a, 60), vec![]);
    assert_eq!(collect(|a, act| a.set_pedal(false, act), &mut a), vec![Release { voice: 0 }]);
}

#[test]
fn unison_moves_together_and_steals_whole_groups() {
    let mut a = poly::<4>(2);
    let s = on(&mut a, 60);
    assert_eq!(s.len(), 2);
    assert!(matches!(s[1], Start { voice: 1, unison: (1, 2), .. }));
    on(&mut a, 62);
    let stolen = on(&mut a, 64);
    assert!(stolen.iter().all(|x| matches!(x, Start { stolen: true, .. })), "{stolen:?}");
    assert_eq!(off(&mut a, 62).len(), 2);

    let mut m = VoiceAllocator::<4>::new();
    m.set_mode(VoiceMode::Mono { priority: Priority::Last, multi_trigger: false }, 3, 4, |_| {});
    assert_eq!(on(&mut m, 60).len(), 3);
    assert_eq!(on(&mut m, 61).iter().filter(|x| matches!(x, Glide { .. })).count(), 3);
    assert_eq!(off(&mut m, 61).len(), 3, "back to 60");
    assert_eq!(off(&mut m, 60).iter().filter(|x| matches!(x, Release { .. })).count(), 3);
}

#[test]
fn polyphony_caps_the_pool_in_whole_groups() {
    let mut a = VoiceAllocator::<8>::new();
    a.set_mode(VoiceMode::Poly, 2, 5, |_| {});
    for n in 0..3 {
        for x in on(&mut a, 60 + n) {
            let Start { voice, .. } = x else { panic!() };
            assert!(voice < 4, "{voice}: two groups of 2 fit in 5 voices");
        }
    }
}

#[test]
fn rotation_takes_free_voices_in_turn() {
    let mut a = poly::<3>(1);
    a.set_rotate(true);
    assert!(matches!(on(&mut a, 60)[..], [Start { voice: 0, .. }]));
    off(&mut a, 60);
    a.voice_finished(0);
    // The first voice is free again, but the turn has moved on.
    assert!(matches!(on(&mut a, 62)[..], [Start { voice: 1, .. }]));
    assert!(matches!(on(&mut a, 64)[..], [Start { voice: 2, .. }]));
    assert!(matches!(on(&mut a, 65)[..], [Start { voice: 0, stolen: false, .. }]));
}

#[test]
fn spread_is_even_and_centred() {
    assert_eq!(unison_spread(0, 1), (0.0, 0.0));
    assert_eq!(unison_spread(0, 2).0, -1.0);
    assert_eq!(unison_spread(1, 2).0, 1.0);
    let pans: Vec<f32> = (0..4).map(|i| unison_spread(i, 4).1).collect();
    assert_eq!(pans.iter().filter(|p| **p > 0.0).count(), 2);
}
