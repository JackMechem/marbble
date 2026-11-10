//! Controller assignments: reading messages, learning, scaling, takeover and buttons.

use std::time::{Duration, Instant};

use super::*;

#[test]
fn parses_cc_and_pitch_bend() {
    let m = parse(&[0xB3, 7, 127]).unwrap();
    assert_eq!((m.channel, m.kind, m.value, m.pressed), (3, CtlKind::Cc { number: 7 }, 1.0, true));
    let m = parse(&[0xB0, 1, 63]).unwrap();
    assert!(!m.pressed);
    let m = parse(&[0xE1, 0x7F, 0x7F]).unwrap();
    assert_eq!((m.channel, m.kind, m.value), (1, CtlKind::PitchBend, 1.0));
    assert_eq!(parse(&[0xE0, 0, 0x40]).unwrap().value, 8192.0 / 16383.0);
    // Aftertouch and channel mode messages (120..127) aren't controllers.
    assert!(parse(&[0xA0, 60, 10]).is_none());
    assert!(parse(&[0xD0, 60, 0]).is_none());
    assert!(parse(&[0xB0, 123, 0]).is_none());
    assert!(parse(&[0xB0]).is_none());
}

#[test]
fn parses_notes_and_program_changes() {
    let m = parse(&[0x92, 36, 127]).unwrap();
    assert_eq!((m.channel, m.kind, m.value, m.pressed), (2, CtlKind::Note { number: 36 }, 1.0, true));
    // Note off, and note on at velocity 0, release it.
    let m = parse(&[0x82, 36, 64]).unwrap();
    assert_eq!((m.kind, m.pressed), (CtlKind::Note { number: 36 }, false));
    assert!(!parse(&[0x92, 36, 0]).unwrap().pressed);
    let m = parse(&[0xC5, 9, 0]).unwrap();
    assert_eq!((m.channel, m.kind, m.pressed), (5, CtlKind::Program { number: 9 }, true));
    // A program change has two bytes.
    assert_eq!(parse(&[0xC0, 3]).unwrap().kind, CtlKind::Program { number: 3 });
    assert!(CtlKind::Note { number: 1 }.is_button() && CtlKind::Program { number: 1 }.is_button());
    assert!(!CtlKind::Cc { number: 1 }.is_button() && !CtlKind::PitchBend.is_button());
}

fn note(pressed: bool) -> CtlMsg {
    CtlMsg { channel: 0, kind: CtlKind::Note { number: 60 }, value: if pressed { 0.5 } else { 0.0 }, pressed, step: 0 }
}

#[test]
fn toggle_flips_on_each_press() {
    let mut f = Follow::default();
    assert_eq!(f.button(ButtonMode::Toggle, &note(true), false), Some(ButtonAct::Flip));
    assert_eq!(f.button(ButtonMode::Toggle, &note(true), false), None, "held");
    assert_eq!(f.button(ButtonMode::Toggle, &note(false), false), None);
    assert_eq!(f.button(ButtonMode::Toggle, &note(true), false), Some(ButtonAct::Flip));
}

#[test]
fn momentary_follows_the_button() {
    let mut f = Follow::default();
    assert_eq!(f.button(ButtonMode::Momentary, &note(true), false), Some(ButtonAct::Set(true)));
    assert_eq!(f.button(ButtonMode::Momentary, &note(false), false), Some(ButtonAct::Set(false)));
    assert_eq!(f.button(ButtonMode::Momentary, &note(false), false), None, "a second release");
}

#[test]
fn trigger_fires_once_per_press() {
    let mut f = Follow::default();
    assert_eq!(f.button(ButtonMode::Trigger, &note(true), false), Some(ButtonAct::Set(true)));
    assert_eq!(f.button(ButtonMode::Trigger, &note(true), false), None);
    assert_eq!(f.button(ButtonMode::Trigger, &note(false), false), None);
    assert_eq!(f.button(ButtonMode::Trigger, &note(true), false), Some(ButtonAct::Set(true)));
}

