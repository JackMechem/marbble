//! The HUI protocol (Mackie's for Digidesign), both ways, on MIDI channel 1:
//!
//! - **Ping**: the host sends note 0 velocity 0 about once a second, the HUI answers with
//!   velocity 127; without pings it shows itself offline.
//! - **Faders**: a 14-bit CC pair, CC 0x00–0x07 the coarse part and 0x20–0x27 the fine one,
//!   both ways.
//! - **Switches** are addressed by zone and port. A press: CC 0x0F with the zone, then CC 0x2F
//!   with the port, 0x40 added while pressed. A light: CC 0x0C with the zone, then CC 0x2C with
//!   the port, 0x40 added to light it. Port 0 of a strip's zone (0–7) is its fader's touch.
//! - **V-Pots** turn by CC 0x40–0x47 and the jog wheel by CC 0x0D, relative (bit 6 set is
//!   clockwise, bits 0–5 the steps); the rings are CC 0x10–0x17 ([`super::ring_value`]).
//! - **Meters**: polyphonic aftertouch, the strip as the note and the side (0x00 left, 0x10
//!   right) plus the level (0–12) as the value.
//! - **Displays** by system exclusive: each strip's four-character scribble strip (and the
//!   assignment display above them, number 8), and the time display's eight digits.
//!
//! The zone and port numbers are those of the HUI MIDI documentation.

use super::{Button, Digit, Input};

/// The system exclusive header: Mackie's id, the HUI's and its sub-id.
const HEADER: [u8; 6] = [0xF0, 0x00, 0x00, 0x66, 0x05, 0x00];

/// The host's ping.
pub const PING: [u8; 3] = [0x90, 0x00, 0x00];

/// The scribble strip number of the assignment display, above the strips' V-Pots.
pub const ASSIGN_DISPLAY: u8 = 8;

/// Switches by zone and port. A strip's zone (0–7) is its number; its port 0 is the fader's
/// touch, 4 (Auto) and 6 (Insert) aren't used.
const SWITCHES: &[(u8, u8, Button)] = &[
    (0x08, 0, Button::Control),
    (0x08, 1, Button::Shift),
    (0x08, 3, Button::Undo),
    (0x08, 4, Button::Alt),
    (0x08, 5, Button::Option),
    (0x08, 7, Button::Save),
    (0x0A, 0, Button::ChannelLeft),
    (0x0A, 1, Button::BankLeft),
    (0x0A, 2, Button::ChannelRight),
    (0x0A, 3, Button::BankRight),
    (0x0B, 2, Button::AssignPan),
    // Send A; B to E aren't used: pressing Send again goes to the next send.
    (0x0B, 7, Button::AssignSend),
    (0x0D, 0, Button::Down),
    (0x0D, 1, Button::Left),
    (0x0D, 3, Button::Right),
    (0x0D, 4, Button::Up),
    (0x0D, 5, Button::Scrub),
    (0x0E, 1, Button::Rewind),
    (0x0E, 2, Button::Forward),
    (0x0E, 3, Button::Stop),
    (0x0E, 4, Button::Play),
    (0x0E, 5, Button::Record),
    (0x0F, 0, Button::ToStart),
    (0x0F, 3, Button::Cycle),
    (0x16, 0, Button::SmpteLight),
    (0x16, 2, Button::BeatsLight),
    (0x16, 3, Button::RudeSolo),
    (0x18, 0, Button::Trim),
    (0x18, 1, Button::Latch),
    (0x18, 2, Button::Read),
    (0x18, 4, Button::Write),
    (0x18, 5, Button::Touch),
    (0x1B, 0, Button::F(0)),
    (0x1B, 1, Button::F(1)),
    (0x1B, 2, Button::F(2)),
    (0x1B, 3, Button::F(3)),
    (0x1B, 4, Button::F(4)),
    (0x1B, 5, Button::F(5)),
    (0x1B, 6, Button::F(6)),
    (0x1B, 7, Button::F(7)),
];

/// A button's zone and port, if a HUI has it.
pub fn zone_port(b: Button) -> Option<(u8, u8)> {
    let strip = |s: u8, port: u8| (s < 8).then_some((s, port));
    match b {
        Button::Select(s) => strip(s, 1),
        Button::Mute(s) => strip(s, 2),
        Button::Solo(s) => strip(s, 3),
        Button::VPot(s) => strip(s, 5),
        Button::Rec(s) => strip(s, 7),
        b => SWITCHES.iter().find(|(_, _, x)| *x == b).map(|(z, p, _)| (*z, *p)),
    }
}

