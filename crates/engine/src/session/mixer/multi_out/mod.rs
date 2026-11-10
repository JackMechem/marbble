//! Multi-output instruments, Logic's way: the strip of an instrument with extra output buses has
//! a "+" that adds an aux for the next output. The aux is an ordinary aux with its own fader,
//! pan, plug-ins and sends, whose input (`Track::instrument_output`) names the instrument track
//! and the output bus. The main output plays through the instrument's strip as ever.
//!
//! `Project::feeds` counts the output as a route, so the instrument plays first, a key from the
//! aux can't reach the instrument's own plug-ins, solo follows it (a soloed instrument keeps its
//! auxes; a soloed aux doesn't need the instrument's strip) and offline renders of the aux play
//! the instrument as a key source does. `update_pdc` lines the aux up with the main output and
//! switches the bus on while an aux uses it, off the audio thread (`sync_output_buses` and
//! `vst3_host`'s `multi_out.rs`); `dsp/multi_out.rs` is the audio side.
//!
//! The aux survives its instrument: it keeps its settings and just loses this input when the
//! track goes (`prune_routes`; an undo brings both back). A new instrument with fewer outputs, or
//! a plug-in that's missing, leaves it silent, and the strip says so. A copy of the instrument
//! starts with no auxes; a copy of the aux takes the same output, as a second strip on it.
//! Freeze and Bounce in Place would take only the main output (and could leave the auxes with
//! nothing), so they're refused until the auxes are gone.

use crate::session::*;

/// An output of a multi-output instrument, for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct OutputInfo {
    /// The plug-in's bus (0 is the main output).
    pub index: u32,
    pub name: String,
    pub channels: u32,
    /// The plug-in plays it now: an aux uses it.
    pub active: bool,
}

/// The outputs of the instrument on a track.
#[derive(Debug, Clone, Serialize)]
pub struct InstrumentOutputs {
    pub track: Id,
    pub outputs: Vec<OutputInfo>,
}

/// "direct_out_0" as a person would write it.
fn tidy(name: &str) -> String {
    let spaced = name.replace('_', " ");
    let mut c = spaced.trim().chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

impl Session {
    /// Instruments with extra outputs (those with just a main output aren't listed, nor are
    /// plug-ins that aren't loaded).
    pub fn instrument_outputs(&self) -> Vec<InstrumentOutputs> {
        let mut out = Vec::new();
        for t in self.project.tracks.iter().filter(|t| t.kind == TrackKind::Instrument) {
            let Some(c) = t.instrument.as_ref().and_then(|s| self.controllers.get(&s.uid)) else { continue };
            if c.output_buses().len() > 1 {
                let outputs =
                    c.output_buses().iter().map(|b| OutputInfo { index: b.index, name: tidy(&b.name), channels: b.channels, active: b.active }).collect();
                out.push(InstrumentOutputs { track: t.id, outputs });
            }
        }
        out
    }

    /// A new aux on the instrument's next output no aux is on, named after both. Record an undo
    /// step first.
    pub fn add_output_aux(&mut self, track: Id) -> Result<Id> {
        let t = self.project.track(track).ok_or_else(|| anyhow!("no such track"))?;
        if t.kind != TrackKind::Instrument {
            return Err(anyhow!("{} isn't a software instrument track", t.name));
        }
        if t.frozen.is_some() {
            return Err(anyhow!("{} is frozen. Unfreeze it first.", t.name));
        }
        let name = t.name.clone();
        let color = t.color.clone();
        let c = t.instrument.as_ref().and_then(|s| self.controllers.get(&s.uid)).ok_or_else(|| anyhow!("{name} has no instrument with extra outputs"))?;
        if c.output_buses().len() < 2 {
            return Err(anyhow!("{name}'s instrument has no extra outputs"));
        }
        let used = |bus: u32| self.project.tracks.iter().any(|a| a.instrument_output == Some(InstrumentOutput { track, bus }));
        let next = c.output_buses().iter().skip(1).find(|b| !used(b.index)).ok_or_else(|| anyhow!("All of {name}'s outputs already have an aux"))?;
        let (bus, bus_name) = (next.index, tidy(&next.name));
        let id = self.create_aux(Some(format!("{name} {bus_name}")))?;
        if let Some(a) = self.project.track_mut(id) {
            a.color = color;
            a.instrument_output = Some(InstrumentOutput { track, bus });
        }
        self.sync_tracks();
        self.dirty = true;
        Ok(id)
    }

    /// Switches each loaded instrument's extra outputs to the ones auxes use now. Does nothing
    /// unless that changed.
    pub(crate) fn sync_output_buses(&mut self) {
        let tracks = &self.project.tracks;
        for t in tracks.iter().filter(|t| t.kind == TrackKind::Instrument) {
            let Some(c) = t.instrument.as_ref().and_then(|s| self.controllers.get_mut(&s.uid)) else { continue };
            let want: Vec<u32> = tracks.iter().filter_map(|a| a.instrument_output.filter(|o| o.track == t.id).map(|o| o.bus)).collect();
            c.set_active_outputs(&want);
        }
    }

    /// Errors (naming the track) if any of `tracks` has auxes on its extra outputs, for renders
    /// that can't carry those along.
    pub(crate) fn check_no_output_auxes(&self, tracks: &[Id], what: &str) -> Result<()> {
        for &id in tracks {
            if let Some(a) = self.project.tracks.iter().find(|a| a.instrument_output.is_some_and(|o| o.track == id)) {
                let name = self.project.track(id).map(|t| t.name.as_str()).unwrap_or_default();
                return Err(anyhow!(
                    "{name} has auxes on its extra outputs ({} and maybe others). {what} would only take its main output: delete those auxes first.",
                    a.name
                ));
            }
        }
        Ok(())
    }

    /// The instrument that feeds aux `id`, if it plays.
    pub(crate) fn output_source(&self, id: Id) -> Option<Id> {
        let source = self.project.track(id)?.instrument_output?.track;
        (!self.project.is_off(source)).then_some(source)
    }
}

#[cfg(test)]
mod tests;
