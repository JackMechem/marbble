//! Reading controller messages off the MIDI input: control changes, 14-bit pairs and NRPNs
//! ([`HiRes`]), and working out which kind a learn heard ([`Learner`]).

use std::time::{Duration, Instant};

use super::assignment::is_nrpn_part;
use super::*;

/// A controller message: its channel, which controller, the value scaled to 0..1, and whether
/// it counts as a button press (above 63, or above the centre for pitch bend; a note on; any
/// program change).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CtlMsg {
    pub channel: u8,
    pub kind: CtlKind,
    pub value: f32,
    pub pressed: bool,
    /// An NRPN's data increment (1, CC 96) or decrement (-1, CC 97): its parameter steps
    /// rather than goes to `value`. 0 for every other message.
    pub step: i8,
}

/// Reads a control change, pitch bend, note or program change message; anything else is None.
pub fn parse(data: &[u8]) -> Option<CtlMsg> {
    let (&status, rest) = data.split_first()?;
    let channel = status & 0x0F;
    match (status & 0xF0, rest) {
        (0x90, [number, velocity, ..]) if *velocity & 0x7F > 0 => {
            Some(CtlMsg { channel, kind: CtlKind::Note { number: number & 0x7F }, value: (velocity & 0x7F) as f32 / 127.0, pressed: true, step: 0 })
        }
        (0x80 | 0x90, [number, ..]) => Some(CtlMsg { channel, kind: CtlKind::Note { number: number & 0x7F }, value: 0.0, pressed: false, step: 0 }),
        (0xC0, [number, ..]) => Some(CtlMsg { channel, kind: CtlKind::Program { number: number & 0x7F }, value: 1.0, pressed: true, step: 0 }),
        (0xB0, [number, value, ..]) if *number < 120 => {
            let v = value & 0x7F;
            Some(CtlMsg { channel, kind: CtlKind::Cc { number: *number }, value: v as f32 / 127.0, pressed: v > 63, step: 0 })
        }
        (0xE0, [lsb, msb, ..]) => {
            let v = ((msb & 0x7F) as u16) << 7 | (lsb & 0x7F) as u16;
            Some(CtlMsg { channel, kind: CtlKind::PitchBend, value: v as f32 / 16383.0, pressed: v > 8192, step: 0 })
        }
        _ => None,
    }
}

/// One port and channel's state for [`HiRes`].
#[derive(Debug, Clone, Default)]
struct Chan {
    /// The last coarse value of each 14-bit controller (CC 0..=31).
    msb: [Option<u8>; 32],
    /// The NRPN number's MSB (CC 99) and LSB (CC 98).
    param: (Option<u8>, Option<u8>),
    /// An RPN (CC 101, 100) was chosen since: data entry isn't for an NRPN.
    rpn: bool,
    /// The last data entry MSB (CC 6) for the chosen NRPN.
    data: Option<u8>,
}

/// Reads 14-bit controllers and NRPNs out of the control changes that carry them, keeping the
/// state they need per port and channel: a 14-bit controller's coarse part waits for its fine
/// part, an NRPN's number for its data.
#[derive(Debug, Default)]
pub struct HiRes {
    chans: Vec<(String, u8, Chan)>,
}

impl HiRes {
    fn chan(&mut self, port: &str, channel: u8) -> &mut Chan {
        let i = match self.chans.iter().position(|(p, c, _)| p == port && *c == channel) {
            Some(i) => i,
            None => {
                self.chans.push((port.to_string(), channel, Chan::default()));
                self.chans.len() - 1
            }
        };
        &mut self.chans[i].2
    }

