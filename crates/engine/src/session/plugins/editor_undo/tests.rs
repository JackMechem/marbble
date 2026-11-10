//! Undo for edits in a VST3's own editor: gestures, bursts, echoes and what makes a step.

use super::*;

fn at(start: Instant, ms: u64) -> Instant {
    start + Duration::from_millis(ms)
}

#[test]
fn a_gesture_is_one_edit_however_many_values_it_sends() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.apply(Gesture::Begin(1), false, s);
    for i in 0..200 {
        t.apply(Gesture::Perform(1, i as f64 / 200.0), false, at(s, i * 10));
        assert!(!t.due(at(s, i * 10 + 500), true), "not while it's open");
    }
    t.apply(Gesture::End(1), false, at(s, 2000));
    assert!(!t.due(at(s, 2000), true), "waits for the edits to reach the processor");
    assert!(t.due(at(s, 2000) + SETTLE, true));
    let p = t.take().unwrap();
    assert_eq!((p.bracketed, p.params), (true, vec![1]));
    assert!(t.pending.is_none() && t.open.is_empty());
}

#[test]
fn a_click_that_moves_nothing_is_no_edit() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.apply(Gesture::Begin(1), false, s);
    t.apply(Gesture::End(1), false, s);
    assert!(t.pending.is_none());
}

#[test]
fn a_run_of_gestures_at_once_is_one_edit() {
    // What a plug-in loading a preset by hand sends.
    let (mut t, s) = (Tracker::default(), Instant::now());
    for id in 0..50 {
        t.apply(Gesture::Begin(id), false, s);
        t.apply(Gesture::Perform(id, 0.5), false, s);
        t.apply(Gesture::End(id), false, s);
    }
    assert_eq!(t.take().unwrap().params.len(), 50);
}

#[test]
fn overlapping_gestures_end_with_the_last() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.apply(Gesture::Begin(1), false, s);
    t.apply(Gesture::Begin(2), false, s);
    t.apply(Gesture::Perform(1, 0.1), false, s);
    t.apply(Gesture::Perform(2, 0.1), false, s);
    t.apply(Gesture::End(1), false, s);
    assert!(!t.due(at(s, 1000), true));
    t.apply(Gesture::End(2), false, at(s, 1000));
    assert!(t.due(at(s, 1100), true));
}

#[test]
fn a_gesture_that_loses_its_end_still_finishes() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.apply(Gesture::Begin(1), false, s);
    t.apply(Gesture::Perform(1, 0.1), false, s);
    assert!(!t.due(s + STUCK, true));
    assert!(t.due(s + STUCK + Duration::from_millis(1), true));
}

#[test]
fn bare_edits_are_a_burst_until_they_pause() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    for i in 0..20 {
        t.apply(Gesture::Perform(1, i as f64), false, at(s, i * 50));
        assert!(!t.due(at(s, i * 50 + 100), true));
    }
    assert!(t.due(at(s, 19 * 50) + QUIET, true));
    let p = t.take().unwrap();
    assert_eq!((p.bracketed, p.params), (false, vec![1]));
}

#[test]
fn a_step_waits_for_a_processor_that_is_behind_but_not_forever() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.apply(Gesture::Begin(1), false, s);
    t.apply(Gesture::Perform(1, 0.1), false, s);
    t.apply(Gesture::End(1), false, s);
    assert!(!t.due(s + SETTLE, false));
    assert!(t.due(s + SETTLE + DRAIN_WAIT, false));
}

#[test]
fn a_gesture_ends_a_burst() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.apply(Gesture::Perform(1, 0.5), false, s);
    assert!(t.ends_burst(Gesture::Begin(2)));
    assert!(!t.ends_burst(Gesture::Perform(2, 0.5)));
}

#[test]
fn read_only_parameters_and_echoes_are_ignored() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.read_only.insert(9);
    t.apply(Gesture::Begin(9), false, s);
    t.apply(Gesture::Perform(9, 0.5), false, s);
    t.apply(Gesture::End(9), false, s);
    t.apply(Gesture::Perform(1, 0.5), true, s);
    assert!(t.pending.is_none());
    assert!(!t.ends_burst(Gesture::Begin(9)));
}

#[test]
fn repeating_a_value_is_not_an_edit() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.apply(Gesture::Perform(1, 0.5), false, s);
    t.take();
    t.apply(Gesture::Perform(1, 0.5), false, at(s, 1000));
    assert!(t.pending.is_none());
}

#[test]
fn a_parameter_that_never_stops_moving_makes_no_step_and_is_then_ignored() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    let mut ms = 0;
    while s + Duration::from_millis(ms) <= s + BURST_MAX + Duration::from_millis(100) {
        t.apply(Gesture::Perform(7, ms as f64), false, at(s, ms));
        ms += 33;
    }
    assert!(t.pending.is_none());
    assert!(t.own.contains(&7));
    t.apply(Gesture::Perform(7, 0.25), false, at(s, ms + 5000));
    assert!(t.pending.is_none() && t.drifted);
}

#[test]
fn steps_outside_gestures_are_capped() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    let mut made = 0;
    // A plug-in that moves itself every two seconds, changing its state each time.
    for i in 0..40u64 {
        let now = at(s, i * 2000);
        t.apply(Gesture::Perform(3, i as f64), false, now);
        if let Some(p) = t.take() {
            t.finished(&p, true);
            t.stepped(now);
            made += 1;
        }
    }
    assert_eq!(made, CAP_STEPS, "then it's taken to be moving itself");
    assert!(t.own.contains(&3));
    // A knob the user moves in a gesture is another matter.
    t.apply(Gesture::Begin(3), false, at(s, 90_000));
    t.apply(Gesture::Perform(3, 0.125), false, at(s, 90_000));
    t.apply(Gesture::End(3), false, at(s, 90_000));
    assert!(t.take().is_some());
}

#[test]
fn bursts_that_change_nothing_stop_being_read() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    for i in 0..DUD_BURSTS {
        t.apply(Gesture::Perform(4, i as f64), false, at(s, i as u64 * 1000));
        let p = t.take().unwrap();
        t.finished(&p, false);
    }
    t.apply(Gesture::Perform(4, 99.0), false, at(s, 9000));
    assert!(t.pending.is_none(), "an output parameter: no more calls to getState");
}

#[test]
fn restarts_join_what_is_pending_and_get_the_cap() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.restarted(s);
    t.restarted(at(s, 10));
    assert!(t.due(at(s, 10) + QUIET, true));
    assert!(t.take().unwrap().restarted);
    for _ in 0..CAP_STEPS {
        t.stepped(at(s, 20));
    }
    t.restarted(at(s, 30));
    assert!(t.pending.is_none() && t.drifted);
}

#[test]
fn a_restore_ends_everything_and_mutes_the_answers() {
    let (mut t, s) = (Tracker::default(), Instant::now());
    t.apply(Gesture::Begin(1), false, s);
    t.apply(Gesture::Perform(1, 0.2), false, s);
    t.restored(s);
    assert!(t.pending.is_none() && t.open.is_empty());
    assert!(t.muted(at(s, 100)));
    assert!(!t.muted(s + RESTORE_ECHO));
    // A knob still held goes on as bare edits.
    t.apply(Gesture::End(1), false, s + RESTORE_ECHO);
    assert!(t.pending.is_none());
}
