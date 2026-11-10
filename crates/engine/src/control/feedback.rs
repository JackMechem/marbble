//! Controller feedback, as Logic sends it to a control surface: each assignment's target value
//! goes back to the controller it was learnt from, so button lights and motor faders show the
//! DAW's state after a mouse move, an undo, automation or opening a project.
//!
//! This module is the pure part: the message that shows a value on a controller (the inverse of
//! how its moves are applied), which output port a controller's input is paired with, and the
//! per-controller [`Throttle`] that limits the rate and keeps quiet while the controller itself
//! is moving. The session reads the values and decides what to send on the main thread
//! (`session/control/feedback.rs`); `midi/output.rs` sends it from a thread of its own.

use std::time::{Duration, Instant};

use crate::control::controllers::{Assignment, CtlKind, Encoding};

/// The least time between two messages to one controller: at most 40 a second (the 30 ms idle
/// tick makes it about 33), always with the latest value.
pub const MIN_GAP: Duration = Duration::from_millis(25);

/// How long after a controller's last move feedback to it waits, so a motor fader isn't driven
/// against the hand moving it. The same half second after which a controller lets go of its
/// parameter (`session/control/controllers/`).
pub const QUIET: Duration = Duration::from_millis(500);

/// The messages that show one value on a controller (one, or two for a 14-bit controller, four
/// for an NRPN).
pub type Msgs = Vec<[u8; 3]>;

/// What a controller should show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown {
    /// A switch (Mute, Solo, Record, a transport button): full on or off, for a button's light.
    Switch(bool),
    /// A position on the target's own 0..1 travel (the fader's taper for levels).
    Travel(f32),
}