#[test]
fn program_changes_press_every_time() {
    let pc = CtlMsg { channel: 0, kind: CtlKind::Program { number: 4 }, value: 1.0, pressed: true, step: 0 };
    let mut f = Follow::default();
    assert_eq!(f.button(ButtonMode::Toggle, &pc, false), Some(ButtonAct::Flip));
    assert_eq!(f.button(ButtonMode::Toggle, &pc, false), Some(ButtonAct::Flip));
    assert_eq!(f.button(ButtonMode::Momentary, &pc, false), Some(ButtonAct::Set(true)));
}

#[test]
fn a_latching_button_presses_with_every_message() {
    // It sends 127 at one press and 0 at the next, with nothing in between.
    let cc = |v: u8| CtlMsg { channel: 0, kind: CtlKind::Cc { number: 20 }, value: v as f32 / 127.0, pressed: v > 63, step: 0 };
    let mut f = Follow::default();
    assert_eq!(f.button(ButtonMode::Toggle, &cc(127), true), Some(ButtonAct::Flip));
    assert_eq!(f.button(ButtonMode::Toggle, &cc(0), true), Some(ButtonAct::Flip));
    assert_eq!(f.button(ButtonMode::Trigger, &cc(127), true), Some(ButtonAct::Set(true)));
    assert_eq!(f.button(ButtonMode::Trigger, &cc(0), true), Some(ButtonAct::Set(true)));
    // Momentary follows the button's own state.
    assert_eq!(f.button(ButtonMode::Momentary, &cc(127), true), Some(ButtonAct::Set(true)));
    assert_eq!(f.button(ButtonMode::Momentary, &cc(0), true), Some(ButtonAct::Set(false)));
    // Not latching, the 0 is a release: Toggle acts every other message.
    let mut f = Follow::default();
    assert_eq!(f.button(ButtonMode::Toggle, &cc(127), false), Some(ButtonAct::Flip));
    assert_eq!(f.button(ButtonMode::Toggle, &cc(0), false), None);
}

#[test]
fn buttons_on_a_continuous_parameter() {
    let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Note { number: 60 } };
    let mut a = Assignment::new(c, CtlTarget::MasterVolume);
    assert_eq!(a.button_mode, ButtonMode::Trigger, "a pad on a fader sets it");
    a.min = 0.2;
    assert!((a.press_value(&note(true)) - 0.6).abs() < 1e-6, "the velocity's place in the range");
    a.velocity = false;
    assert_eq!(a.press_value(&note(true)), 1.0);
    let mut f = Follow::default();
    assert_eq!(f.button_value(ButtonAct::Flip, 0.9, 0.2), 0.9);
    assert_eq!(f.button_value(ButtonAct::Flip, 0.9, 0.2), 0.2);
    assert_eq!(f.button_value(ButtonAct::Set(true), 0.9, 0.2), 0.9);
    assert_eq!(f.button_value(ButtonAct::Set(false), 0.9, 0.2), 0.2);
    // A command fires; a note on a switch toggles it, as a control change does.
    let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Cc { number: 20 } };
    assert_eq!(Assignment::new(c, CtlTarget::Command { id: "undo".into() }).button_mode, ButtonMode::Trigger);
    let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Note { number: 60 } };
    assert_eq!(Assignment::new(c, CtlTarget::Mute { track: 1 }).button_mode, ButtonMode::Toggle);
}

#[test]
fn port_key_drops_alsa_numbers() {
    assert_eq!(port_key("nanoKONTROL2:nanoKONTROL2 MIDI 1 20:0"), "nanoKONTROL2:nanoKONTROL2 MIDI 1");
    assert_eq!(port_key("nanoKONTROL2:nanoKONTROL2 MIDI 1 24:0"), "nanoKONTROL2:nanoKONTROL2 MIDI 1");
    assert_eq!(port_key("Launch Control XL"), "Launch Control XL");
    assert_eq!(port_key("Device 3:a"), "Device 3:a");
    assert_eq!(port_key("12:0"), "12:0");
}

#[test]
fn switched_off_ports_are_kept_by_key() {
    // An entry from before keys, with the numbers the port had then, and the same device's key.
    let saved = vec!["nanoKONTROL2:nanoKONTROL2 MIDI 1 20:0".to_string(), "nanoKONTROL2:nanoKONTROL2 MIDI 1".to_string(), "Launch Control XL".to_string()];
    let keys = port_keys(&saved);
    assert_eq!(keys, ["nanoKONTROL2:nanoKONTROL2 MIDI 1", "Launch Control XL"]);
    // Replugged, the device comes back under new numbers and is still off.
    assert!(keys.iter().any(|k| k == port_key("nanoKONTROL2:nanoKONTROL2 MIDI 1 24:0")));
}

