//! Controller feedback: scaling, 14-bit and relative controllers, switches and lights, throttling.

use super::*;
use crate::control::controllers::{Controller, CtlTarget};

fn assignment(kind: CtlKind, channel: u8) -> Assignment {
    Assignment::new(Controller { port: "P".into(), channel, kind }, CtlTarget::MasterVolume, false)
}

#[test]
fn cc_carries_the_travel() {
    let a = assignment(CtlKind::Cc { number: 7 }, 2);
    assert_eq!(message(&a, Shown::Travel(1.0)), Some(vec![[0xB2, 7, 127]]));
    assert_eq!(message(&a, Shown::Travel(0.0)), Some(vec![[0xB2, 7, 0]]));
    assert_eq!(message(&a, Shown::Travel(0.5)), Some(vec![[0xB2, 7, 64]]));
    assert_eq!(message(&a, Shown::Travel(f32::NAN)), Some(vec![[0xB2, 7, 0]]));
}

#[test]
fn feedback_inverts_the_input_scaling() {
    // A controller value goes in through the range and back out unchanged, for every value.
    let mut a = assignment(CtlKind::Cc { number: 1 }, 0);
    for (min, max) in [(0.0, 1.0), (0.25, 0.75), (1.0, 0.0), (0.1, 0.9)] {
        a.min = min;
        a.max = max;
        for v in 0..=127u8 {
            let travel = a.scale(v as f32 / 127.0);
            assert_eq!(message(&a, Shown::Travel(travel)), Some(vec![[0xB0, 1, v]]), "{min}..{max} at {v}");
        }
    }
}

#[test]
fn outside_the_range_pins_to_an_end() {
    let mut a = assignment(CtlKind::Cc { number: 1 }, 0);
    a.min = 0.25;
    a.max = 0.75;
    assert_eq!(message(&a, Shown::Travel(0.1)), Some(vec![[0xB0, 1, 0]]));
    assert_eq!(message(&a, Shown::Travel(0.9)), Some(vec![[0xB0, 1, 127]]));
    // A range of one point shows the bottom.
    a.max = 0.25;
    assert_eq!(message(&a, Shown::Travel(0.9)), Some(vec![[0xB0, 1, 0]]));
}

#[test]
fn level_feedback_follows_the_fader_taper() {
    use crate::control::controllers::{db_to_fader, fader_to_db};
    let a = assignment(CtlKind::Cc { number: 7 }, 0);
    // 0 dB sits where CC 111 puts the fader, as on screen.
    let unity = message(&a, Shown::Travel(db_to_fader(0.0))).unwrap()[0][2];
    assert!((fader_to_db(unity as f32 / 127.0)).abs() < 0.5, "{unity}");
    assert_eq!(message(&a, Shown::Travel(db_to_fader(6.0))), Some(vec![[0xB0, 7, 127]]));
    assert_eq!(message(&a, Shown::Travel(db_to_fader(-120.0))), Some(vec![[0xB0, 7, 0]]));
}

#[test]
fn pitch_bend_is_fourteen_bits() {
    let a = assignment(CtlKind::PitchBend, 1);
    assert_eq!(message(&a, Shown::Travel(1.0)), Some(vec![[0xE1, 0x7F, 0x7F]]));
    assert_eq!(message(&a, Shown::Travel(0.0)), Some(vec![[0xE1, 0, 0]]));
    // The centre is 8192 (0x2000): LSB 0, MSB 0x40.
    assert_eq!(message(&a, Shown::Travel(8192.0 / 16383.0)), Some(vec![[0xE1, 0, 0x40]]));
    // It reads back as the value it was made from.
    let m = message(&a, Shown::Travel(0.3)).unwrap()[0];
    assert!((crate::control::controllers::parse(&m).unwrap().value - 0.3).abs() < 1.0 / 16383.0);
}

#[test]
fn a_pad_is_lit_by_a_note() {
    let a = assignment(CtlKind::Note { number: 36 }, 9);
    assert_eq!(message(&a, Shown::Switch(true)), Some(vec![[0x99, 36, 127]]));
    assert_eq!(message(&a, Shown::Switch(false)), Some(vec![[0x99, 36, 0]]));
    assert_eq!(message(&a, Shown::Travel(0.5)), Some(vec![[0x99, 36, 64]]));
    assert_eq!(message(&assignment(CtlKind::Program { number: 3 }, 0), Shown::Switch(true)), None);
}