/// The controller value (0..1) that puts the parameter at `travel`: the inverse of
/// [`Assignment::scale`], so a controller whose range is a stretch of the travel shows where the
/// parameter is inside it (pinned to an end outside it).
pub fn unscale(a: &Assignment, travel: f32) -> f32 {
    let (lo, hi) = (a.min.clamp(0.0, 1.0), a.max.clamp(0.0, 1.0));
    if (hi - lo).abs() < 1e-6 {
        return 0.0;
    }
    ((travel - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// The messages that show `value` on the assignment's controller, as the same kind of message
/// the controller sends (two for a 14-bit controller, four for an NRPN). None for a controller
/// that has nothing to show: a program change, or a relative encoder, whose value isn't a
/// position.
pub fn message(a: &Assignment, value: Shown) -> Option<Msgs> {
    if a.encoding != Encoding::Absolute {
        return None;
    }
    let v = match value {
        Shown::Switch(on) => on as u8 as f32,
        Shown::Travel(t) => unscale(a, t),
    };
    encode(a.controller.kind, a.controller.channel, v)
}

/// A controller value (0..1) as that controller's messages. A new kind of controller needs its
/// arm here.
pub fn encode(kind: CtlKind, channel: u8, v: f32) -> Option<Msgs> {
    let ch = channel & 0x0F;
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let fine = (v * 16383.0).round() as u16;
    let (msb, lsb) = ((fine >> 7) as u8, (fine & 0x7F) as u8);
    let one = |m: [u8; 3]| Some(vec![m]);
    match kind {
        CtlKind::Cc { number } => one([0xB0 | ch, number & 0x7F, (v * 127.0).round() as u8]),
        CtlKind::PitchBend => one([0xE0 | ch, lsb, msb]),
        // The coarse part, then the fine part, as the controller sends them.
        CtlKind::Cc14 { number } => Some(vec![[0xB0 | ch, number & 0x1F, msb], [0xB0 | ch, (number & 0x1F) + 32, lsb]]),
        // The number (CC 99, 98), then the value by data entry (CC 6, 38).
        CtlKind::Nrpn { number } => {
            Some(vec![[0xB0 | ch, 99, (number >> 7) as u8 & 0x7F], [0xB0 | ch, 98, (number & 0x7F) as u8], [0xB0 | ch, 6, msb], [0xB0 | ch, 38, lsb]])
        }
        // A pad's light: a note on, velocity 127 for on and 0 for off (a level in between for a
        // pad on a fader, which pads with colours or brightness show).
        CtlKind::Note { number } => one([0x90 | ch, number & 0x7F, (v * 127.0).round() as u8]),
        // A program change has no state to show.
        CtlKind::Program { .. } => None,
    }
}

/// A message as a controller shows it: a note off is a note on with velocity 0, which is how
/// feedback switches a note's light off.
fn canonical(d: [u8; 3]) -> [u8; 3] {
    if d[0] & 0xF0 == 0x80 {
        [0x90 | (d[0] & 0x0F), d[1], 0]
    } else {
        d
    }
}

/// What one controller shows and when it was last sent to or moved.
#[derive(Debug, Clone, Default)]
pub struct Throttle {
    /// The last message sent to the controller or received from it: what it shows now. None
    /// when that isn't known (a new assignment, an output that was just connected).
    shown: Option<Msgs>,
    sent_at: Option<Instant>,
    /// Feedback waits until then: the controller is being moved.
    quiet_until: Option<Instant>,
}

impl Throttle {
    /// The controller (a fader or knob) sent `data` at `now`: it shows that, and is being moved
    /// for [`QUIET`]. Buttons on switches go through [`Throttle::button`] instead.
    pub fn heard(&mut self, data: [u8; 3], now: Instant) {
        self.moved(vec![canonical(data)], now);
    }

    /// The controller moved to where `shown` puts it (a control surface's fader, whose position
    /// may take two messages), at `now`: [`Throttle::heard`].
    pub fn moved(&mut self, shown: Msgs, now: Instant) {
        self.shown = Some(shown);
        self.quiet_until = Some(now + QUIET);
    }

    /// The controller's button on a switch was pressed (`released`: let go) at `now`. A button's
    /// light shows what the host last sent it, not its own presses, so what it shows is
    /// unknown until then: once the button is let go, the switch's state is sent even when it
    /// is what the button itself sent (a pad's note off after the press that switched Mute off).
    pub fn button(&mut self, released: bool, now: Instant) {
        self.shown = None;
        self.quiet_until = Some(if released { now } else { now + QUIET });
    }

    /// The message to send at `now` so the controller shows `want`, if any: none when it shows
    /// that already, while it is being moved, or within [`MIN_GAP`] of the last message (a
    /// later call sends the value then current, so the last one always goes).
    pub fn due(&mut self, want: Msgs, now: Instant) -> Option<Msgs> {
        if self.shown.as_ref() == Some(&want)
            || self.quiet_until.is_some_and(|t| now < t)
            || self.sent_at.is_some_and(|t| now.saturating_duration_since(t) < MIN_GAP)
        {
            return None;
        }
        self.shown = Some(want.clone());
        self.sent_at = Some(now);
        Some(want)
    }

    /// What the controller shows is unknown again (its output was connected anew): the next
    /// [`Throttle::due`] sends the value whatever it is.
    pub fn forget(&mut self) {
        self.shown = None;
    }
}

/// The device a port belongs to: ALSA's client name ("nanoKONTROL2" of "nanoKONTROL2:nanoKONTROL2
/// MIDI 1"), or the name inside Windows' "MIDIIN2 (…)" and "MIDIOUT2 (…)". `key` is a port name as
/// `controllers::port_key` gives it.
pub fn device_name(key: &str) -> &str {
    if let Some((client, _)) = key.split_once(':') {
        return client;
    }
    for prefix in ["MIDIIN", "MIDIOUT"] {
        if let Some(rest) = key.strip_prefix(prefix) {
            let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            if let Some(inner) = rest.strip_prefix(" (").and_then(|r| r.strip_suffix(')')) {
                return inner;
            }
        }
    }
    key
}

/// The output paired with the input `input` when none is chosen in Settings › MIDI: the output
/// port of the same name (on Linux a device's input and output are one ALSA port), or else the
/// only output of the same device. `outputs` are the connected outputs' keys.
pub fn auto_output<'a>(input: &str, outputs: &'a [String]) -> Option<&'a str> {
    if let Some(o) = outputs.iter().find(|o| *o == input) {
        return Some(o);
    }
    let dev = device_name(input);
    let mut same = outputs.iter().filter(|o| device_name(o) == dev);
    match (same.next(), same.next()) {
        (Some(o), None) => Some(o),
        _ => None,
    }
}

/// The output feedback for controllers on `input` goes to: `chosen` in Settings › MIDI (an
/// empty name for none), else [`auto_output`]. None when that output isn't connected.
pub fn paired_output<'a>(input: &str, chosen: Option<&str>, outputs: &'a [String]) -> Option<&'a str> {
    match chosen {
        Some(c) => outputs.iter().find(|o| !c.is_empty() && *o == c).map(|o| o.as_str()),
        None => auto_output(input, outputs),
    }
}

#[cfg(test)]
mod tests {
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
}
