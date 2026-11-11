//! Freezing an instrument whose extra outputs feed auxes (`session/mixer/multi_out/`), as Logic
//! does: each of those outputs is rendered too, as its aux hears it (before the aux's own
//! plug-ins and fader), into a freeze file of its own. While the instrument is frozen each aux
//! plays its output's render (`Track::frozen_output`) in place of the plug-in's bus, which is gone
//! with the plug-in, so the mix sounds the same; the aux's plug-ins, fader and sends stay live.
//! Unfreezing gives the auxes back the live outputs and deletes the renders.

use super::*;

/// The render of one aux's output, written to disk and waiting to be placed.
pub(super) struct OutputRender {
    aux: Id,
    path: PathBuf,
    clip: Clip,
}

impl Session {
    /// Renders the output each of `auxes` is on over `range` (from, to, tail), into the project's
    /// Freeze Files folder. `progress` gets the aux's index and 0..1.
    pub(super) fn render_outputs(&mut self, auxes: &[Id], range: (i64, i64, i64), progress: &mut dyn FnMut(usize, f64)) -> Result<Vec<OutputRender>> {
        let (from, to, tail) = range;
        let sr = self.sample_rate();
        let mut out = Vec::new();
        for (j, &aux) in auxes.iter().enumerate() {
            let Some(name) = self.project.track(aux).map(|a| a.name.clone()) else { continue };
            // The aux alone, before its plug-ins and fader: what reaches it from the instrument.
            let iso = self.isolate_track(aux, false, false);
            let mut audio = self.render(from, to, tail, Some(iso), &mut |f| progress(j, f))?;
            keep_ringing(&mut audio, (to - from) as usize);
            let path = self.freeze_path(&name);
            crate::audio_file::decode::write_wav(&path, sr as u32, &audio)?;
            out.push(OutputRender { aux, clip: Clip::from_channels(path.clone(), sr as u32, audio), path });
        }
        Ok(out)
    }

    /// Each aux plays its output's render, which starts at `start` as the instrument's does.
    pub(super) fn place_outputs(&mut self, renders: Vec<OutputRender>, start: i64) {
        for r in renders {
            if self.project.track(r.aux).is_none() {
                let _ = std::fs::remove_file(&r.path);
                continue;
            }
            let clip = self.add_clip(&r.path, r.clip);
            self.project.track_mut(r.aux).unwrap().frozen_output = Some(FreezeInfo { clip, start });
            self.push_regions(r.aux);
        }
    }

    /// The auxes on `track`'s extra outputs go back to the live outputs; their renders are deleted.
    pub(super) fn unfreeze_outputs(&mut self, track: Id) {
        for aux in self.output_auxes(track) {
            let Some(f) = self.project.track_mut(aux).and_then(|a| a.frozen_output.take()) else { continue };
            self.delete_freeze_file(f.clip);
            self.push_regions(aux);
        }
    }

    /// The playback list of an aux whose instrument is frozen: its output's render.
    pub(crate) fn frozen_output_dsp(&self, t: &Track) -> Option<Arc<Vec<RegionDsp>>> {
        self.freeze_file_dsp(t.frozen_output?)
    }
}
