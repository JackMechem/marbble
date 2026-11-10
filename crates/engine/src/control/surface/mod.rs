//! Control surfaces, as Logic's Control Surfaces setup has them: a Mackie Control (with any
//! number of Mackie Control Extenders) or a HUI, on an input and an output port, its strips
//! showing a bank of the visible tracks.
//!
//! This module is the pure part, shared by both protocols: the units' settings
//! ([`SurfaceConfig`], in Settings › MIDI › Control Surfaces), what a surface did ([`Input`],
//! with its [`Button`]s), banking, and what the displays show (names fitted to a strip, values,
//! the time display's digits, V-Pot rings, meter levels). The protocols' own messages are in
//! [`mcu`] and [`hui`]. The session reads the surfaces' messages, applies them through the
//! controller assignments' edit paths and sends what changed back (`session/control/surface/`), on the
//! main thread; the messages go out from the `midi-out` thread (`midi/output.rs`).

pub mod hui;
pub mod mcu;

use serde::{Deserialize, Serialize};

use crate::model::tempo::Bbt;

/// Channel strips on a unit: a Mackie Control, an Extender and a HUI all have eight.
pub const STRIPS: usize = 8;

/// The master fader's strip number in [`Input::Fader`] and [`Input::Touch`] (a Mackie Control's
/// ninth fader).
pub const MASTER: u8 = 8;

/// A surface's protocol.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    /// Mackie Control (also Logic Control, and the Mackie Control mode of other surfaces): 8
    /// strips, a master fader, transport, the time display.
    #[default]
    Mcu,
    /// Mackie Control Extender: 8 more strips, nothing else.
    McuExtender,
    /// Digidesign / Mackie HUI.
    Hui,
}

impl SurfaceKind {
    /// Whether the unit has the master section: transport, the time display, the global
    /// buttons (and on a Mackie Control, the master fader).
    pub fn is_main(self) -> bool {
        !matches!(self, SurfaceKind::McuExtender)
    }

    /// Its name in Settings.
    pub fn name(self) -> &'static str {
        match self {
            SurfaceKind::Mcu => "Mackie Control",
            SurfaceKind::McuExtender => "Mackie Control Extender",
            SurfaceKind::Hui => "HUI",
        }
    }
}

/// One unit, as set up in Settings › MIDI › Control Surfaces.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceConfig {
    pub kind: SurfaceKind,
    /// The ports it is on, by key (`controllers::port_key`); empty for none.
    pub input: String,
    pub output: String,
    /// Its first strip, counted from the bank's first track: 0 for the main unit, 8 for an
    /// extender to its right, and so on.
    pub offset: u32,
    /// F1–F8 (on a HUI too): the key command each runs (an id of the commands MIDI can fire,
    /// `midiCommands` in src/keyboard/keymap.ts), empty for none.
    pub functions: Vec<String>,
}

/// Something a surface did, read from its messages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// Fader `strip` (0..=7, [`MASTER`]) moved to `value`, 0..1 on its travel.
    Fader {
        strip: u8,
        value: f32,
    },
    /// A fader was touched (`on`) or let go.
    Touch {
        strip: u8,
        on: bool,
    },
    /// A V-Pot turned, by `steps` (positive clockwise).
    VPot {
        strip: u8,
        steps: i32,
    },
    /// The jog wheel turned, by `steps` (positive clockwise).
    Jog {
        steps: i32,
    },
    Button {
        button: Button,
        down: bool,
    },
    /// A Mackie Control answered the device query (or announced itself): its serial number and
    /// the challenge the host answers.
    Hello {
        serial: [u8; 7],
        challenge: [u8; 4],
    },
    /// A Mackie Control confirmed the connection; a HUI answered the ping.
    Alive,
}

/// A button (or, for the last three, a light) on a surface. Buttons a protocol doesn't have
/// are never read from it, and their lights never sent to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    /// Record enable of strip 0..=7.
    Rec(u8),
    Solo(u8),
    Mute(u8),
    Select(u8),
    /// A V-Pot pushed.
    VPot(u8),
    AssignTrack,
    AssignSend,
    AssignPan,
    AssignPlugin,
    AssignEq,
    AssignInstrument,
    BankLeft,
    BankRight,
    ChannelLeft,
    ChannelRight,
    Flip,
    GlobalView,
    NameValue,
    SmpteBeats,
    /// F1..=F8 (0..=7).
    F(u8),
    Shift,
    Option,
    Control,
    Alt,
    Read,
    Write,
    Trim,
    Touch,
    Latch,
    Group,
    Save,
    Undo,
    Cancel,
    Enter,
    Marker,
    Nudge,
    Cycle,
    Drop,
    Replace,
    Click,
    /// The master section's Solo: clears every solo; its light is the "rude solo" one.
    SoloClear,
    Rewind,
    Forward,
    Stop,
    Play,
    Record,
    Up,
    Down,
    Left,
    Right,
    Zoom,
    Scrub,
    /// HUI's Return to Zero.
    ToStart,
    /// A light: the time display shows minutes and seconds.
    SmpteLight,
    /// A light: the time display shows bars and beats.
    BeatsLight,
    /// A light: some track is soloed.
    RudeSolo,
}

