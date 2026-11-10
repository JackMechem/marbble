//! The Mackie Control protocol (Mackie Control Universal, its Extender, Logic Control and the
//! Mackie Control modes of other surfaces), both ways:
//!
//! - **Faders**: pitch bend, 14 bits, on channel 1–8 for the strips and 9 for the master, both
//!   ways. Touching one sends note 0x68–0x70 (velocity 127, 0 when let go).
//! - **Buttons**: notes, velocity 127 pressed and 0 released; the host lights a button's LED with
//!   the same note (127 on, 0 off, 1 flashing).
//! - **V-Pots**: CC 0x10–0x17, relative (bit 6 set is counter-clockwise, bits 0–5 the steps);
//!   their rings CC 0x30–0x37 ([`super::ring_value`]). The jog wheel is CC 0x3C, relative too.
//! - **Meters**: channel pressure on channel 1, the strip in the high nibble and the level
//!   (0–12) in the low one. The surface lets them fall by itself.
//! - **Displays**: the LCD, 2 lines of 56 characters, by system exclusive (`0x12`, an offset and
//!   the characters); the time display's ten digits and the assignment display's two by CC
//!   0x40–0x4B, rightmost first, as seven-segment characters.
//! - **Handshake**: the host's device query (`0x00`) is answered with the surface's serial
//!   number and a challenge (`0x01`), the host answers that (`0x02`), and the surface confirms
//!   (`0x03`).

use super::{Button, Digit, Input};

/// The system exclusive header: Mackie's id, then the device's.
const MACKIE: [u8; 4] = [0xF0, 0x00, 0x00, 0x66];

/// The device id of a Mackie Control and of an Extender.
pub const MAIN: u8 = 0x14;
pub const EXTENDER: u8 = 0x15;

/// Device ids a surface may answer with: a Mackie Control, its Extender, a Logic Control and
/// its Extender.
const DEVICES: [u8; 4] = [0x14, 0x15, 0x10, 0x11];

/// The first fader touch note (strip 1); the master's is 0x70.
const TOUCH: u8 = 0x68;

/// The buttons by note. Strips' buttons are 0x00–0x27 (eight each of Rec, Solo, Mute, Select
/// and V-Pot), the fader touches 0x68–0x70.
const NOTES: &[(u8, Button)] = &[
    (0x28, Button::AssignTrack),
    (0x29, Button::AssignSend),
    (0x2A, Button::AssignPan),
    (0x2B, Button::AssignPlugin),
    (0x2C, Button::AssignEq),
    (0x2D, Button::AssignInstrument),
    (0x2E, Button::BankLeft),
    (0x2F, Button::BankRight),
    (0x30, Button::ChannelLeft),
    (0x31, Button::ChannelRight),
    (0x32, Button::Flip),
    (0x33, Button::GlobalView),
    (0x34, Button::NameValue),
    (0x35, Button::SmpteBeats),
    (0x36, Button::F(0)),
    (0x37, Button::F(1)),
    (0x38, Button::F(2)),
    (0x39, Button::F(3)),
    (0x3A, Button::F(4)),
    (0x3B, Button::F(5)),
    (0x3C, Button::F(6)),
    (0x3D, Button::F(7)),
    // 0x3E–0x45: the global view's track type buttons, not used.
    (0x46, Button::Shift),
    (0x47, Button::Option),
    (0x48, Button::Control),
    (0x49, Button::Alt),
    (0x4A, Button::Read),
    (0x4B, Button::Write),
    (0x4C, Button::Trim),
    (0x4D, Button::Touch),
    (0x4E, Button::Latch),
    (0x4F, Button::Group),
    (0x50, Button::Save),
    (0x51, Button::Undo),
    (0x52, Button::Cancel),
    (0x53, Button::Enter),
    (0x54, Button::Marker),
    (0x55, Button::Nudge),
    (0x56, Button::Cycle),
    (0x57, Button::Drop),
    (0x58, Button::Replace),
    (0x59, Button::Click),
    (0x5A, Button::SoloClear),
    (0x5B, Button::Rewind),
    (0x5C, Button::Forward),
    (0x5D, Button::Stop),
    (0x5E, Button::Play),
    (0x5F, Button::Record),
    (0x60, Button::Up),
    (0x61, Button::Down),
    (0x62, Button::Left),
    (0x63, Button::Right),
    (0x64, Button::Zoom),
    (0x65, Button::Scrub),
    // 0x66, 0x67: the footswitches, not used.
    (0x71, Button::SmpteLight),
    (0x72, Button::BeatsLight),
    (0x73, Button::RudeSolo),
];

