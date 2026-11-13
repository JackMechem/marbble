//! Marbble Synth (`crate::instrument::basic::synth::BasicSynth`, untouched) in the instrument registry: no parameters,
//! no panel. Its id and sound are what every existing project holds, so it stays as it always was.

use vst3_host::MidiEvent;

pub mod synth;

use super::{FxContext, Instrument, InstrumentInfo};
use crate::instrument::basic::synth::BasicSynth;

struct Basic(BasicSynth);

impl Instrument for Basic {
    fn set_param(&mut self, _: u32, _: f32) {}

    fn param(&self, _: u32) -> f32 {
        0.0
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], _: &FxContext) {
        self.0.process(l, r, midi);
    }

    fn all_notes_off(&mut self) {
        self.0.all_notes_off();
    }

    fn reset(&mut self) {
        self.0.silence();
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: crate::session::BUILTIN_SYNTH,
        name: "Marbble Synth",
        group: "Marbble",
        about: "A small polyphonic synth with no settings",
        params: &[],
        sections: &[],
        meters: &[],
        presets: &[],
        new: |sample_rate, _| Box::new(Basic(BasicSynth::new(sample_rate))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::test_kit::{assert_repeats, make, render};

    /// Renders with the mod wheel up repeat exactly, whatever was played before them: the
    /// vibrato's phase, the wheel and expression all start from rest (#39).
    #[test]
    fn renders_repeat_with_the_mod_wheel_up() {
        let wheel_up = [(0usize, [0xB0u8, 1, 127]), (100, [0x90, 60, 100]), (9000, [0x90, 64, 90]), (30000, [0x80, 60, 0]), (40000, [0x80, 64, 0])];
        let first = assert_repeats(&info(), &[], &wheel_up, 60000);
        let no_wheel = [(100usize, [0x90u8, 60, 100]), (30000, [0x80, 60, 0])];
        let fresh = render(make(&info(), 48000.0, 77, &[]).as_mut(), 48000.0, 60000, 512, &no_wheel);
        let mut t = make(&info(), 48000.0, 77, &[]);
        // Played live first, for an odd stretch, leaving the wheel up and expression down.
        render(t.as_mut(), 48000.0, 12345, 256, &[(0, [0xB0, 1, 100]), (10, [0xB0, 11, 40]), (20, [0x90, 67, 80])]);
        t.reset();
        assert!(first == render(t.as_mut(), 48000.0, 60000, 512, &wheel_up), "the LFO's phase carried over");
        render(t.as_mut(), 48000.0, 777, 256, &[(0, [0xB0, 1, 100]), (10, [0xB0, 11, 40])]);
        t.reset();
        assert!(fresh == render(t.as_mut(), 48000.0, 60000, 512, &no_wheel), "the wheel or expression carried over");
    }

    /// The registry's Marbble Synth is the old one, sample for sample, through everything the
    /// engine does to it: notes past the 16 voices, bend, wheel, pedal, expression, stops and
    /// renders starting.
    #[test]
    fn plays_exactly_what_the_old_synth_played() {
        let mut new = (info().new)(48000.0, 12345);
        let mut old = BasicSynth::new(48000.0);
        let ctx = FxContext { sample_rate: 48000.0, tempo: 120.0, playing: true, position: 0.0 };
        let msg = |k: u32| -> [u8; 3] {
            match k % 11 {
                0..=4 => [0x90, 30 + (k * 7 % 60) as u8, 20 + (k * 13 % 107) as u8],
                5 | 6 => [0x80, 30 + ((k - 15) * 7 % 60) as u8, 0],
                7 => [0xE0, (k % 127) as u8, 30 + (k % 60) as u8],
                8 => [0xB0, 1, (k % 128) as u8],
                9 => [0xB0, 11, 60 + (k % 68) as u8],
                _ => [0xB0, 64, if k.is_multiple_of(3) { 127 } else { 0 }],
            }
        };
        for block in 0..600u32 {
            let midi: Vec<MidiEvent> = (0..4).map(|i| MidiEvent { offset: i * 100, data: msg(block * 4 + i + 16) }).collect();
            let (mut a, mut b) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            let (mut c, mut d) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            new.process(&mut a, &mut b, &midi, &ctx);
            old.process(&mut c, &mut d, &midi);
            assert!(a == c && b == d, "block {block}");
            if block % 97 == 96 {
                new.all_notes_off();
                old.all_notes_off();
            }
            if block % 211 == 210 {
                new.reset();
                old.silence();
            }
        }
    }
}