#[test]
fn fader_taper_round_trips() {
    for db in [-90.0f32, -60.0, -40.0, -24.0, -12.0, -6.0, 0.0, 3.0, 6.0] {
        assert!((fader_to_db(db_to_fader(db)) - db).abs() < 1e-3, "{db}");
    }
    assert_eq!(db_to_fader(-200.0), 0.0);
    assert_eq!(fader_to_db(0.0), -96.0);
    assert_eq!(fader_to_db(1.0), 6.0);
    // 0 dB sits about three quarters of the way up, as on Logic's fader.
    let unity = db_to_fader(0.0);
    assert!(unity > 0.85 && unity < 0.9, "{unity}");
}

#[test]
fn old_eq_assignments_keep_their_travel() {
    // The fixed Channel EQ's assignments mapped 0..1 to 20 Hz..20 kHz and Q 0.1..18 on log
    // scales and gain to ±24 dB; the Channel EQ insert's parameters they now drive do too.
    use crate::fx::channel_eq as ceq;
    let info = ceq::info();
    let plain = |id: u32, n: f32| crate::dsp::to_plain(info.param(id).unwrap(), n);
    for n in [0.0f32, 0.3, 0.5, 1.0] {
        assert!((plain(ceq::freq(2), n) - 20.0 * 1000f32.powf(n)).abs() < 0.01 * 20.0 * 1000f32.powf(n));
        assert!((plain(ceq::gain(2), n) - (-24.0 + 48.0 * n)).abs() < 1e-4);
        assert!((plain(ceq::q(2), n) - 0.1 * 180f32.powf(n)).abs() < 1e-3 * 180f32.powf(n));
        assert!((plain(ceq::OUTPUT, n) - (-24.0 + 48.0 * n)).abs() < 1e-4);
    }
    assert_eq!(plain(ceq::on(3), 0.7), 1.0);
}

#[test]
fn range_scales_and_inverts() {
    let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Cc { number: 1 } };
    let mut a = Assignment::new(c, CtlTarget::MasterVolume);
    assert_eq!(a.scale(0.5), 0.5);
    a.min = 0.25;
    a.max = 0.75;
    assert_eq!((a.scale(0.0), a.scale(1.0)), (0.25, 0.75));
    a.min = 1.0;
    a.max = 0.0;
    assert_eq!((a.scale(0.0), a.scale(1.0)), (1.0, 0.0));
}

#[test]
fn jump_always_moves() {
    let mut f = Follow::default();
    assert!(f.take(Takeover::Jump, 0.1, 0.8));
    assert!(f.take(Takeover::Jump, 0.9, 0.2));
}

#[test]
fn pickup_waits_for_the_crossing() {
    let mut f = Follow::default();
    // Parameter at 0.6; the controller starts low and comes up.
    assert!(!f.take(Takeover::Pickup, 0.1, 0.6));
    assert!(!f.take(Takeover::Pickup, 0.3, 0.6));
    assert!(f.take(Takeover::Pickup, 0.7, 0.6), "passing the value catches it");
    f.applied(0.7);
    assert!(f.take(Takeover::Pickup, 0.2, 0.7), "caught, it follows anywhere");
    f.applied(0.2);
    // The mouse moves the parameter away: it has to be caught again.
    assert!(!f.take(Takeover::Pickup, 0.25, 0.9));
    assert!(f.take(Takeover::Pickup, 0.9, 0.9), "landing on it catches it");
}

#[test]
fn pickup_catches_within_a_step() {
    let mut f = Follow::default();
    assert!(f.take(Takeover::Pickup, 0.5 + 1.0 / 127.0, 0.5));
}

#[test]
fn pickup_follows_a_stepped_parameter() {
    let mut f = Follow::default();
    assert!(f.take(Takeover::Pickup, 0.48, 0.48));
    // A choice parameter lands on its nearest step; that isn't someone else's move.
    f.applied(0.5);
    assert!(f.take(Takeover::Pickup, 0.41, 0.5));
}

