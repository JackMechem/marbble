//! The Compressor's id, parameters, factory presets, meters and registry entry. Parameter ids are
//! saved in projects and never change.

use super::*;

/// The plug-in id.
pub const ID: &str = "builtin:compressor";

/// Parameter ids (stable forever: saved projects and automation refer to them).
pub mod param {
    pub const CIRCUIT: u32 = 0;
    pub const THRESHOLD: u32 = 1;
    pub const RATIO: u32 = 2;
    pub const KNEE: u32 = 3;
    pub const ATTACK: u32 = 4;
    pub const RELEASE: u32 = 5;
    pub const AUTO_RELEASE: u32 = 6;
    pub const MAKEUP: u32 = 7;
    pub const AUTO_GAIN: u32 = 8;
    pub const DETECTOR: u32 = 9;
    pub const DISTORTION: u32 = 10;
    pub const MIX: u32 = 11;
    pub const OUTPUT: u32 = 12;
    pub const LINK: u32 = 13;
    pub const SC_MODE: u32 = 14;
    pub const SC_FREQ: u32 = 15;
    pub const LISTEN: u32 = 16;
    pub const LOOKAHEAD: u32 = 17;
    pub const FET_RATIO: u32 = 18;
    pub const OPTO_MODE: u32 = 19;
    pub const TIME_CONSTANT: u32 = 20;
}

pub(super) const NPARAMS: usize = 21;

// One argument per `ParamSpec` field, so the table below reads as rows.
#[allow(clippy::too_many_arguments)]
const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, kind: ParamKind, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind, log }
}

const C: ParamKind = ParamKind::Continuous;
const T: ParamKind = ParamKind::Toggle;

