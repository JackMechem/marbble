//! A CLAP plug-in's features ("audio-effect", "compressor", "stereo"…) in the VST3 subcategory
//! terms the menus group by ("Fx|Dynamics", "Instrument|Synth").

/// Feature names and the subcategory each puts a plug-in in, the first found winning.
const EFFECTS: &[(&str, &str)] = &[
    ("compressor", "Dynamics"),
    ("limiter", "Dynamics"),
    ("expander", "Dynamics"),
    ("gate", "Dynamics"),
    ("transient-shaper", "Dynamics"),
    ("de-esser", "Dynamics"),
    ("equalizer", "EQ"),
    ("filter", "Filter"),
    ("reverb", "Reverb"),
    ("delay", "Delay"),
    ("distortion", "Distortion"),
    ("chorus", "Modulation"),
    ("flanger", "Modulation"),
    ("phaser", "Modulation"),
    ("tremolo", "Modulation"),
    ("frequency-shifter", "Modulation"),
    ("pitch-shifter", "Pitch Shift"),
    ("pitch-correction", "Pitch Shift"),
    ("analyzer", "Analyzer"),
    ("restoration", "Restoration"),
    ("mastering", "Mastering"),
    ("surround", "Spatial"),
    ("ambisonic", "Spatial"),
    ("utility", "Tools"),
    ("mixing", "Tools"),
];

const INSTRUMENTS: &[(&str, &str)] = &[("synthesizer", "Synth"), ("sampler", "Sampler"), ("drum", "Drum"), ("drum-machine", "Drum")];

pub(crate) fn is_instrument(features: &[String]) -> bool {
    features.iter().any(|f| f == "instrument")
}

pub(crate) fn subcategories(features: &[String]) -> String {
    let find = |table: &[(&str, &'static str)]| table.iter().find(|(f, _)| features.iter().any(|x| x == f)).map(|(_, c)| *c);
    if is_instrument(features) {
        return find(INSTRUMENTS).map_or("Instrument".into(), |c| format!("Instrument|{c}"));
    }
    find(EFFECTS).map_or("Fx".into(), |c| format!("Fx|{c}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn features_become_subcategories() {
        assert_eq!(subcategories(&f(&["audio-effect", "compressor", "stereo"])), "Fx|Dynamics");
        assert_eq!(subcategories(&f(&["audio-effect", "stereo"])), "Fx");
        assert_eq!(subcategories(&f(&["instrument", "sampler", "stereo"])), "Instrument|Sampler");
        assert_eq!(subcategories(&f(&["instrument"])), "Instrument");
        assert!(is_instrument(&f(&["instrument", "synthesizer"])));
        assert!(!is_instrument(&f(&["audio-effect", "reverb"])));
    }
}