#[test]
fn finds_by_port_channel_and_controller() {
    let c = |port: &str, channel, number| Controller { port: port.into(), channel, kind: CtlKind::Cc { number } };
    let list = vec![
        Assignment::new(c("A", 0, 7), CtlTarget::MasterVolume),
        Assignment::new(c("B", 0, 7), CtlTarget::MasterVolume),
        Assignment::new(c("A", 1, 7), CtlTarget::MasterVolume),
    ];
    let m = |ch, n| CtlMsg { channel: ch, kind: CtlKind::Cc { number: n }, value: 0.0, pressed: false, step: 0 };
    assert_eq!(find(&list, "A", &m(0, 7)), Some(0));
    assert_eq!(find(&list, "B", &m(0, 7)), Some(1));
    assert_eq!(find(&list, "A", &m(1, 7)), Some(2));
    assert_eq!(find(&list, "A", &m(2, 7)), None);
    assert_eq!(find(&list, "A", &m(0, 8)), None);
}

#[test]
fn assignment_file_fills_defaults() {
    let json = r#"{"assignments":[{"controller":{"port":"P","channel":0,"kind":{"type":"cc","number":7}},"target":{"type":"volume","track":3}}]}"#;
    let f: AssignmentFile = serde_json::from_str(json).unwrap();
    let a = &f.assignments[0];
    assert_eq!((a.mode, a.min, a.max, a.button_mode, a.velocity, a.latching, a.feedback), (Takeover::Jump, 0.0, 1.0, ButtonMode::Toggle, true, false, true));
    assert_eq!((a.encoding, a.sensitivity), (Encoding::Absolute, 1.0));
    assert_eq!(a.target, CtlTarget::Volume { track: 3 });
    let back: AssignmentFile = serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap();
    assert_eq!(back.assignments, f.assignments);
}

#[test]
fn relative_encodings() {
    use Encoding::*;
    assert_eq!(Absolute.steps(1), None);
    assert_eq!(
        (TwosComplement.steps(1), TwosComplement.steps(63), TwosComplement.steps(127), TwosComplement.steps(64)),
        (Some(1), Some(63), Some(-1), Some(-64))
    );
    assert_eq!((SignMagnitude.steps(1), SignMagnitude.steps(65), SignMagnitude.steps(127), SignMagnitude.steps(64)), (Some(1), Some(-1), Some(-63), Some(0)));
    assert_eq!((BinaryOffset.steps(65), BinaryOffset.steps(63), BinaryOffset.steps(64), BinaryOffset.steps(0)), (Some(1), Some(-1), Some(0), Some(-64)));
    let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Cc { number: 16 } };
    let mut a = Assignment::new(c, CtlTarget::MasterVolume);
    let msg = |v: u8| CtlMsg { channel: 0, kind: CtlKind::Cc { number: 16 }, value: v as f32 / 127.0, pressed: v > 63, step: 0 };
    assert_eq!(a.relative_steps(&msg(1)), None, "absolute");
    a.encoding = BinaryOffset;
    assert_eq!(a.relative_steps(&msg(66)), Some(2));
    let pb = CtlMsg { kind: CtlKind::PitchBend, ..msg(66) };
    assert_eq!(a.relative_steps(&pb), None, "only control changes are encoders");
}

#[test]
fn relative_moves_follow_the_range_and_sensitivity() {
    let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Cc { number: 16 } };
    let mut a = Assignment::new(c, CtlTarget::MasterVolume);
    a.encoding = Encoding::TwosComplement;
    let mut f = Follow::default();
    let v = f.relative(&a, 10, 0.5).unwrap();
    assert!((v - (0.5 + 10.0 / 127.0)).abs() < 1e-6);
    f.applied(v);
    a.sensitivity = 2.0;
    let w = f.relative(&a, -5, v).unwrap();
    assert!((w - 0.5).abs() < 1e-6, "{w}");
    // It stops at the ends.
    assert_eq!(f.relative(&a, 500, w), Some(1.0));
    // A stepped parameter that rounded the move back still gets there.
    let mut f = Follow::default();
    a.sensitivity = 1.0;
    let first = f.relative(&a, 3, 0.5).unwrap();
    f.applied(0.5);
    let second = f.relative(&a, 3, 0.5).unwrap();
    assert!(second > first);
    // The range: the encoder sweeps only it, and min above max turns it round.
    a.min = 0.8;
    a.max = 0.2;
    let mut f = Follow::default();
    let v = f.relative(&a, 127, 0.5).unwrap();
    assert!((v - 0.2).abs() < 1e-6, "{v}");
    a.max = 0.8;
    assert_eq!(Follow::default().relative(&a, 1, 0.5), None, "an empty range");
}