impl Button {
    /// The strip a channel strip button belongs to.
    pub fn strip(self) -> Option<u8> {
        match self {
            Button::Rec(s) | Button::Solo(s) | Button::Mute(s) | Button::Select(s) | Button::VPot(s) => Some(s),
            _ => None,
        }
    }
}

/// The bank's first track, kept within the tracks: banking stops where the last track reaches
/// the last strip, so the strips are full whenever there are enough tracks.
pub fn clamp_bank(bank: i64, tracks: usize, strips: usize) -> usize {
    bank.clamp(0, tracks.saturating_sub(strips) as i64) as usize
}

/// How many strips the units cover together (the main unit's and its extenders').
pub fn strips_of(units: &[SurfaceConfig]) -> usize {
    units.iter().map(|u| u.offset as usize + STRIPS).max().unwrap_or(STRIPS)
}

/// The next unit's offset: right of every unit so far.
pub fn next_offset(units: &[SurfaceConfig]) -> u32 {
    if units.is_empty() {
        0
    } else {
        strips_of(units) as u32
    }
}

/// A character as a display has it: printable ASCII, accented Latin letters without their
/// accents, a few symbols as their nearest ASCII; None for the rest (left out).
fn ascii(c: char) -> Option<char> {
    if (' '..='~').contains(&c) {
        return Some(c);
    }
    let folded = match c {
        'à'..='å' => 'a',
        'À'..='Å' => 'A',
        'ç' => 'c',
        'Ç' => 'C',
        'è'..='ë' => 'e',
        'È'..='Ë' => 'E',
        'ì'..='ï' => 'i',
        'Ì'..='Ï' => 'I',
        'ñ' => 'n',
        'Ñ' => 'N',
        'ò'..='ö' | 'ø' => 'o',
        'Ò'..='Ö' | 'Ø' => 'O',
        'ù'..='ü' => 'u',
        'Ù'..='Ü' => 'U',
        'ý' | 'ÿ' => 'y',
        'Ý' => 'Y',
        'ß' => 's',
        '♯' => '#',
        '♭' => 'b',
        '›' | '→' => '>',
        '‹' | '←' => '<',
        '–' | '—' => '-',
        '‘' | '’' => '\'',
        '“' | '”' => '"',
        '\t' => ' ',
        _ => return None,
    };
    Some(folded)
}

/// A name shortened to `width` characters as a scribble strip shows it, padded with spaces:
/// spaces go first, then lower-case vowels from the end (never the first letter), then the
/// tail. "Bass Guitar" is "BssGtr" in 6.
pub fn fit(name: &str, width: usize) -> String {
    let mut chars: Vec<char> = name.chars().filter_map(ascii).collect::<String>().trim().chars().collect();
    if chars.len() > width {
        chars.retain(|c| *c != ' ');
    }
    let mut i = chars.len();
    while chars.len() > width && i > 1 {
        i -= 1;
        if "aeiou".contains(chars[i]) {
            chars.remove(i);
        }
    }
    chars.truncate(width);
    let mut s: String = chars.into_iter().collect();
    while s.chars().count() < width {
        s.push(' ');
    }
    s
}

/// `s` centred in `width` characters (cut to it when longer).
pub fn center(s: &str, width: usize) -> String {
    let s: String = s.chars().take(width).collect();
    let n = s.chars().count();
    let left = (width - n) / 2;
    format!("{}{s}{}", " ".repeat(left), " ".repeat(width - n - left))
}

/// A level as a strip shows it: "-12.5", "+3.0", "-inf".
pub fn db_text(db: f32) -> String {
    if !db.is_finite() || db <= -96.0 {
        return "-inf".into();
    }
    let tenths = (db * 10.0).round() / 10.0;
    if tenths > 0.0 {
        format!("+{tenths:.1}")
    } else if tenths < 0.0 {
        format!("{tenths:.1}")
    } else {
        "0.0".into()
    }
}

/// A pan position (-1..1) as Logic writes it on a strip: "C", "L32", "R64".
pub fn pan_text(pan: f32) -> String {
    let v = (pan.clamp(-1.0, 1.0) * 64.0).round() as i32;
    match v {
        0 => "C".into(),
        v if v < 0 => format!("L{}", -v),
        v => format!("R{v}"),
    }
}

/// A normalized plug-in parameter (0..1) as a strip shows it: "56%".
pub fn percent_text(v: f32) -> String {
    format!("{:.0}%", v.clamp(0.0, 1.0) * 100.0)
}

