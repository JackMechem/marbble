//! The Delay's parameter ids, ranges, factory presets and registry entry. Ids are saved in projects
//! and never change.

use super::*;

pub const MODE: u32 = 0;
pub const SYNC: u32 = 1;
pub const TIME_L: u32 = 2;
pub const TIME_R: u32 = 3;
pub const NOTE_L: u32 = 4;
pub const NOTE_R: u32 = 5;
/// The right channel's time and feedback follow the left's.
pub const LINK: u32 = 6;
pub const FEEDBACK_L: u32 = 7;
pub const FEEDBACK_R: u32 = 8;
pub const CROSSFEED: u32 = 9;
pub const LOW_CUT: u32 = 10;
pub const HIGH_CUT: u32 = 11;
pub const MOD_RATE: u32 = 12;
pub const MOD_DEPTH: u32 = 13;
pub const WOW: u32 = 14;
pub const FLUTTER: u32 = 15;
pub const SATURATION: u32 = 16;
pub const SMOOTH: u32 = 17;
pub const WIDTH: u32 = 18;
pub const DRY: u32 = 19;
pub const WET: u32 = 20;
pub const FREEZE: u32 = 21;

pub const MODES: &[&str] = &["Stereo", "Ping-Pong", "Tape", "Digital"];
/// The default mode, which the code handles as "not one of the others".
#[cfg_attr(not(test), allow(dead_code))]
pub(super) const STEREO: usize = 0;
pub(super) const PING_PONG: usize = 1;
pub(super) const TAPE: usize = 2;
pub(super) const DIGITAL: usize = 3;

/// The longest delay time, in ms (synced times are clamped to it).
pub const MAX_TIME_MS: f32 = 4000.0;
/// The chorus modulation's largest excursion.
pub(super) const MOD_MS: f32 = 3.0;
/// Extra room in the lines for modulation, wow and flutter.
pub(super) const MARGIN_MS: f32 = 30.0;
/// The crossfade between read heads when the time changes in the clean modes.
pub(super) const XFADE_MS: f32 = 25.0;
/// Low and high cut at these extremes are off.
pub(super) const LOW_CUT_OFF: f32 = 20.0;
pub(super) const HIGH_CUT_OFF: f32 = 20_000.0;
/// Tape mode's loop gain at 100 % feedback: a little over unity, so it can run away into
/// self-oscillation, which the saturation then limits.
pub(super) const TAPE_OVERDRIVE: f32 = 1.1;
/// Digital mode's feedback ceiling.
pub(super) const DIGITAL_MAX_FEEDBACK: f32 = 0.98;
pub(super) const MAX_TAIL_SECS: f32 = 30.0;

// One argument per `ParamSpec` field, so the table below reads as rows.
#[allow(clippy::too_many_arguments)]
const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, kind: ParamKind, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind, log }
}

const C: ParamKind = ParamKind::Continuous;
const T: ParamKind = ParamKind::Toggle;
const NOTE: ParamKind = ParamKind::Choice(NOTE_LABELS);