fn cc(channel: u8, number: u8, v: u8) -> CtlMsg {
    CtlMsg { channel, kind: CtlKind::Cc { number }, value: v as f32 / 127.0, pressed: v > 63, step: 0 }
}

#[test]
fn reads_14_bit_pairs() {
    let mut h = HiRes::default();
    let pair = |n: u8| n == 7;
    let m = h.read("P", cc(0, 7, 64), pair, false).unwrap();
    assert_eq!((m.kind, m.value), (CtlKind::Cc14 { number: 7 }, 64.0 / 127.0));
    let m = h.read("P", cc(0, 39, 0), pair, false).unwrap();
    assert_eq!((m.kind, m.value), (CtlKind::Cc14 { number: 7 }, 8192.0 / 16383.0));
    let m = h.read("P", cc(0, 39, 127), pair, false).unwrap();
    assert_eq!(m.value, 8319.0 / 16383.0);
    // Other channels keep their own coarse part; a fine part without one waits.
    assert!(h.read("P", cc(1, 39, 5), pair, false).is_none());
    assert!(h.read("Q", cc(0, 39, 5), pair, false).is_none());
    // Unpaired controllers pass through.
    assert_eq!(h.read("P", cc(0, 8, 5), pair, false).unwrap().kind, CtlKind::Cc { number: 8 });
    assert_eq!(h.read("P", cc(0, 40, 5), pair, false).unwrap().kind, CtlKind::Cc { number: 40 });
}

#[test]
fn reads_nrpns() {
    let mut h = HiRes::default();
    let none = |_: u8| false;
    assert!(h.read("P", cc(2, 99, 3), none, true).is_none());
    assert!(h.read("P", cc(2, 98, 5), none, true).is_none());
    let m = h.read("P", cc(2, 6, 127), none, true).unwrap();
    assert_eq!((m.kind, m.value), (CtlKind::Nrpn { number: 3 << 7 | 5 }, 1.0));
    let m = h.read("P", cc(2, 38, 127), none, true).unwrap();
    assert_eq!((m.kind, m.value), (CtlKind::Nrpn { number: 389 }, 1.0));
    let m = h.read("P", cc(2, 38, 0), none, true).unwrap();
    assert_eq!(m.value, 16256.0 / 16383.0);
    // An RPN takes data entry over, and the null number lets go: data entry is then an
    // ordinary controller.
    assert!(h.read("P", cc(2, 101, 0), none, true).is_none());
    assert_eq!(h.read("P", cc(2, 6, 1), none, true).unwrap().kind, CtlKind::Cc { number: 6 });
    h.read("P", cc(2, 99, 127), none, true);
    h.read("P", cc(2, 98, 127), none, true);
    assert_eq!(h.read("P", cc(2, 6, 1), none, true).unwrap().kind, CtlKind::Cc { number: 6 });
    // Data increment and decrement step the chosen NRPN, whatever their value byte.
    h.read("P", cc(2, 99, 3), none, true);
    h.read("P", cc(2, 98, 5), none, true);
    let up = h.read("P", cc(2, 96, 0), none, true).unwrap();
    let down = h.read("P", cc(2, 97, 9), none, true).unwrap();
    assert_eq!((up.kind, up.step, down.kind, down.step), (CtlKind::Nrpn { number: 389 }, 1, CtlKind::Nrpn { number: 389 }, -1));
    let a = Assignment::new(Controller { port: "P".into(), channel: 2, kind: CtlKind::Nrpn { number: 389 } }, CtlTarget::MasterVolume);
    assert_eq!((a.relative_steps(&up), a.relative_steps(&down)), (Some(1), Some(-1)));
    assert_eq!(a.relative_steps(&h.read("P", cc(2, 6, 64), none, true).unwrap()), None, "data entry sets it");
    // Without NRPNs in use, these are ordinary controllers.
    assert_eq!(h.read("P", cc(2, 99, 1), none, false).unwrap().kind, CtlKind::Cc { number: 99 });
    assert_eq!(h.read("P", cc(2, 96, 1), none, false).unwrap().step, 0);
    // What an NRPN assignment takes off the live path.
    let n = CtlKind::Nrpn { number: 389 };
    assert!(n.takes(CtlKind::Cc { number: 99 }) && n.takes(CtlKind::Cc { number: 38 }) && !n.takes(CtlKind::Cc { number: 7 }));
    let p = CtlKind::Cc14 { number: 7 };
    assert!(p.takes(CtlKind::Cc { number: 7 }) && p.takes(CtlKind::Cc { number: 39 }) && !p.takes(CtlKind::Cc { number: 8 }));
    assert!(CtlKind::PitchBend.takes(CtlKind::PitchBend));
}