pub(super) static PARAMS: [ParamSpec; NPARAMS] = [
    spec(
        param::CIRCUIT,
        "Circuit",
        0.0,
        4.0,
        0.0,
        Unit::None,
        ParamKind::Choice(&["Platinum Digital", "Studio VCA", "Studio FET", "Classic Opto", "Vintage Vari-Mu"]),
        false,
    ),
    spec(param::THRESHOLD, "Threshold", -60.0, 0.0, -20.0, Unit::Db, C, false),
    spec(param::RATIO, "Ratio", 1.0, 30.0, 4.0, Unit::Ratio, C, true),
    spec(param::KNEE, "Knee", 0.0, 24.0, 3.0, Unit::Db, C, false),
    spec(param::ATTACK, "Attack", 0.02, 200.0, 10.0, Unit::Ms, C, true),
    spec(param::RELEASE, "Release", 5.0, 5000.0, 150.0, Unit::Ms, C, true),
    spec(param::AUTO_RELEASE, "Auto Release", 0.0, 1.0, 0.0, Unit::None, T, false),
    spec(param::MAKEUP, "Make Up", -20.0, 40.0, 0.0, Unit::Db, C, false),
    spec(param::AUTO_GAIN, "Auto Gain", 0.0, 3.0, 0.0, Unit::None, ParamKind::Choice(&["Off", "0 dB", "-12 dB", "-24 dB"]), false),
    spec(param::DETECTOR, "Detection", 0.0, 1.0, 0.0, Unit::None, ParamKind::Choice(&["Peak", "RMS"]), false),
    spec(param::DISTORTION, "Distortion", 0.0, 3.0, 0.0, Unit::None, ParamKind::Choice(&["Off", "Soft", "Hard", "Clip"]), false),
    spec(param::MIX, "Mix", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(param::OUTPUT, "Output", -30.0, 12.0, 0.0, Unit::Db, C, false),
    spec(param::LINK, "Stereo Link", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(param::SC_MODE, "Side Chain Filter", 0.0, 3.0, 0.0, Unit::None, ParamKind::Choice(&["Off", "HP", "LP", "BP"]), false),
    spec(param::SC_FREQ, "Side Chain Freq", 20.0, 20000.0, 150.0, Unit::Hz, C, true),
    spec(param::LISTEN, "Listen", 0.0, 1.0, 0.0, Unit::None, T, false),
    spec(param::LOOKAHEAD, "Lookahead", 0.0, 20.0, 0.0, Unit::Ms, C, false),
    spec(param::FET_RATIO, "FET Ratio", 0.0, 4.0, 0.0, Unit::None, ParamKind::Choice(&["4:1", "8:1", "12:1", "20:1", "All"]), false),
    spec(param::OPTO_MODE, "Opto Mode", 0.0, 1.0, 0.0, Unit::None, ParamKind::Choice(&["Compress", "Limit"]), false),
    spec(
        param::TIME_CONSTANT,
        "Time Constant",
        0.0,
        5.0,
        0.0,
        Unit::None,
        ParamKind::Choice(&["1 (0.3 s)", "2 (0.8 s)", "3 (2 s)", "4 (5 s)", "5 (2–10 s)", "6 (0.3–25 s)"]),
        false,
    ),
];

/// Factory presets. Each lists only what it changes from the defaults.
pub const PRESETS: &[FactoryPreset] = {
    use param::*;
    &[
        FactoryPreset {
            name: "Vocal Leveler (Opto)",
            values: &[(CIRCUIT, 3.0), (THRESHOLD, -24.0), (OPTO_MODE, 0.0), (MAKEUP, 5.0)],
        },
        FactoryPreset {
            name: "Vocal Aggressive (FET)",
            values: &[(CIRCUIT, 2.0), (THRESHOLD, -26.0), (FET_RATIO, 1.0), (ATTACK, 0.2), (RELEASE, 120.0), (MAKEUP, 7.0)],
        },
        FactoryPreset {
            name: "Drum Bus Glue (VCA)",
            values: &[
                (CIRCUIT, 1.0),
                (THRESHOLD, -18.0),
                (RATIO, 4.0),
                (KNEE, 2.0),
                (ATTACK, 10.0),
                (RELEASE, 100.0),
                (DETECTOR, 1.0),
                (MAKEUP, 3.0),
                (SC_MODE, 1.0),
                (SC_FREQ, 80.0),
            ],
        },
        FactoryPreset {
            name: "Mix Bus Glue (VCA)",
            values: &[
                (CIRCUIT, 1.0),
                (THRESHOLD, -14.0),
                (RATIO, 2.0),
                (KNEE, 4.0),
                (ATTACK, 30.0),
                (AUTO_RELEASE, 1.0),
                (DETECTOR, 1.0),
                (MAKEUP, 1.5),
                (SC_MODE, 1.0),
                (SC_FREQ, 60.0),
            ],
        },
        FactoryPreset {
            // A slow attack lets the beater through before the VCA clamps down.
            name: "Kick Punch (VCA)",
            values: &[(CIRCUIT, 1.0), (THRESHOLD, -16.0), (RATIO, 4.0), (KNEE, 0.0), (ATTACK, 30.0), (RELEASE, 100.0), (MAKEUP, 3.0)],
        },
        FactoryPreset {
            name: "Snare Smack (FET)",
            values: &[(CIRCUIT, 2.0), (THRESHOLD, -24.0), (FET_RATIO, 0.0), (ATTACK, 0.8), (RELEASE, 60.0), (MAKEUP, 5.0)],
        },
        FactoryPreset {
            name: "1176 All Buttons",
            values: &[(CIRCUIT, 2.0), (FET_RATIO, 4.0), (THRESHOLD, -30.0), (ATTACK, 0.8), (RELEASE, 60.0), (MAKEUP, 6.0)],
        },
        FactoryPreset {
            name: "Bass Even Out (Opto)",
            values: &[(CIRCUIT, 3.0), (THRESHOLD, -22.0), (OPTO_MODE, 0.0), (MAKEUP, 4.0), (SC_MODE, 1.0), (SC_FREQ, 40.0)],
        },
        FactoryPreset {
            name: "Acoustic Gentle (Platinum)",
            values: &[
                (THRESHOLD, -22.0),
                (RATIO, 2.0),
                (KNEE, 9.0),
                (ATTACK, 15.0),
                (RELEASE, 250.0),
                (AUTO_RELEASE, 1.0),
                (DETECTOR, 1.0),
                (MAKEUP, 2.0),
            ],
        },
        FactoryPreset {
            // Crush hard, then blend under the dry drums (New York compression).
            name: "Parallel Drums (Mix 40 %)",
            values: &[
                (CIRCUIT, 2.0),
                (FET_RATIO, 3.0),
                (THRESHOLD, -35.0),
                (ATTACK, 0.1),
                (RELEASE, 150.0),
                (MAKEUP, 10.0),
                (DISTORTION, 1.0),
                (MIX, 0.4),
            ],
        },
        FactoryPreset {
            name: "Fairchild Warmth (Vari-Mu)",
            values: &[(CIRCUIT, 4.0), (THRESHOLD, -20.0), (RATIO, 3.0), (TIME_CONSTANT, 1.0), (MAKEUP, 3.0)],
        },
        FactoryPreset {
            name: "Vari-Mu Bus (Program TC 5)",
            values: &[(CIRCUIT, 4.0), (THRESHOLD, -16.0), (RATIO, 2.0), (TIME_CONSTANT, 4.0), (MAKEUP, 1.5)],
        },
        FactoryPreset {
            name: "Transparent Peak Control (Platinum)",
            values: &[(THRESHOLD, -10.0), (RATIO, 8.0), (KNEE, 2.0), (ATTACK, 1.0), (RELEASE, 80.0), (LOOKAHEAD, 2.0)],
        },
        FactoryPreset {
            // Pick the kick (or voice-over) track as the side-chain source; the low-pass keeps
            // the detector on its low end.
            name: "Side-chain Duck",
            values: &[(THRESHOLD, -30.0), (RATIO, 6.0), (KNEE, 3.0), (ATTACK, 2.0), (RELEASE, 180.0), (SC_MODE, 2.0), (SC_FREQ, 150.0)],
        },
    ]
};

/// Meter names, in the order [`Effect::meters`] writes them. Input and Output are peak dBFS,
/// Gain Reduction is positive dB, and Level is the detector's level in dB (the x position of
/// the dot on the panel's transfer curve).
pub const METERS: &[&str] = &["Input", "Output", "Gain Reduction", "Level"];

/// The registry entry.
pub fn info() -> EffectInfo {
    EffectInfo {
        id: ID,
        name: "Compressor",
        category: "Dynamics",
        params: &PARAMS,
        meters: METERS,
        sidechain: true,
        new: |sr| Box::new(Compressor::new(sr)),
    }
}