/// How a V-Pot's ring of 11 lights shows a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingMode {
    /// One light at the position (pan).
    Dot = 0,
    /// Lit from the centre to the position.
    BoostCut = 1,
    /// Lit from the left to the position (levels, plug-in parameters).
    Wrap = 2,
    /// Lit out from the centre, both ways, as far as the position.
    Spread = 3,
}

/// A V-Pot ring's value, as both protocols have it: the mode in bits 4–5, the position 1..=11
/// in bits 0–3, the centre light in bit 6; 0 (all off) for None.
pub fn ring_value(mode: RingMode, v: Option<f32>, center_light: bool) -> u8 {
    let Some(v) = v.filter(|v| v.is_finite()) else { return 0 };
    let pos = 1 + (v.clamp(0.0, 1.0) * 10.0).round() as u8;
    (center_light as u8) << 6 | (mode as u8) << 4 | pos
}

/// A meter's level on a strip's 12 segments (0 is none), from a peak (linear): each segment
/// lights at its level in dB.
pub fn meter_level(peak: f32) -> u8 {
    const SEGMENTS: [f32; 12] = [-60.0, -50.0, -40.0, -30.0, -24.0, -18.0, -14.0, -10.0, -6.0, -4.0, -2.0, 0.0];
    if !peak.is_finite() || peak <= 0.0 {
        return 0;
    }
    let db = 20.0 * peak.log10();
    SEGMENTS.iter().take_while(|&&t| db >= t - 1e-4).count() as u8
}

/// One character of a seven-segment display and its dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Digit {
    pub ch: u8,
    pub dot: bool,
}

/// Characters as digits, leftmost first, with a dot after each position in `dots`.
fn digits(s: &str, dots: &[usize]) -> Vec<Digit> {
    s.bytes().enumerate().map(|(i, ch)| Digit { ch, dot: dots.contains(&i) }).collect()
}

/// Bars, beats, divisions and ticks on a Mackie Control's ten digits ("  5. 2. 3. 12"), as
/// Logic shows them.
pub fn bbt_digits(b: &Bbt) -> Vec<Digit> {
    let s = format!("{:>3}{:>2}{:>2}{:>3}", b.bar.clamp(-99, 999), b.beat.min(99), b.div.min(99), b.tick.min(999));
    digits(&s, &[2, 4, 6])
}