#[test]
fn learns_what_it_hears() {
    let t0 = Instant::now();
    let later = t0 + Duration::from_millis(200);
    // A 14-bit pair: the coarse part waits for its fine part.
    let mut l = Learner::default();
    assert!(l.feed("P", cc(0, 1, 64), t0).is_none());
    assert_eq!(l.feed("P", cc(0, 33, 9), t0).unwrap().kind, CtlKind::Cc14 { number: 1 });
    // A 7-bit controller in 0..31: once the wait is over.
    let mut l = Learner::default();
    assert!(l.feed("P", cc(3, 1, 64), t0).is_none());
    assert!(l.poll(t0).is_none());
    let c = l.poll(later).unwrap();
    assert_eq!((c.kind, c.channel), (CtlKind::Cc { number: 1 }, 3));
    // ...or as soon as something else comes.
    let mut l = Learner::default();
    l.feed("P", cc(0, 1, 64), t0);
    assert_eq!(l.feed("P", cc(0, 2, 64), t0).unwrap().kind, CtlKind::Cc { number: 1 });
    // Above 31, at once.
    assert_eq!(Learner::default().feed("P", cc(0, 74, 1), t0).unwrap().kind, CtlKind::Cc { number: 74 });
    // An NRPN.
    let mut l = Learner::default();
    assert!(l.feed("P", cc(0, 99, 0), t0).is_none());
    assert!(l.feed("P", cc(0, 98, 20), t0).is_none());
    assert_eq!(l.feed("P", cc(0, 6, 64), t0).unwrap().kind, CtlKind::Nrpn { number: 20 });
    // A note's release isn't learnt; its press is.
    let mut l = Learner::default();
    assert!(l.feed("P", note(false), t0).is_none());
    assert_eq!(l.feed("P", note(true), t0).unwrap().kind, CtlKind::Note { number: 60 });
}

#[test]
fn selected_track_targets_resolve() {
    assert_eq!(SelectedParam::Send { slot: 1 }.on(4), Some(CtlTarget::Send { track: 4, slot: 1 }));
    assert_eq!(SelectedParam::Monitor.on(4), Some(CtlTarget::Monitor { track: 4 }));
    assert_eq!(SelectedParam::PluginParam { index: 0 }.on(4), None);
    assert_eq!(SelectedParam::Bypass { slot: 2 }.name(), "Insert 3 Bypass");
    assert!(CtlTarget::Selected { param: SelectedParam::Solo }.is_button());
    assert!(!CtlTarget::Selected { param: SelectedParam::Pan }.is_button());
    let json = r#"{"type":"selected","param":{"type":"plugin_param","index":3}}"#;
    let t: CtlTarget = serde_json::from_str(json).unwrap();
    assert_eq!(t, CtlTarget::Selected { param: SelectedParam::PluginParam { index: 3 } });
}

#[test]
fn commands_are_saved_by_id() {
    let t = CtlTarget::Command { id: "next-marker".into() };
    assert_eq!(serde_json::to_string(&t).unwrap(), r#"{"type":"command","id":"next-marker"}"#);
    assert!(t.is_button() && t.track().is_none());
}
