//! The Reverb's parameter ids, ranges, factory presets and registry entry, and the limits they refer
//! to. Ids are saved in projects and never change.

use super::*;

pub const ROOM: u32 = 0;
pub const PREDELAY: u32 = 1;
pub const SYNC: u32 = 2;
pub const PREDELAY_NOTE: u32 = 3;
pub const SIZE: u32 = 4;
/// RT60 in ms (the contract has no seconds unit).
pub const DECAY: u32 = 5;
pub const DENSITY: u32 = 6;
pub const DIFFUSION: u32 = 7;
/// How much faster the highs decay than the mids.
pub const DAMPING: u32 = 8;
/// The low band's RT60 as a multiple of the mid RT60.
pub const LOW_DECAY: u32 = 9;
/// 0 = early reflections only, 0.5 = both at full level, 1 = late reverb only.
pub const BALANCE: u32 = 10;
pub const MOD_RATE: u32 = 11;
pub const MOD_DEPTH: u32 = 12;
pub const WIDTH: u32 = 13;
pub const LOW_CUT: u32 = 14;
pub const HIGH_CUT: u32 = 15;
pub const FREEZE: u32 = 16;
pub const DRY: u32 = 17;
pub const WET: u32 = 18;

pub const ROOM_TYPES: &[&str] = &["Plate", "Room", "Hall", "Chamber", "Cathedral", "Ambience"];
pub(super) const PLATE: usize = 0;

/// The longest pre-delay (synced values are clamped to it).
pub(super) const MAX_PREDELAY_MS: f32 = 1000.0;
/// Size maps to this scale of the room's nominal dimensions: 0.35× .. 1.65×, 1× at 50 %.
pub(super) const SIZE_MIN: f32 = 0.35;
pub(super) const SIZE_SPAN: f32 = 1.3;
pub(super) const SIZE_MAX: f32 = SIZE_MIN + SIZE_SPAN;
/// The largest modulation excursion of the network's lines.
pub(super) const MOD_MS: f32 = 1.0;
/// Crossover frequencies of the two-band decay filter.
pub(super) const LOW_XOVER_HZ: f32 = 250.0;
pub(super) const HIGH_XOVER_HZ: f32 = 3500.0;
/// Freeze's RT60: an hour, infinite in practice, but finite so the loop stays (just) lossy.
/// The modulated lines' interpolation still darkens a frozen sound slowly.
pub(super) const FREEZE_RT60: f32 = 3600.0;
pub(super) const MAX_TAIL_SECS: f32 = 30.0;
/// How fast a size change may move a network line (samples per sample) and the size scale (per
/// sample), so sweeping Size bends the pitch by at most about 5 % instead of clicking.
pub(super) const LINE_SLEW: f32 = 0.05;
pub(super) const SCALE_SLEW: f32 = 1e-5;
/// The fade when the room type changes, which clears the old tail.
pub(super) const TYPE_FADE_MS: f32 = 15.0;

// One argument per `ParamSpec` field, so the table below reads as rows.
#[allow(clippy::too_many_arguments)]
const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, kind: ParamKind, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind, log }
}

const C: ParamKind = ParamKind::Continuous;