#[test]
fn high_resolution_controllers_get_14_bits() {
    let a = assignment(CtlKind::Cc14 { number: 7 }, 1);
    assert_eq!(message(&a, Shown::Travel(1.0)), Some(vec![[0xB1, 7, 127], [0xB1, 39, 127]]));
    assert_eq!(message(&a, Shown::Travel(8192.0 / 16383.0)), Some(vec![[0xB1, 7, 64], [0xB1, 39, 0]]));
    let a = assignment(CtlKind::Nrpn { number: 130 }, 0);
    assert_eq!(message(&a, Shown::Travel(1.0)), Some(vec![[0xB0, 99, 1], [0xB0, 98, 2], [0xB0, 6, 127], [0xB0, 38, 127]]));
    // They read back as the value they were made from.
    let mut h = crate::control::controllers::HiRes::default();
    let a = assignment(CtlKind::Cc14 { number: 7 }, 0);
    let got = message(&a, Shown::Travel(0.3))
        .unwrap()
        .iter()
        .filter_map(|m| h.read("P", crate::control::controllers::parse(m).unwrap(), |n| n == 7, false))
        .last()
        .unwrap();
    assert!((got.value - 0.3).abs() < 1.0 / 16383.0);
}

#[test]
fn relative_encoders_get_no_feedback() {
    let mut a = assignment(CtlKind::Cc { number: 16 }, 0);
    a.encoding = Encoding::TwosComplement;
    assert_eq!(message(&a, Shown::Travel(0.5)), None);
}

#[test]
fn switches_are_full_on_or_off() {
    let mut a = assignment(CtlKind::Cc { number: 30 }, 0);
    // The range is a fader's business, not a button's.
    a.min = 0.25;
    a.max = 0.5;
    assert_eq!(message(&a, Shown::Switch(true)), Some(vec![[0xB0, 30, 127]]));
    assert_eq!(message(&a, Shown::Switch(false)), Some(vec![[0xB0, 30, 0]]));
    let a = assignment(CtlKind::PitchBend, 0);
    assert_eq!(message(&a, Shown::Switch(true)), Some(vec![[0xE0, 0x7F, 0x7F]]));
}

#[test]
fn throttle_sends_changes_only() {
    let t0 = Instant::now();
    let mut t = Throttle::default();
    assert_eq!(t.due(vec![[0xB0, 7, 10]], t0), Some(vec![[0xB0, 7, 10]]), "the first value always goes");
    let t1 = t0 + Duration::from_millis(100);
    assert_eq!(t.due(vec![[0xB0, 7, 10]], t1), None, "unchanged");
    assert_eq!(t.due(vec![[0xB0, 7, 11]], t1), Some(vec![[0xB0, 7, 11]]));
    t.forget();
    let t2 = t1 + Duration::from_millis(100);
    assert_eq!(t.due(vec![[0xB0, 7, 11]], t2), Some(vec![[0xB0, 7, 11]]), "sent again once forgotten");
}

#[test]
fn throttle_limits_the_rate_but_keeps_the_last_value() {
    let t0 = Instant::now();
    let mut t = Throttle::default();
    let mut sent = Vec::new();
    // A value changing every millisecond for half a second, asked every millisecond.
    for ms in 0..500u64 {
        if let Some(m) = t.due(vec![[0xB0, 7, (ms / 4) as u8]], t0 + Duration::from_millis(ms)) {
            sent.push((ms, m[0][2]));
        }
    }
    assert!(sent.len() <= 500 / MIN_GAP.as_millis() as usize + 1, "{} sent", sent.len());
    assert!(sent.windows(2).all(|w| w[1].0 - w[0].0 >= MIN_GAP.as_millis() as u64));
    // The value stops changing; the next ask after the gap sends the final one.
    let last = (499 / 4) as u8;
    let after = t0 + Duration::from_millis(500) + MIN_GAP;
    if sent.last().map(|s| s.1) != Some(last) {
        assert_eq!(t.due(vec![[0xB0, 7, last]], after), Some(vec![[0xB0, 7, last]]));
    }
    assert_eq!(t.due(vec![[0xB0, 7, last]], after + MIN_GAP), None);
}

#[test]
fn throttle_keeps_quiet_while_the_controller_moves() {
    let t0 = Instant::now();
    let mut t = Throttle::default();
    t.heard([0xB0, 7, 90], t0);
    // The parameter lands a little elsewhere (a stepped value): not sent back while moving.
    assert_eq!(t.due(vec![[0xB0, 7, 88]], t0 + Duration::from_millis(100)), None);
    assert_eq!(t.due(vec![[0xB0, 7, 88]], t0 + QUIET - Duration::from_millis(1)), None);
    // After it stops, the controller is told where the parameter is.
    assert_eq!(t.due(vec![[0xB0, 7, 88]], t0 + QUIET), Some(vec![[0xB0, 7, 88]]));
    // Where the parameter is what the controller sent, nothing goes back at all.
    t.heard([0xB0, 7, 40], t0 + Duration::from_secs(2));
    assert_eq!(t.due(vec![[0xB0, 7, 40]], t0 + Duration::from_secs(3)), None);
}