    /// What a message from `port` means: a 14-bit controller's value when `pair(n)` says CC n
    /// and n + 32 are one (the coarse part moves it at 7 bits, the fine part to 14), an NRPN's
    /// value when `nrpn` says NRPNs are in use on its channel (CC 99, 98, 101 and 100 are then
    /// only selections, and give None), else the message itself.
    pub fn read(&mut self, port: &str, msg: CtlMsg, pair: impl Fn(u8) -> bool, nrpn: bool) -> Option<CtlMsg> {
        let CtlKind::Cc { number: n } = msg.kind else { return Some(msg) };
        let v = (msg.value * 127.0).round() as u8;
        let with = |kind: CtlKind, value: f32| Some(CtlMsg { kind, value, pressed: value > 0.5, ..msg });
        let c = self.chan(port, msg.channel);
        if nrpn && is_nrpn_part(n) {
            let number = match c.param {
                (Some(m), Some(l)) if !c.rpn && (m, l) != (127, 127) => Some((m as u16) << 7 | l as u16),
                _ => None,
            };
            match (n, number) {
                (99, _) => {
                    (c.param.0, c.rpn, c.data) = (Some(v), false, None);
                    return None;
                }
                (98, _) => {
                    (c.param.1, c.rpn, c.data) = (Some(v), false, None);
                    return None;
                }
                (100 | 101, _) => {
                    c.rpn = true;
                    return None;
                }
                (6, Some(number)) => {
                    c.data = Some(v);
                    return with(CtlKind::Nrpn { number }, v as f32 / 127.0);
                }
                (38, Some(number)) => {
                    let m = c.data?;
                    return with(CtlKind::Nrpn { number }, ((m as u16) << 7 | v as u16) as f32 / 16383.0);
                }
                // Increment and decrement step the parameter, as a relative encoder does.
                (96 | 97, Some(number)) => {
                    let step = if n == 96 { 1 } else { -1 };
                    return Some(CtlMsg { kind: CtlKind::Nrpn { number }, value: 0.0, pressed: false, step, ..msg });
                }
                // No NRPN chosen: data entry is an ordinary controller.
                _ => {}
            }
        }
        if n < 32 && pair(n) {
            c.msb[n as usize] = Some(v);
            return with(CtlKind::Cc14 { number: n }, v as f32 / 127.0);
        }
        if (32..64).contains(&n) && pair(n - 32) {
            let m = c.msb[n as usize - 32]?;
            return with(CtlKind::Cc14 { number: n - 32 }, ((m as u16) << 7 | v as u16) as f32 / 16383.0);
        }
        Some(msg)
    }
}

/// How long a learn waits for the fine part of a CC 0..=31 before taking it as a 7-bit
/// controller. A 14-bit controller sends both parts together.
const PAIR_WAIT: Duration = Duration::from_millis(120);

/// Works out which controller a learn heard: a note on, a program change, pitch bend, a
/// control change, or, from the messages that follow, a 14-bit pair or an NRPN.
#[derive(Debug, Default)]
pub struct Learner {
    hires: HiRes,
    /// A CC 0..=31 that may be the coarse part of a 14-bit pair.
    pending: Option<(String, CtlMsg, Instant)>,
}

impl Learner {
    /// Feeds a message from `port` (as [`parse`] reads it); returns the controller to assign,
    /// once it's clear.
    pub fn feed(&mut self, port: &str, msg: CtlMsg, now: Instant) -> Option<Controller> {
        // The release of a note held when the learn started isn't one to learn.
        if msg.kind.is_button() && !msg.pressed {
            return None;
        }
        let msg = self.hires.read(port, msg, |_| false, true)?;
        let controller = |port: &str, m: &CtlMsg, kind| Controller { port: port.to_string(), channel: m.channel, kind };
        if let Some((p, first, _)) = self.pending.take() {
            let pairs =
                matches!(first.kind, CtlKind::Cc { number } if p == port && first.channel == msg.channel && msg.kind == CtlKind::Cc { number: number + 32 });
            let kind = match first.kind {
                CtlKind::Cc { number } if pairs => CtlKind::Cc14 { number },
                kind => kind,
            };
            return Some(controller(&p, &first, kind));
        }
        match msg.kind {
            CtlKind::Cc { number } if number < 32 => {
                self.pending = Some((port.to_string(), msg, now));
                None
            }
            kind => Some(controller(port, &msg, kind)),
        }
    }

    /// A coarse part whose fine part didn't come is a 7-bit controller.
    pub fn poll(&mut self, now: Instant) -> Option<Controller> {
        let (port, msg, at) = self.pending.as_ref()?;
        (now.duration_since(*at) >= PAIR_WAIT).then(|| Controller { port: port.clone(), channel: msg.channel, kind: msg.kind })
    }
}