pub(super) static PARAMS: [ParamSpec; 19] = [
    spec(ROOM, "Room Type", 0.0, 5.0, 2.0, Unit::None, ParamKind::Choice(ROOM_TYPES), false),
    spec(PREDELAY, "Pre-delay", 0.0, 500.0, 20.0, Unit::Ms, C, false),
    spec(SYNC, "Sync", 0.0, 1.0, 0.0, Unit::None, ParamKind::Toggle, false),
    spec(PREDELAY_NOTE, "Pre-delay Note", 0.0, 17.0, 12.0, Unit::Note, ParamKind::Choice(NOTE_LABELS), false),
    spec(SIZE, "Size", 0.0, 1.0, 0.5, Unit::Percent, C, false),
    spec(DECAY, "Decay", 100.0, 20_000.0, 2000.0, Unit::Ms, C, true),
    spec(DENSITY, "Density", 0.0, 1.0, 0.7, Unit::Percent, C, false),
    spec(DIFFUSION, "Diffusion", 0.0, 1.0, 0.7, Unit::Percent, C, false),
    spec(DAMPING, "Damping", 0.0, 1.0, 0.4, Unit::Percent, C, false),
    spec(LOW_DECAY, "Low Decay", 0.5, 2.0, 1.0, Unit::None, C, true),
    spec(BALANCE, "Early/Late", 0.0, 1.0, 0.5, Unit::Percent, C, false),
    spec(MOD_RATE, "Mod Rate", 0.05, 5.0, 0.6, Unit::Hz, C, true),
    spec(MOD_DEPTH, "Mod Depth", 0.0, 1.0, 0.35, Unit::Percent, C, false),
    spec(WIDTH, "Width", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(LOW_CUT, "Low Cut", 20.0, 1000.0, 30.0, Unit::Hz, C, true),
    spec(HIGH_CUT, "High Cut", 1000.0, 20_000.0, 16_000.0, Unit::Hz, C, true),
    spec(FREEZE, "Freeze", 0.0, 1.0, 0.0, Unit::None, ParamKind::Toggle, false),
    spec(DRY, "Dry", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(WET, "Wet", 0.0, 1.0, 0.3, Unit::Percent, C, false),
];

/// Factory presets. Room types: 0 Plate, 1 Room, 2 Hall, 3 Chamber, 4 Cathedral, 5 Ambience.
#[rustfmt::skip] // One row per preset's values: split per value, these tables run to thousands of lines.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Vocal Plate",
        values: &[(ROOM, 0.0), (PREDELAY, 35.0), (DECAY, 1800.0), (DAMPING, 0.45), (LOW_CUT, 150.0), (HIGH_CUT, 11_000.0), (WET, 0.25)],
    },
    FactoryPreset {
        name: "Bright Plate",
        values: &[(ROOM, 0.0), (PREDELAY, 10.0), (DECAY, 2400.0), (DAMPING, 0.1), (DIFFUSION, 0.8), (HIGH_CUT, 18_000.0), (LOW_CUT, 120.0)],
    },
    FactoryPreset {
        name: "Snare Plate",
        values: &[(ROOM, 0.0), (PREDELAY, 5.0), (SIZE, 0.4), (DECAY, 1300.0), (DENSITY, 0.9), (DAMPING, 0.3), (LOW_CUT, 200.0), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Small Room",
        values: &[(ROOM, 1.0), (PREDELAY, 4.0), (SIZE, 0.3), (DECAY, 600.0), (DAMPING, 0.5), (BALANCE, 0.4), (WET, 0.25)],
    },
    FactoryPreset {
        name: "Drum Room",
        values: &[(ROOM, 1.0), (PREDELAY, 8.0), (SIZE, 0.6), (DECAY, 1100.0), (DENSITY, 0.85), (DAMPING, 0.35), (BALANCE, 0.35), (LOW_DECAY, 0.8), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Short Slap Room",
        values: &[(ROOM, 1.0), (PREDELAY, 0.0), (SIZE, 0.15), (DECAY, 350.0), (DIFFUSION, 0.4), (BALANCE, 0.2), (HIGH_CUT, 9000.0), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Warm Hall",
        values: &[(ROOM, 2.0), (PREDELAY, 25.0), (SIZE, 0.6), (DECAY, 2600.0), (DAMPING, 0.65), (LOW_DECAY, 1.3), (HIGH_CUT, 8000.0), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Concert Hall",
        values: &[(ROOM, 2.0), (PREDELAY, 30.0), (SIZE, 0.75), (DECAY, 2200.0), (DAMPING, 0.4), (LOW_DECAY, 1.15), (BALANCE, 0.55), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Chamber",
        values: &[(ROOM, 3.0), (PREDELAY, 15.0), (SIZE, 0.5), (DECAY, 1500.0), (DENSITY, 0.8), (DAMPING, 0.4), (WET, 0.28)],
    },
    FactoryPreset {
        name: "Big Cathedral",
        values: &[(ROOM, 4.0), (PREDELAY, 60.0), (SIZE, 0.85), (DECAY, 7000.0), (DAMPING, 0.5), (LOW_DECAY, 1.25), (MOD_DEPTH, 0.45), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Ambience Glue",
        values: &[(ROOM, 5.0), (PREDELAY, 0.0), (SIZE, 0.4), (DECAY, 450.0), (DENSITY, 0.9), (BALANCE, 0.45), (WIDTH, 0.8), (WET, 0.15)],
    },
    FactoryPreset {
        name: "Long Ambient Pad",
        values: &[(ROOM, 4.0), (PREDELAY, 80.0), (SIZE, 1.0), (DECAY, 14_000.0), (DAMPING, 0.55), (MOD_RATE, 0.3), (MOD_DEPTH, 0.7), (BALANCE, 0.85), (LOW_CUT, 120.0), (WET, 0.45)],
    },
    FactoryPreset {
        name: "Synced Pre-delay Hall",
        values: &[(ROOM, 2.0), (SYNC, 1.0), (PREDELAY_NOTE, 12.0), (DECAY, 2400.0), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Frozen Pad",
        values: &[(ROOM, 2.0), (SIZE, 0.9), (DECAY, 8000.0), (MOD_DEPTH, 0.6), (BALANCE, 1.0), (FREEZE, 1.0), (DRY, 0.0), (WET, 0.7)],
    },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:reverb",
        name: "Reverb",
        category: "Reverb",
        params: &PARAMS,
        meters: &[],
        sidechain: false,
        new: |sample_rate| Box::new(Reverb::new(sample_rate)),
    }
}