#[test]
fn a_button_light_is_set_on_release() {
    let t0 = Instant::now();
    let mut t = Throttle::default();
    t.due(vec![[0xB0, 30, 0]], t0);
    // A press switches Mute on: its light is lit once the button is let go, even though the
    // button itself sent 127 (its own light isn't lit by the press).
    t.button(false, t0 + Duration::from_secs(1));
    assert_eq!(t.due(vec![[0xB0, 30, 127]], t0 + Duration::from_secs(1)), None, "held");
    t.button(true, t0 + Duration::from_millis(1080));
    assert_eq!(t.due(vec![[0xB0, 30, 127]], t0 + Duration::from_millis(1080)), Some(vec![[0xB0, 30, 127]]));
}

#[test]
fn a_note_off_reads_as_velocity_0() {
    let t0 = Instant::now();
    let mut t = Throttle::default();
    t.heard([0x81, 36, 64], t0);
    assert_eq!(t.due(vec![[0x91, 36, 0]], t0 + QUIET), None);
}

#[test]
fn a_pad_light_follows_its_switch_both_ways() {
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let mut t = Throttle::default();
    assert_eq!(t.due(vec![[0x90, 36, 0]], t0), Some(vec![[0x90, 36, 0]]), "paired: out");
    // Press 1 switches Mute on: nothing while held, lit on release.
    t.button(false, ms(100));
    assert_eq!(t.due(vec![[0x90, 36, 127]], ms(150)), None);
    t.button(true, ms(220));
    assert_eq!(t.due(vec![[0x90, 36, 127]], ms(220)), Some(vec![[0x90, 36, 127]]));
    // Press 2 switches it off. The pad's own note off isn't what its light shows: put out.
    t.button(false, ms(1000));
    t.button(true, ms(1120));
    assert_eq!(t.due(vec![[0x90, 36, 0]], ms(1120)), Some(vec![[0x90, 36, 0]]));
    assert_eq!(t.due(vec![[0x90, 36, 0]], ms(1200)), None, "once");
}

#[test]
fn device_names() {
    assert_eq!(device_name("nanoKONTROL2:nanoKONTROL2 MIDI 1"), "nanoKONTROL2");
    assert_eq!(device_name("MIDIIN2 (nanoKONTROL2)"), "nanoKONTROL2");
    assert_eq!(device_name("MIDIOUT2 (nanoKONTROL2)"), "nanoKONTROL2");
    assert_eq!(device_name("Launch Control XL"), "Launch Control XL");
}

#[test]
fn pairs_by_port_then_device() {
    let outs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let o = outs(&["nanoKONTROL2:nanoKONTROL2 MIDI 1", "Launchpad:Launchpad MIDI 1", "Launchpad:Launchpad MIDI 2", "MIDIOUT2 (X-Touch)"]);
    assert_eq!(auto_output("nanoKONTROL2:nanoKONTROL2 MIDI 1", &o), Some("nanoKONTROL2:nanoKONTROL2 MIDI 1"));
    assert_eq!(auto_output("Launchpad:Launchpad MIDI 2", &o), Some("Launchpad:Launchpad MIDI 2"));
    // Two outputs of the device and neither has the input's name: no guess.
    assert_eq!(auto_output("Launchpad:Launchpad In", &o), None);
    assert_eq!(auto_output("MIDIIN2 (X-Touch)", &o), Some("MIDIOUT2 (X-Touch)"));
    assert_eq!(auto_output("Keystation 49", &o), None);
    // A choice wins; an empty one is none; a choice that isn't connected sends nowhere.
    assert_eq!(paired_output("Keystation 49", Some("Launchpad:Launchpad MIDI 1"), &o), Some("Launchpad:Launchpad MIDI 1"));
    assert_eq!(paired_output("nanoKONTROL2:nanoKONTROL2 MIDI 1", Some(""), &o), None);
    assert_eq!(paired_output("nanoKONTROL2:nanoKONTROL2 MIDI 1", Some("Gone"), &o), None);
    assert_eq!(paired_output("nanoKONTROL2:nanoKONTROL2 MIDI 1", None, &o), Some("nanoKONTROL2:nanoKONTROL2 MIDI 1"));
}