/// The button at a zone and port, if any (a strip's fader touch isn't one).
pub fn button_of(zone: u8, port: u8) -> Option<Button> {
    if zone < 8 {
        return match port {
            1 => Some(Button::Select(zone)),
            2 => Some(Button::Mute(zone)),
            3 => Some(Button::Solo(zone)),
            5 => Some(Button::VPot(zone)),
            7 => Some(Button::Rec(zone)),
            _ => None,
        };
    }
    SWITCHES.iter().find(|(z, p, _)| *z == zone && *p == port).map(|(_, _, b)| *b)
}

/// Reads a HUI's messages, which need what came before: the zone a switch is in, a fader's
/// coarse part.
#[derive(Debug, Clone, Default)]
pub struct Decoder {
    zone: Option<u8>,
    coarse: [u8; 8],
}

impl Decoder {
    /// One message from the surface; None when it only sets up the next one or isn't read.
    pub fn feed(&mut self, msg: &[u8]) -> Option<Input> {
        match *msg {
            [0x90, 0x00, 0x7F, ..] => Some(Input::Alive),
            [0xB0, 0x0F, zone, ..] => {
                self.zone = Some(zone & 0x7F);
                None
            }
            [0xB0, 0x2F, v, ..] => {
                let (zone, port, down) = (self.zone?, v & 0x0F, v & 0x40 != 0);
                if zone < 8 && port == 0 {
                    return Some(Input::Touch { strip: zone, on: down });
                }
                Some(Input::Button { button: button_of(zone, port)?, down })
            }
            [0xB0, cc @ 0x00..=0x07, v, ..] => {
                self.coarse[cc as usize] = v & 0x7F;
                None
            }
            [0xB0, cc @ 0x20..=0x27, v, ..] => {
                let strip = cc - 0x20;
                let n = (self.coarse[strip as usize] as u16) << 7 | (v & 0x7F) as u16;
                Some(Input::Fader { strip, value: n as f32 / 16383.0 })
            }
            [0xB0, cc @ 0x40..=0x47, v, ..] => Some(Input::VPot { strip: cc - 0x40, steps: super::encoder_steps(v, false) }),
            [0xB0, 0x0D, v, ..] => Some(Input::Jog { steps: super::encoder_steps(v, false) }),
            _ => None,
        }
    }
}

/// A button's light: the zone, then the port.
pub fn led(b: Button, on: bool) -> Option<[[u8; 3]; 2]> {
    let (zone, port) = zone_port(b)?;
    Some([[0xB0, 0x0C, zone], [0xB0, 0x2C, port | if on { 0x40 } else { 0 }]])
}

/// A fader's position (0..1), strip 0..=7: the coarse part, then the fine one.
pub fn fader(strip: u8, v: f32) -> [[u8; 3]; 2] {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let n = (v * 16383.0).round() as u16;
    let s = strip & 0x07;
    [[0xB0, s, (n >> 7) as u8], [0xB0, 0x20 + s, (n & 0x7F) as u8]]
}

/// A V-Pot ring's value ([`super::ring_value`]).
pub fn ring(strip: u8, value: u8) -> [u8; 3] {
    [0xB0, 0x10 + (strip & 0x07), value & 0x7F]
}

/// A strip's meter level (0..=12), on both sides.
pub fn meter(strip: u8, level: u8) -> [[u8; 3]; 2] {
    let (s, l) = (strip & 0x07, level.min(0x0C));
    [[0xA0, s, l], [0xA0, s, 0x10 | l]]
}

/// A scribble strip's four characters (strips 0..=7, [`ASSIGN_DISPLAY`]). Characters outside
/// printable ASCII show as spaces.
pub fn scribble(strip: u8, text: &[u8]) -> Vec<u8> {
    let mut m = HEADER.to_vec();
    m.extend([0x10, strip.min(ASSIGN_DISPLAY)]);
    m.extend((0..4).map(|i| text.get(i).copied().filter(|c| (0x20..0x7F).contains(c)).unwrap_or(b' ')));
    m.push(0xF7);
    m
}

