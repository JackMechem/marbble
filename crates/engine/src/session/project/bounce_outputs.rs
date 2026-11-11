//! Bounce in Place of an instrument whose extra outputs feed auxes (`session/mixer/multi_out/`).
//! The main output is bounced as any track's is. With the dialog's Instrument Multi-Output
//! (`BipOptions::outputs`, Logic's Include Instrument Multi Output), each aux on the instrument's
//! outputs is rendered through its own strip as the dialog asks of the main one (Audio FX, and
//! Volume/Pan when included) and added into the bounce. Muting the source then mutes those auxes
//! too, as their sound is in the bounce; without the option they play the live outputs on.

use super::bounce::BipOptions;
use crate::session::*;

impl Session {
    /// Renders each of `auxes` over `range` (from, to, tail) and adds it into `audio`, a render of
    /// the same range. `progress` gets the aux's index and 0..1.
    pub(super) fn add_output_renders(
        &mut self,
        auxes: &[Id],
        range: (i64, i64, i64),
        o: &BipOptions,
        audio: &mut [Vec<f32>],
        progress: &mut dyn FnMut(usize, f64),
    ) -> Result<()> {
        let (from, to, tail) = range;
        for (j, &aux) in auxes.iter().enumerate() {
            let iso = self.isolate_track(aux, o.fader, o.effects);
            let output = self.render(from, to, tail, Some(iso), &mut |f| progress(j, f))?;
            // Both cover the same range, lined up with its start.
            for (into, from) in audio.iter_mut().zip(&output) {
                into.iter_mut().zip(from).for_each(|(a, b)| *a += b);
            }
        }
        Ok(())
    }

    /// The source of a bounce that took its auxes along was muted: its auxes, which its mute
    /// doesn't reach, are muted too.
    pub(super) fn mute_output_auxes(&mut self, source: Id) {
        for aux in self.output_auxes(source) {
            if let Some(a) = self.project.track_mut(aux) {
                a.mute = true;
            }
        }
    }
}