pub(super) static PARAMS: [ParamSpec; 22] = [
    spec(MODE, "Mode", 0.0, 3.0, 0.0, Unit::None, ParamKind::Choice(MODES), false),
    spec(SYNC, "Sync", 0.0, 1.0, 1.0, Unit::None, T, false),
    spec(TIME_L, "Time L", 1.0, MAX_TIME_MS, 500.0, Unit::Ms, C, true),
    spec(TIME_R, "Time R", 1.0, MAX_TIME_MS, 375.0, Unit::Ms, C, true),
    spec(NOTE_L, "Note L", 0.0, 17.0, 6.0, Unit::Note, NOTE, false),
    spec(NOTE_R, "Note R", 0.0, 17.0, 10.0, Unit::Note, NOTE, false),
    spec(LINK, "Link", 0.0, 1.0, 0.0, Unit::None, T, false),
    spec(FEEDBACK_L, "Feedback L", 0.0, 1.0, 0.35, Unit::Percent, C, false),
    spec(FEEDBACK_R, "Feedback R", 0.0, 1.0, 0.35, Unit::Percent, C, false),
    spec(CROSSFEED, "Cross-feed", 0.0, 1.0, 0.0, Unit::Percent, C, false),
    spec(LOW_CUT, "Low Cut", LOW_CUT_OFF, 2000.0, LOW_CUT_OFF, Unit::Hz, C, true),
    spec(HIGH_CUT, "High Cut", 500.0, HIGH_CUT_OFF, HIGH_CUT_OFF, Unit::Hz, C, true),
    spec(MOD_RATE, "Mod Rate", 0.05, 10.0, 0.8, Unit::Hz, C, true),
    spec(MOD_DEPTH, "Mod Depth", 0.0, 1.0, 0.0, Unit::Percent, C, false),
    spec(WOW, "Wow", 0.0, 1.0, 0.3, Unit::Percent, C, false),
    spec(FLUTTER, "Flutter", 0.0, 1.0, 0.2, Unit::Percent, C, false),
    spec(SATURATION, "Saturation", 0.0, 1.0, 0.3, Unit::Percent, C, false),
    spec(SMOOTH, "Smooth", 0.0, 1.0, 0.5, Unit::Percent, C, false),
    spec(WIDTH, "Width", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(DRY, "Dry", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(WET, "Wet", 0.0, 1.0, 0.35, Unit::Percent, C, false),
    spec(FREEZE, "Freeze", 0.0, 1.0, 0.0, Unit::None, T, false),
];

/// Factory presets. Modes: 0 Stereo, 1 Ping-Pong, 2 Tape, 3 Digital. Note indices follow
/// [`NOTE_LABELS`] (6 = 1/4, 9 = 1/8, 10 = 1/8 dotted, 11 = 1/8 triplet, 14 = 1/16 triplet).
#[rustfmt::skip] // One row per preset's values: split per value, these tables run to thousands of lines.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Quarter Note",
        values: &[(MODE, 3.0), (SYNC, 1.0), (NOTE_L, 6.0), (FEEDBACK_L, 0.35), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Dotted Eighth (U2)",
        values: &[(MODE, 3.0), (SYNC, 1.0), (NOTE_L, 10.0), (FEEDBACK_L, 0.45), (HIGH_CUT, 7000.0), (LOW_CUT, 150.0), (WET, 0.4)],
    },
    FactoryPreset {
        name: "Slapback",
        values: &[(MODE, 2.0), (SYNC, 0.0), (TIME_L, 95.0), (LINK, 1.0), (FEEDBACK_L, 0.08), (WOW, 0.15), (FLUTTER, 0.1), (SATURATION, 0.4), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Ping-Pong Eighths",
        values: &[(MODE, 1.0), (SYNC, 1.0), (NOTE_L, 9.0), (NOTE_R, 9.0), (FEEDBACK_L, 0.5), (HIGH_CUT, 9000.0), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Tape Echo",
        values: &[(MODE, 2.0), (SYNC, 1.0), (NOTE_L, 10.0), (LINK, 1.0), (FEEDBACK_L, 0.45), (WOW, 0.35), (FLUTTER, 0.25), (SATURATION, 0.45), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Dub Delay",
        values: &[
            (MODE, 2.0),
            (SYNC, 1.0),
            (NOTE_L, 10.0),
            (NOTE_R, 6.0),
            (FEEDBACK_L, 0.85),
            (FEEDBACK_R, 0.85),
            (CROSSFEED, 0.35),
            (LOW_CUT, 250.0),
            (HIGH_CUT, 3500.0),
            (SATURATION, 0.6),
            (WOW, 0.4),
            (WET, 0.45),
        ],
    },
    FactoryPreset {
        name: "Wide Doubler",
        values: &[(MODE, 0.0), (SYNC, 0.0), (TIME_L, 13.0), (TIME_R, 21.0), (FEEDBACK_L, 0.0), (FEEDBACK_R, 0.0), (MOD_RATE, 0.6), (MOD_DEPTH, 0.35), (WET, 0.5)],
    },
    FactoryPreset {
        name: "Vocal Throw",
        values: &[(MODE, 0.0), (SYNC, 1.0), (NOTE_L, 6.0), (NOTE_R, 10.0), (FEEDBACK_L, 0.4), (FEEDBACK_R, 0.4), (LOW_CUT, 300.0), (HIGH_CUT, 6000.0), (DRY, 1.0), (WET, 0.5)],
    },
    FactoryPreset {
        name: "Lo-Fi Tape",
        values: &[(MODE, 2.0), (SYNC, 1.0), (NOTE_L, 9.0), (LINK, 1.0), (FEEDBACK_L, 0.55), (WOW, 0.8), (FLUTTER, 0.7), (SATURATION, 0.85), (HIGH_CUT, 4000.0), (LOW_CUT, 200.0), (SMOOTH, 0.2), (WET, 0.4)],
    },
    FactoryPreset {
        name: "Triplet Bounce",
        values: &[(MODE, 1.0), (SYNC, 1.0), (NOTE_L, 11.0), (NOTE_R, 11.0), (FEEDBACK_L, 0.45), (WIDTH, 0.9), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Sixteenth Shimmer",
        values: &[(MODE, 0.0), (SYNC, 1.0), (NOTE_L, 12.0), (NOTE_R, 14.0), (FEEDBACK_L, 0.55), (FEEDBACK_R, 0.5), (CROSSFEED, 0.5), (MOD_DEPTH, 0.25), (LOW_CUT, 400.0), (WET, 0.3)],
    },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:delay",
        name: "Delay",
        category: "Delay",
        params: &PARAMS,
        meters: &[],
        sidechain: false,
        new: |sample_rate| Box::new(Delay::new(sample_rate)),
    }
}