/// The time display's digits, given leftmost first (up to eight): sent rightmost first, as
/// numbers (a dot adds 0x10). Anything but a digit shows as 0.
pub fn timecode(digits: &[Digit]) -> Vec<u8> {
    let mut m = HEADER.to_vec();
    m.push(0x11);
    m.extend(digits.iter().rev().take(8).map(|d| (if d.ch.is_ascii_digit() { d.ch - b'0' } else { 0 }) | (d.dot as u8) << 4));
    m.push(0xF7);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(&[0x90, 0x00, 0x7F]), Some(Input::Alive));
        assert_eq!(PING, [0x90, 0, 0]);
    }

    #[test]
    fn switches_by_zone_and_port() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(&[0xB0, 0x2F, 0x42]), None, "no zone yet");
        assert_eq!(d.feed(&[0xB0, 0x0F, 0x03]), None);
        assert_eq!(d.feed(&[0xB0, 0x2F, 0x42]), Some(Input::Button { button: Button::Mute(3), down: true }));
        assert_eq!(d.feed(&[0xB0, 0x2F, 0x02]), Some(Input::Button { button: Button::Mute(3), down: false }));
        assert_eq!(d.feed(&[0xB0, 0x2F, 0x40]), Some(Input::Touch { strip: 3, on: true }));
        assert_eq!(d.feed(&[0xB0, 0x2F, 0x00]), Some(Input::Touch { strip: 3, on: false }));
        assert_eq!(d.feed(&[0xB0, 0x2F, 0x44]), None, "Auto isn't used");
        d.feed(&[0xB0, 0x0F, 0x0E]);
        assert_eq!(d.feed(&[0xB0, 0x2F, 0x44]), Some(Input::Button { button: Button::Play, down: true }));
        d.feed(&[0xB0, 0x0F, 0x0A]);
        assert_eq!(d.feed(&[0xB0, 0x2F, 0x43]), Some(Input::Button { button: Button::BankRight, down: true }));
        // Every switch reads back from its light's address.
        for zone in 0..0x1D {
            for port in 0..8 {
                if let Some(b) = button_of(zone, port) {
                    assert_eq!(zone_port(b), Some((zone, port)), "{b:?}");
                    assert_eq!(led(b, true), Some([[0xB0, 0x0C, zone], [0xB0, 0x2C, 0x40 | port]]));
                    assert_eq!(led(b, false), Some([[0xB0, 0x0C, zone], [0xB0, 0x2C, port]]));
                }
            }
        }
        assert_eq!(zone_port(Button::Rec(0)), Some((0, 7)));
        assert_eq!(zone_port(Button::Flip), None, "Mackie Control only");
    }

    #[test]
    fn faders_both_ways() {
        let mut d = Decoder::default();
        for (strip, v) in [(0u8, 0.0f32), (2, 0.5), (7, 1.0), (4, 0.123)] {
            let [hi, lo] = fader(strip, v);
            assert_eq!(hi[1], strip);
            assert_eq!(lo[1], 0x20 + strip);
            assert_eq!(d.feed(&hi), None);
            let Some(Input::Fader { strip: s, value }) = d.feed(&lo) else { panic!() };
            assert_eq!(s, strip);
            assert!((value - v).abs() < 1.0 / 16383.0, "{value} {v}");
        }
        assert_eq!(fader(1, 1.0), [[0xB0, 0x01, 0x7F], [0xB0, 0x21, 0x7F]]);
        assert_eq!(fader(1, 8192.0 / 16383.0), [[0xB0, 0x01, 0x40], [0xB0, 0x21, 0x00]]);
    }

    #[test]
    fn vpots_rings_and_meters() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(&[0xB0, 0x41, 0x41]), Some(Input::VPot { strip: 1, steps: 1 }));
        assert_eq!(d.feed(&[0xB0, 0x47, 0x03]), Some(Input::VPot { strip: 7, steps: -3 }));
        assert_eq!(d.feed(&[0xB0, 0x0D, 0x42]), Some(Input::Jog { steps: 2 }));
        assert_eq!(ring(3, 0x16), [0xB0, 0x13, 0x16]);
        assert_eq!(meter(2, 9), [[0xA0, 2, 9], [0xA0, 2, 0x19]]);
        assert_eq!(meter(2, 99), [[0xA0, 2, 12], [0xA0, 2, 0x1C]]);
    }

    #[test]
    fn displays() {
        assert_eq!(scribble(0, b"Kick"), vec![0xF0, 0, 0, 0x66, 0x05, 0x00, 0x10, 0, b'K', b'i', b'c', b'k', 0xF7]);
        assert_eq!(scribble(ASSIGN_DISPLAY, b"Pn"), vec![0xF0, 0, 0, 0x66, 0x05, 0x00, 0x10, 8, b'P', b'n', b' ', b' ', 0xF7]);
        assert_eq!(scribble(1, &[0xE9, b'a']), vec![0xF0, 0, 0, 0x66, 0x05, 0x00, 0x10, 1, b' ', b'a', b' ', b' ', 0xF7]);
        let d = |ch, dot| Digit { ch, dot };
        // "005.02.491": sent from the right.
        let digits = crate::surface::hui_bbt_digits(&crate::tempo::Bbt { bar: 5, beat: 2, div: 3, tick: 12 });
        assert_eq!(digits[2], d(b'5', true));
        assert_eq!(timecode(&digits), vec![0xF0, 0, 0, 0x66, 0x05, 0x00, 0x11, 1, 9, 4, 0x12, 0, 0x15, 0, 0, 0xF7]);
    }
}