/// Hours, minutes, seconds and milliseconds on a Mackie Control's ten digits ("  0.01.23.456").
pub fn time_digits(secs: f64) -> Vec<Digit> {
    let ms = (secs.max(0.0) * 1000.0).floor() as u64;
    let s = format!("{:>3}{:02}{:02}{:03}", (ms / 3_600_000).min(999), ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    digits(&s, &[2, 4, 6])
}

/// Bars, beats and ticks (960 a beat) on a HUI's eight digits ("005.02.491"), as Pro Tools
/// shows them. Its display has digits only, so they're padded with zeros.
pub fn hui_bbt_digits(b: &Bbt) -> Vec<Digit> {
    let ticks = (b.div.saturating_sub(1) * 240 + b.tick.saturating_sub(1)).min(999);
    let s = format!("{:03}{:02}{:03}", b.bar.clamp(0, 999), b.beat.min(99), ticks);
    digits(&s, &[2, 4])
}

/// Hours, minutes, seconds and hundredths on a HUI's eight digits ("00.01.23.45").
pub fn hui_time_digits(secs: f64) -> Vec<Digit> {
    let cs = (secs.max(0.0) * 100.0).floor() as u64;
    let s = format!("{:02}{:02}{:02}{:02}", (cs / 360_000).min(99), cs / 6000 % 60, cs / 100 % 60, cs % 100);
    digits(&s, &[1, 3, 5])
}

/// Steps from a relative encoder whose bit 6 is the sign and bits 0–5 the count; `ccw_bit`
/// says whether the set bit means counter-clockwise (a Mackie Control) or clockwise (a HUI).
pub fn encoder_steps(v: u8, ccw_bit: bool) -> i32 {
    let n = (v & 0x3F) as i32;
    if (v & 0x40 != 0) == ccw_bit {
        -n
    } else {
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_fit_a_strip() {
        assert_eq!(fit("Kick", 6), "Kick  ");
        assert_eq!(fit("Vocals", 6), "Vocals");
        assert_eq!(fit("Bass Guitar", 6), "BssGtr");
        assert_eq!(fit("Track 12", 6), "Trck12");
        assert_eq!(fit("Café Piano", 6), "CafePn");
        assert_eq!(fit("Drum Kit", 4), "DrmK");
        assert_eq!(fit("  ", 4), "    ");
        // Only printable ASCII goes out.
        assert!(fit("Bass › Synth ♯ 中", 7).bytes().all(|b| (0x20..0x7F).contains(&b)));
        assert_eq!(fit("Strings", 7), "Strings");
    }

    #[test]
    fn values_as_text() {
        assert_eq!(db_text(-12.46), "-12.5");
        assert_eq!(db_text(3.0), "+3.0");
        assert_eq!(db_text(0.0), "0.0");
        assert_eq!(db_text(-0.01), "0.0");
        assert_eq!(db_text(-96.0), "-inf");
        assert_eq!(db_text(f32::NEG_INFINITY), "-inf");
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-0.5), "L32");
        assert_eq!(pan_text(1.0), "R64");
        assert_eq!(percent_text(0.555), "56%");
        assert_eq!(center("C", 6), "  C   ");
        assert_eq!(center("-12.5dB", 6), "-12.5d");
    }

    #[test]
    fn banks_stay_full() {
        assert_eq!(clamp_bank(0, 20, 16), 0);
        assert_eq!(clamp_bank(16, 20, 16), 4);
        assert_eq!(clamp_bank(-8, 20, 16), 0);
        assert_eq!(clamp_bank(3, 5, 8), 0, "fewer tracks than strips");
        let unit = |offset| SurfaceConfig { offset, ..Default::default() };
        assert_eq!(strips_of(&[]), 8);
        assert_eq!(strips_of(&[unit(0), unit(8)]), 16);
        assert_eq!(strips_of(&[unit(8), unit(0), unit(16)]), 24);
        assert_eq!(next_offset(&[]), 0);
        assert_eq!(next_offset(&[unit(0), unit(8)]), 16);
    }

    #[test]
    fn rings() {
        assert_eq!(ring_value(RingMode::Dot, None, false), 0);
        assert_eq!(ring_value(RingMode::Dot, Some(0.5), false), 0x06, "centred pan: the middle light");
        assert_eq!(ring_value(RingMode::Dot, Some(0.0), false), 0x01);
        assert_eq!(ring_value(RingMode::Wrap, Some(1.0), false), 0x2B);
        assert_eq!(ring_value(RingMode::Spread, Some(0.5), true), 0x76);
        assert_eq!(ring_value(RingMode::BoostCut, Some(f32::NAN), false), 0);
    }

    #[test]
    fn meter_segments() {
        assert_eq!(meter_level(0.0), 0);
        assert_eq!(meter_level(f32::NAN), 0);
        assert_eq!(meter_level(0.0005), 0, "-66 dB");
        assert_eq!(meter_level(0.001), 1, "-60 dB");
        assert_eq!(meter_level(0.5), 8, "-6.02 dB");
        assert_eq!(meter_level(0.502), 9, "-5.99 dB");
        assert_eq!(meter_level(1.0), 12);
        assert_eq!(meter_level(4.0), 12);
    }

    fn text(d: &[Digit]) -> String {
        d.iter().map(|d| format!("{}{}", d.ch as char, if d.dot { "." } else { "" })).collect()
    }

    #[test]
    fn time_display() {
        let b = Bbt { bar: 5, beat: 2, div: 3, tick: 12 };
        assert_eq!(text(&bbt_digits(&b)), "  5. 2. 3. 12");
        assert_eq!(bbt_digits(&b).len(), 10);
        assert_eq!(text(&bbt_digits(&Bbt { bar: 1234, beat: 1, div: 1, tick: 1 })), "999. 1. 1.  1");
        assert_eq!(text(&time_digits(83.456)), "  0.01.23.456");
        assert_eq!(text(&time_digits(3600.0 * 2.0 + 1.0)), "  2.00.01.000");
        assert_eq!(text(&hui_bbt_digits(&b)), "005.02.491");
        assert_eq!(hui_bbt_digits(&b).len(), 8);
        assert_eq!(text(&hui_time_digits(83.456)), "00.01.23.45");
    }

    #[test]
    fn encoders() {
        // A Mackie Control: bit 6 is counter-clockwise.
        assert_eq!(encoder_steps(0x01, true), 1);
        assert_eq!(encoder_steps(0x05, true), 5);
        assert_eq!(encoder_steps(0x41, true), -1);
        assert_eq!(encoder_steps(0x4F, true), -15);
        // A HUI: bit 6 is clockwise.
        assert_eq!(encoder_steps(0x41, false), 1);
        assert_eq!(encoder_steps(0x01, false), -1);
    }

    #[test]
    fn settings_read_with_defaults() {
        let c: SurfaceConfig = serde_json::from_str(r#"{"kind":"mcu_extender","input":"X-Touch Ext"}"#).unwrap();
        assert_eq!(c.kind, SurfaceKind::McuExtender);
        assert_eq!(c.output, "");
        assert_eq!(c.offset, 0);
        assert!(c.functions.is_empty());
        assert!(!c.kind.is_main());
        assert!(SurfaceKind::Hui.is_main());
    }
}