/// The note of a button (or light), if a Mackie Control has it.
pub fn note_of(b: Button) -> Option<u8> {
    let strip = |base: u8, s: u8| (s < 8).then_some(base + s);
    match b {
        Button::Rec(s) => strip(0x00, s),
        Button::Solo(s) => strip(0x08, s),
        Button::Mute(s) => strip(0x10, s),
        Button::Select(s) => strip(0x18, s),
        Button::VPot(s) => strip(0x20, s),
        Button::ToStart => None,
        b => NOTES.iter().find(|(_, x)| *x == b).map(|(n, _)| *n),
    }
}

/// The button a note is, if any (fader touches aren't buttons).
pub fn button_of(note: u8) -> Option<Button> {
    let s = note & 0x07;
    Some(match note {
        0x00..=0x07 => Button::Rec(s),
        0x08..=0x0F => Button::Solo(s),
        0x10..=0x17 => Button::Mute(s),
        0x18..=0x1F => Button::Select(s),
        0x20..=0x27 => Button::VPot(s),
        n => return NOTES.iter().find(|(m, _)| *m == n).map(|(_, b)| *b),
    })
}

/// Reads one message from a surface.
pub fn decode(msg: &[u8]) -> Option<Input> {
    let (&status, rest) = msg.split_first()?;
    match (status & 0xF0, rest) {
        (0xE0, [lsb, msb, ..]) => {
            let v = ((msb & 0x7F) as u16) << 7 | (lsb & 0x7F) as u16;
            let strip = status & 0x0F;
            (strip <= super::MASTER).then_some(Input::Fader { strip, value: v as f32 / 16383.0 })
        }
        (0x90 | 0x80, [note, vel, ..]) => {
            let down = status & 0xF0 == 0x90 && *vel > 0;
            match *note {
                n @ TOUCH..=0x70 => Some(Input::Touch { strip: n - TOUCH, on: down }),
                n => Some(Input::Button { button: button_of(n)?, down }),
            }
        }
        (0xB0, [cc @ 0x10..=0x17, v, ..]) => Some(Input::VPot { strip: cc - 0x10, steps: super::encoder_steps(*v, true) }),
        (0xB0, [0x3C, v, ..]) => Some(Input::Jog { steps: super::encoder_steps(*v, true) }),
        (0xF0, _) => sysex(msg),
        _ => None,
    }
}

/// The handshake's messages from the surface.
fn sysex(msg: &[u8]) -> Option<Input> {
    if msg.len() < 7 || msg[..4] != MACKIE || !DEVICES.contains(&msg[4]) || msg.last() != Some(&0xF7) {
        return None;
    }
    let body = &msg[6..msg.len() - 1];
    match msg[5] {
        // Host connection query: the serial number, then the challenge (some surfaces send
        // none, and expect no answer to it).
        0x01 if body.len() >= 7 => {
            let mut serial = [0; 7];
            serial.copy_from_slice(&body[..7]);
            let mut challenge = [0; 4];
            if body.len() >= 11 {
                challenge.copy_from_slice(&body[7..11]);
            }
            Some(Input::Hello { serial, challenge })
        }
        // Connection confirmed; the version reply.
        0x03 | 0x14 => Some(Input::Alive),
        _ => None,
    }
}

fn sysex_msg(device: u8, body: &[u8]) -> Vec<u8> {
    let mut m = MACKIE.to_vec();
    m.push(device);
    m.extend(body.iter().map(|b| b & 0x7F));
    m.push(0xF7);
    m
}

/// The device query the host sends until the surface answers.
pub fn device_query(device: u8) -> Vec<u8> {
    sysex_msg(device, &[0x00])
}

/// The answer to a surface's challenge, as Logic computes it (the formula Ardour has from
/// Logic's documentation; a shift by 32 or more wraps as it does on x86).
pub fn challenge_response(c: [u8; 4]) -> [u8; 4] {
    let l = c.map(|b| b as i32);
    [
        0x7F & (l[0] + (l[1] ^ 0x0A) - l[3]),
        0x7F & (l[2].wrapping_shr(l[3] as u32) ^ (l[0] + l[3])),
        0x7F & ((l[3] - (l[2] << 2)) ^ (l[0] | l[1])),
        0x7F & (l[1] - l[2] + (0xF0 ^ (l[3] << 4))),
    ]
    .map(|v| v as u8)
}

/// The host connection reply: the surface's serial number and the answer to its challenge.
pub fn connection_reply(device: u8, serial: [u8; 7], challenge: [u8; 4]) -> Vec<u8> {
    let mut body = vec![0x02];
    body.extend(serial);
    body.extend(challenge_response(challenge));
    sysex_msg(device, &body)
}

