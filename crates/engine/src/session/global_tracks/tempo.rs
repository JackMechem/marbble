//! Session edits for the Tempo and Signature global tracks.
//!
//! Regions stay locked to absolute time: a tempo change moves the bar grid, not the audio or
//! MIDI. "Move MIDI Regions with Tempo" (`move_midi` below) instead keeps MIDI regions and
//! their notes on their bars and beats; audio never moves.

use crate::model::tempo::{normalize_sigs, normalize_tempo, SigEvent, TempoEvent, TempoMap};
use crate::session::*;

impl Session {
    /// The project's tempo map, built from its current tempo and signature changes.
    pub fn tempo_map(&self) -> TempoMap {
        TempoMap::from_project(&self.project)
    }

    /// What plug-ins are told about the transport at the playhead, from the engine itself
    /// (for checks: it shows which tempo map the engine has).
    pub fn transport_info(&self) -> vst3_host::TransportInfo {
        self.engine.lock().transport()
    }

    /// Replaces the Tempo track (an event at 0 is added when missing). With `move_midi`, MIDI
    /// regions and their notes move to keep their bar and beat positions.
    pub fn set_tempo_map(&mut self, events: Vec<TempoEvent>, move_midi: bool) {
        let old = self.tempo_map();
        let ev = normalize_tempo(&events, self.project.tempo);
        self.project.tempo = ev[0].bpm;
        // A single event is the plain project tempo, stored as projects always stored it.
        self.project.tempo_map = if ev.len() == 1 { Vec::new() } else { ev };
        if move_midi {
            let new = self.tempo_map();
            self.move_midi_with_tempo(&old, &new);
        }
        self.dirty = true;
        self.sync_globals();
    }

    /// Replaces the Signature track (a change at bar 1 is added when missing).
    pub fn set_signatures(&mut self, events: Vec<SigEvent>) {
        let ev = normalize_sigs(&events, (self.project.sig_num, self.project.sig_den));
        self.project.sig_num = ev[0].num;
        self.project.sig_den = ev[0].den;
        self.project.signatures = if ev.len() == 1 { Vec::new() } else { ev };
        self.dirty = true;
        self.sync_globals();
    }

    /// Sets the tempo of the event in effect at `pos` (the LCD's tempo field).
    pub fn set_tempo_at(&mut self, pos: i64, bpm: f64, move_midi: bool) {
        let mut ev = self.tempo_map().events().to_vec();
        let i = ev.iter().rposition(|e| e.pos <= pos).unwrap_or(0);
        ev[i].bpm = bpm;
        self.set_tempo_map(ev, move_midi);
    }

    /// Sets the signature in effect at 1-based `bar` (the LCD's signature menu).
    pub fn set_signature_at(&mut self, bar: u32, num: u32, den: u32) {
        let mut ev = self.tempo_map().signatures().to_vec();
        let i = ev.iter().rposition(|e| e.bar <= bar).unwrap_or(0);
        ev[i].num = num;
        ev[i].den = den;
        self.set_signatures(ev);
    }

    /// Moves MIDI regions, notes and controllers so that what sat at a bar and beat under
    /// `old` sits at the same bar and beat under `new`, on every track alternative (the
    /// inactive ones too, so switching to one finds it on its bars). Audio regions keep their
    /// time.
    pub fn move_midi_with_tempo(&mut self, old: &TempoMap, new: &TempoMap) {
        let to = |s: i64| old.remap(new, s as f64).round() as i64;
        let mut touched = Vec::new();
        // Frozen tracks can't change (their freeze file would no longer match).
        for t in self.project.tracks.iter_mut().filter(|t| t.frozen.is_none()) {
            let mut moved = false;
            for r in &mut t.regions {
                moved |= follow_tempo(r, &to);
            }
            for r in t.alternatives.iter_mut().flat_map(|a| a.regions.iter_mut()) {
                follow_tempo(r, &to);
            }
            if moved {
                touched.push(t.id);
            }
        }
        for id in touched {
            self.push_regions(id);
        }
        self.dirty = true;
    }

    /// `move_midi_with_tempo` between two Tempo tracks (the project's signatures apply).
    pub fn move_midi_between(&mut self, old: &[TempoEvent], new: &[TempoEvent]) {
        let p = &self.project;
        let sr = p.sample_rate as f64;
        let a = TempoMap::new(sr, old, &p.signatures, p.tempo, (p.sig_num, p.sig_den));
        let b = TempoMap::new(sr, new, &p.signatures, p.tempo, (p.sig_num, p.sig_den));
        self.move_midi_with_tempo(&a, &b);
    }
}

/// Moves a MIDI region, its notes and controllers from each timeline sample `s` to `to(s)`.
/// False (and nothing changed) for an audio region.
fn follow_tempo(r: &mut Region, to: &dyn Fn(i64) -> i64) -> bool {
    let rs = r.start;
    let RegionContent::Midi { notes, cc } = &mut r.content else { return false };
    let start = to(rs);
    for n in Arc::make_mut(notes) {
        let (a, b) = (to(rs + n.start), to(rs + n.start + n.length));
        n.start = a - start;
        n.length = (b - a).max(1);
    }
    for c in Arc::make_mut(cc) {
        c.start = to(rs + c.start) - start;
    }
    r.length = (to(rs + r.length) - start).max(1);
    r.loop_end = r.loop_end.map(to);
    r.start = start;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::session::MidiContent;

    #[test]
    fn move_midi_with_tempo_moves_every_track_alternative() {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let sr = s.project.sample_rate as i64;
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        // A region on bar 3 with a note on its second beat, at 120 BPM.
        let map = s.tempo_map();
        let bar3 = map.bar_start(3).round() as i64;
        let beat = (map.samples_per_quarter_at(0.0)).round() as i64;
        let id = s.create_midi_region(t, bar3, 4 * beat).unwrap();
        s.set_midi_content(vec![MidiContent {
            region: id,
            notes: vec![Note { start: beat, length: beat, pitch: 60, velocity: 100, muted: false }],
            cc: vec![],
            start: None,
            length: None,
        }]);
        // Alternative B is a copy; A, with the original, waits.
        s.new_track_alternative(t, true).unwrap();
        // Twice as fast from 1 s on (half way through bar 1).
        s.set_tempo_map(vec![TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: sr, bpm: 240.0, ramp: false }], true);
        let map = s.tempo_map();
        let on_bars = |r: &Region| {
            let RegionContent::Midi { notes, .. } = &r.content else { return false };
            let bb = map.bar_beat_at((r.start + notes[0].start) as f64);
            (r.start as f64 - map.bar_start(3)).abs() <= 1.0 && (bb.bar, bb.beat, bb.div) == (3, 2, 1)
        };
        let track = s.project.track(t).unwrap();
        assert!(on_bars(&track.regions[0]), "the active alternative's region: {:?}", track.regions[0].start);
        let waiting = &track.alternatives[0].regions[0];
        assert!(on_bars(waiting), "the inactive alternative's region moves too: {} vs {}", waiting.start, map.bar_start(3));
    }
}