/// Sets how a strip's meter shows: its signal light, peak hold and the LCD's level meter
/// (bits 0, 1 and 2 of `mode`).
pub fn meter_mode(device: u8, strip: u8, mode: u8) -> Vec<u8> {
    sysex_msg(device, &[0x20, strip & 0x07, mode & 0x07])
}

/// The LCD's characters from `offset` (0..=0x37 the top line, 0x38..=0x6F the bottom one).
/// Characters outside printable ASCII show as spaces.
pub fn lcd(device: u8, offset: u8, text: &[u8]) -> Vec<u8> {
    let mut body = vec![0x12, offset.min(0x6F)];
    body.extend(text.iter().take(0x70 - offset.min(0x6F) as usize).map(|&c| if (0x20..0x7F).contains(&c) { c } else { b' ' }));
    sysex_msg(device, &body)
}

/// A fader's position (0..1), strip 0..=7 or [`super::MASTER`].
pub fn fader(strip: u8, v: f32) -> [u8; 3] {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let n = (v * 16383.0).round() as u16;
    [0xE0 | (strip & 0x0F), (n & 0x7F) as u8, (n >> 7) as u8]
}

/// A button's light.
pub fn led(b: Button, on: bool) -> Option<[u8; 3]> {
    Some([0x90, note_of(b)?, if on { 0x7F } else { 0 }])
}

/// A V-Pot ring's value ([`super::ring_value`]).
pub fn ring(strip: u8, value: u8) -> [u8; 3] {
    [0xB0, 0x30 + (strip & 0x07), value & 0x7F]
}

/// A strip's meter level (0..=12).
pub fn meter(strip: u8, level: u8) -> [u8; 2] {
    [0xD0, (strip & 0x07) << 4 | level.min(0x0C)]
}

/// A character as the seven-segment displays have it: letters (either case) 0x01–0x1A, digits
/// and most punctuation as in ASCII, anything else blank.
pub fn seven_segment(c: u8) -> u8 {
    match c.to_ascii_uppercase() {
        c @ 0x40..=0x5F => c - 0x40,
        c @ 0x20..=0x3F => c,
        _ => 0x20,
    }
}

/// A digit of the time display (positions 0..=9, rightmost first) or the assignment display
/// (10 the right one, 11 the left one).
pub fn digit(position: u8, d: Digit) -> [u8; 3] {
    [0xB0, 0x40 + position.min(11), seven_segment(d.ch) | (d.dot as u8) << 6]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::surface::{ring_value, RingMode, MASTER};

    #[test]
    fn faders_both_ways() {
        assert_eq!(fader(0, 1.0), [0xE0, 0x7F, 0x7F]);
        assert_eq!(fader(3, 0.0), [0xE3, 0, 0]);
        assert_eq!(fader(MASTER, 8192.0 / 16383.0), [0xE8, 0, 0x40]);
        assert_eq!(fader(1, f32::NAN), [0xE1, 0, 0]);
        for v in [0.0, 0.25, 0.7, 1.0] {
            let Some(Input::Fader { strip, value }) = decode(&fader(5, v)) else { panic!() };
            assert_eq!(strip, 5);
            assert!((value - v).abs() < 1.0 / 16383.0);
        }
        // Channel 10 and up aren't faders.
        assert_eq!(decode(&[0xE9, 0, 0]), None);
    }

    #[test]
    fn touches_and_buttons() {
        assert_eq!(decode(&[0x90, 0x68, 0x7F]), Some(Input::Touch { strip: 0, on: true }));
        assert_eq!(decode(&[0x90, 0x70, 0x00]), Some(Input::Touch { strip: MASTER, on: false }));
        assert_eq!(decode(&[0x90, 0x10, 0x7F]), Some(Input::Button { button: Button::Mute(0), down: true }));
        assert_eq!(decode(&[0x90, 0x1F, 0x00]), Some(Input::Button { button: Button::Select(7), down: false }));
        assert_eq!(decode(&[0x80, 0x5E, 0x40]), Some(Input::Button { button: Button::Play, down: false }));
        assert_eq!(decode(&[0x90, 0x36, 0x7F]), Some(Input::Button { button: Button::F(0), down: true }));
        assert_eq!(decode(&[0x90, 0x3E, 0x7F]), None, "an unused button");
        // Every button reads back from its own light's note.
        for n in 0u8..0x74 {
            if let Some(b) = button_of(n) {
                assert_eq!(note_of(b), Some(n), "{b:?}");
                assert_eq!(led(b, true), Some([0x90, n, 0x7F]));
            }
        }
        assert_eq!(note_of(Button::Rec(2)), Some(0x02));
        assert_eq!(note_of(Button::VPot(7)), Some(0x27));
        assert_eq!(note_of(Button::Rec(8)), None);
        assert_eq!(note_of(Button::ToStart), None, "HUI only");
        assert_eq!(led(Button::RudeSolo, false), Some([0x90, 0x73, 0]));
    }

    #[test]
    fn vpots_and_jog() {
        assert_eq!(decode(&[0xB0, 0x10, 0x01]), Some(Input::VPot { strip: 0, steps: 1 }));
        assert_eq!(decode(&[0xB0, 0x17, 0x43]), Some(Input::VPot { strip: 7, steps: -3 }));
        assert_eq!(decode(&[0xB0, 0x3C, 0x41]), Some(Input::Jog { steps: -1 }));
        assert_eq!(decode(&[0xB0, 0x3C, 0x07]), Some(Input::Jog { steps: 7 }));
        assert_eq!(ring(2, ring_value(RingMode::Dot, Some(0.5), false)), [0xB0, 0x32, 0x06]);
    }

    #[test]
    fn meters() {
        assert_eq!(meter(0, 12), [0xD0, 0x0C]);
        assert_eq!(meter(7, 3), [0xD0, 0x73]);
        assert_eq!(meter(1, 40), [0xD0, 0x1C], "at most 12");
    }

    #[test]
    fn lcd_sysex() {
        assert_eq!(lcd(MAIN, 0, b"Kick"), vec![0xF0, 0, 0, 0x66, 0x14, 0x12, 0, b'K', b'i', b'c', b'k', 0xF7]);
        assert_eq!(lcd(EXTENDER, 0x38, b"C"), vec![0xF0, 0, 0, 0x66, 0x15, 0x12, 0x38, b'C', 0xF7]);
        // Nothing beyond the bottom line's end, nothing outside 7 bits.
        let m = lcd(MAIN, 0x6E, &[b'a', 0xE9, b'c', b'd']);
        assert_eq!(m, vec![0xF0, 0, 0, 0x66, 0x14, 0x12, 0x6E, b'a', b' ', 0xF7]);
    }

    #[test]
    fn digits() {
        assert_eq!(seven_segment(b'0'), 0x30);
        assert_eq!(seven_segment(b'9'), 0x39);
        assert_eq!(seven_segment(b'A'), 0x01);
        assert_eq!(seven_segment(b's'), 0x13);
        assert_eq!(seven_segment(b' '), 0x20);
        assert_eq!(seven_segment(0xE9), 0x20);
        assert_eq!(digit(0, Digit { ch: b'1', dot: false }), [0xB0, 0x40, 0x31]);
        assert_eq!(digit(2, Digit { ch: b'5', dot: true }), [0xB0, 0x42, 0x75]);
        assert_eq!(digit(11, Digit { ch: b'P', dot: false }), [0xB0, 0x4B, 0x10]);
    }

    #[test]
    fn handshake() {
        assert_eq!(device_query(MAIN), vec![0xF0, 0, 0, 0x66, 0x14, 0x00, 0xF7]);
        let hello = [0xF0, 0, 0, 0x66, 0x14, 0x01, 1, 2, 3, 4, 5, 6, 7, 0x10, 0x20, 0x30, 0x40, 0xF7];
        let Some(Input::Hello { serial, challenge }) = decode(&hello) else { panic!() };
        assert_eq!(serial, [1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(challenge, [0x10, 0x20, 0x30, 0x40]);
        // Without a challenge.
        assert!(matches!(decode(&[0xF0, 0, 0, 0x66, 0x15, 0x01, 1, 2, 3, 4, 5, 6, 7, 0xF7]), Some(Input::Hello { challenge: [0, 0, 0, 0], .. })));
        assert_eq!(decode(&[0xF0, 0, 0, 0x66, 0x14, 0x03, 1, 2, 3, 4, 5, 6, 7, 0xF7]), Some(Input::Alive));
        assert_eq!(decode(&[0xF0, 0, 0, 0x66, 0x05, 0x03, 0xF7]), None, "not a Mackie Control");
        assert_eq!(decode(&[0xF0, 0, 0, 0x66, 0x14, 0x01, 1, 2]), None, "cut short");
        let reply = connection_reply(MAIN, serial, challenge);
        assert_eq!(reply[..13], [0xF0, 0, 0, 0x66, 0x14, 0x02, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(reply[13..17], challenge_response(challenge));
        assert_eq!(reply.len(), 18);
        assert!(reply[1..17].iter().all(|b| *b < 0x80));
        // Worked by hand from the formula: l = [0x10, 0x20, 0x30, 0x40].
        assert_eq!(challenge_response([0x10, 0x20, 0x30, 0x40]), [0x7A, 0x60, 0x30, 0x60]);
        assert_eq!(meter_mode(EXTENDER, 3, 3), vec![0xF0, 0, 0, 0x66, 0x15, 0x20, 3, 3, 0xF7]);
    }
}
