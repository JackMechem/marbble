//! Session edits for MIDI content: the Piano Roll's wholesale note/controller replacement and
//! Standard MIDI File import and export.

use crate::midi::smf::{self, SmfCc, SmfFile, SmfNote, SmfTrack};
use crate::model::tempo::{events_from_quarters, sigs_from_quarters, SigEvent, TempoEvent, TempoMap};
use crate::session::*;

/// One region's new content, as the Piano Roll sends it after an edit.
#[derive(Debug, Clone, Deserialize)]
pub struct MidiContent {
    pub region: Id,
    pub notes: Vec<Note>,
    #[serde(default)]
    pub cc: Vec<CcEvent>,
    /// New timeline start, when the edit grew the region to the left. The notes and controllers
    /// are relative to it.
    #[serde(default)]
    pub start: Option<i64>,
    /// New length, when notes were drawn or moved past the region's end.
    #[serde(default)]
    pub length: Option<i64>,
}

/// A MIDI file's own tempo map (None when it has no tempo: it then follows the project's).
fn file_tempo_map(file: &SmfFile, sr: f64) -> Option<TempoMap> {
    let ppq = file.ppq.max(1) as f64;
    let mut changes: Vec<(f64, f64)> = file.tempos.iter().map(|&(t, bpm)| (t as f64 / ppq, bpm)).collect();
    if changes.is_empty() {
        changes.push((0.0, file.tempo?));
    }
    if changes[0].0 > 0.0 {
        // The Standard MIDI File default.
        changes.insert(0, (0.0, 120.0));
    }
    Some(TempoMap::new(sr, &events_from_quarters(sr, &changes), &[], 120.0, (4, 4)))
}

/// A file's pickup, in quarters: a first bar (its signature changing at its end) shorter than the
/// bar after it. `sigs` are the file's signature changes as (quarter, numerator, denominator).
fn file_pickup(sigs: &[(f64, u32, u32)]) -> Option<f64> {
    let [(q0, n0, d0), (q1, n1, d1), ..] = *sigs else { return None };
    let len = q1 - q0;
    let one_bar = (len - n0 as f64 * 4.0 / d0 as f64).abs() < 1e-6;
    (q0 == 0.0 && one_bar && len < n1 as f64 * 4.0 / d1 as f64 - 1e-6).then_some(len)
}

/// The signature a MIDI file gives a pickup `q` quarters long: whole beats of the finest
/// denominator needed (to a 32nd), e.g. 1/4 for a quarter, 3/8 for a dotted quarter.
fn pickup_signature(q: f64) -> (u32, u32) {
    let beats = |den: u32| q * den as f64 / 4.0;
    let den = [4, 8, 16, 32].into_iter().find(|&d| (beats(d) - beats(d).round()).abs() < 1e-6).unwrap_or(32);
    ((beats(den).round() as u32).max(1), den)
}

/// Where an imported file's ticks land in the project, its tick 0 at `start`.
struct FileTime {
    /// The project's tempo map, as it is once the file's timing has been adopted.
    map: TempoMap,
    /// The file's own (see `file_tempo_map`).
    file_map: Option<TempoMap>,
    ppq: f64,
    start: i64,
    /// `start` in project quarter notes.
    start_q: f64,
}

impl FileTime {
    fn new(map: TempoMap, file_map: Option<TempoMap>, ppq: u16, start: i64) -> FileTime {
        let start_q = map.beats_at(start as f64);
        FileTime { map, file_map, ppq: ppq.max(1) as f64, start, start_q }
    }

    /// Without a tempo of its own, the file follows the project's bars and beats.
    fn to_samples(&self, ticks: u64) -> i64 {
        let q = ticks as f64 / self.ppq;
        match &self.file_map {
            Some(fm) => self.start + fm.sample_at(q).round() as i64,
            None => self.map.sample_at(self.start_q + q).round() as i64,
        }
    }

    /// The first project bar line at or after sample `s`.
    fn bar_line_after(&self, s: i64) -> i64 {
        let map = &self.map;
        let b = map.bar_at(s as f64);
        let at = map.bar_start(b);
        (if at >= s as f64 { at } else { map.bar_start(b + 1) }).round() as i64
    }

    /// One part of the file as a region that starts at the bar holding its first event and
    /// ends on a bar line.
    fn region(&self, part: &SmfTrack, id: Id, name: String) -> Region {
        let map = &self.map;
        let first = part.notes.iter().map(|n| n.start).chain(part.cc.iter().map(|c| c.start)).min().unwrap_or(0);
        let last = part.notes.iter().map(|n| n.start + n.length).chain(part.cc.iter().map(|c| c.start + 1)).max().unwrap_or(0);
        let rstart = (map.bar_start(map.bar_at(self.to_samples(first) as f64)).round() as i64).max(0);
        let end = self.to_samples(last);
        let length = self.bar_line_after(end.max(rstart + 1)) - rstart;
        let notes = part
            .notes
            .iter()
            .map(|n| {
                let on = self.to_samples(n.start);
                Note {
                    start: on - rstart,
                    length: (self.to_samples(n.start + n.length) - on).max(1),
                    pitch: n.pitch,
                    velocity: n.velocity.max(1),
                    muted: false,
                }
            })
            .collect();
        let cc = part.cc.iter().map(|c| CcEvent { start: self.to_samples(c.start) - rstart, kind: c.kind, value: c.value }).collect();
        Region::midi(id, name, rstart, length, notes, cc)
    }
}

impl Session {
    /// Replaces the notes and controllers of MIDI regions (and optionally their bounds).
    /// Regions that aren't MIDI are skipped.
    pub fn set_midi_content(&mut self, edits: Vec<MidiContent>) {
        let mut touched = Vec::new();
        for e in edits {
            let Some((track, idx)) = self.find_region(e.region) else { continue };
            let r = &mut self.project.track_mut(track).unwrap().regions[idx];
            let RegionContent::Midi { notes, cc } = &mut r.content else { continue };
            let (mut n, mut c) = (e.notes, e.cc);
            crate::edit::midi_content::normalize(&mut n, &mut c);
            (*notes, *cc) = (Arc::new(n), Arc::new(c));
            if let Some(s) = e.start {
                r.start = s.max(0);
            }
            if let Some(l) = e.length {
                r.length = l.max(1);
            }
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
    }

    /// Imports a Standard MIDI File with its tick 0 at `start`. With `track` set (an instrument
    /// track) the file's first part lands on it and the others on new instrument tracks below;
    /// otherwise every part gets a new track, inserted at `index` (the end when None). Ticks
    /// become samples through the file's tempo map, so the file plays as it was written; in an
    /// empty project its tempo map and time signatures are adopted. Returns the new regions.
    pub fn import_midi(&mut self, path: &Path, track: Option<Id>, start: i64, index: Option<usize>) -> Result<Vec<Id>> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let file = smf::read(&bytes)?;
        if file.tracks.is_empty() {
            return Err(anyhow!("{} has no notes", path.display()));
        }
        let start = start.max(0);
        let sr = self.project.sample_rate as f64;
        let file_map = file_tempo_map(&file, sr);
        if self.project.tracks.iter().all(|t| t.regions.is_empty()) {
            self.adopt_file_timing(&file, file_map.as_ref(), start);
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "MIDI".into());
        let time = FileTime::new(self.tempo_map(), file_map, file.ppq, start);

        let target = track.filter(|id| self.project.track(*id).map(|t| t.kind == TrackKind::Instrument).unwrap_or(false));
        let mut next_index = match (target, index) {
            (Some(id), _) => self.project.tracks.iter().position(|t| t.id == id).map(|i| i + 1).unwrap_or(self.project.tracks.len()),
            (None, Some(i)) => i.min(self.project.tracks.len()),
            (None, None) => self.project.tracks.len(),
        };
        let mut made = Vec::new();
        let mut first_track = None;
        for (k, part) in file.tracks.iter().enumerate() {
            let name = if part.name.is_empty() { stem.clone() } else { part.name.clone() };
            let tid = match target {
                Some(id) if k == 0 => id,
                _ => {
                    let id = self.add_track(TrackKind::Instrument, InputSel::None, None, Some(next_index))?;
                    next_index += 1;
                    self.project.track_mut(id).unwrap().name = name.clone();
                    id
                }
            };
            first_track.get_or_insert(tid);
            let rid = self.project.alloc_id();
            let region = time.region(part, rid, name);
            self.project.track_mut(tid).unwrap().regions.push(region);
            self.push_regions(tid);
            made.push(rid);
        }
        if first_track.is_some() {
            self.select_track(first_track);
        }
        self.dirty = true;
        Ok(made)
    }

    /// An empty project takes on an imported file's tempo map (`file_map`) and time signatures,
    /// the file's tick 0 and bar 1 at `start`.
    fn adopt_file_timing(&mut self, file: &SmfFile, file_map: Option<&TempoMap>, start: i64) {
        if let Some(fm) = file_map {
            // Keep the tempo tidy: files often store 119.99999 for 120. (Other values stay
            // exact, so a ramp written as steps keeps its length.)
            let tidy = |bpm: f64| {
                let r = (bpm * 100.0).round() / 100.0;
                if (bpm - r).abs() < 1e-3 {
                    r
                } else {
                    bpm
                }
            };
            let mut ev: Vec<TempoEvent> = fm.events().iter().map(|e| TempoEvent { pos: e.pos + start, bpm: tidy(e.bpm), ramp: false }).collect();
            ev.insert(0, TempoEvent { pos: 0, bpm: ev[0].bpm, ramp: false });
            self.set_tempo_map(ev, false);
        }
        if !file.signatures.is_empty() {
            let ppq = file.ppq.max(1) as f64;
            let mut q: Vec<(f64, u32, u32)> = file.signatures.iter().map(|&(t, n, d)| (t as f64 / ppq, n, d)).collect();
            // At the timeline's start, a short first bar is the project's pickup.
            let pickup = if start == 0 { file_pickup(&q) } else { None };
            if let Some(p) = pickup {
                q.remove(0);
                q.iter_mut().for_each(|c| c.0 -= p);
            }
            let (_, num, den) = q[0];
            let first = self.project.first_bar;
            self.set_signatures(vec![SigEvent { bar: first, num, den }]);
            if start == 0 {
                self.set_bar_numbering(pickup.unwrap_or(0.0), first);
            }
            // The file's first full bar is the project bar at `start` (or after the pickup).
            let at = self.tempo_map().bar_at(start as f64).max(first);
            let mut all = vec![SigEvent { bar: first, num, den }];
            all.extend(sigs_from_quarters(&q, (4, 4), at));
            self.set_signatures(all);
        }
    }

    /// Writes the given MIDI regions to a Standard MIDI File, one file track per project track,
    /// at their project positions, loops repeated. Muted notes, and anything a region's end
    /// cuts off, are left out, as they don't play.
    pub fn export_midi(&self, ids: &[Id], path: &Path) -> Result<()> {
        let map = self.tempo_map();
        let ppq = smf::PPQ as f64;
        let ticks = |s: i64| (map.beats_at(s.max(0) as f64) * ppq).round() as u64;
        let mut parts = Vec::new();
        for t in &self.project.tracks {
            let mut part = SmfTrack { name: t.name.clone(), ..Default::default() };
            // A looped region exports every pass, as it plays.
            // Playback parameters are applied, so the file holds what plays.
            for r in t.regions.iter().filter(|r| ids.contains(&r.id)).flat_map(|r| crate::edit::region_params::bake(r, &map).real_copies()) {
                let RegionContent::Midi { notes, cc } = &r.content else { continue };
                for n in notes.iter().filter(|n| !n.muted && n.start >= 0 && n.start < r.length) {
                    let on = ticks(r.start + n.start);
                    let off = ticks(r.start + (n.start + n.length).min(r.length));
                    part.notes.push(SmfNote { start: on, length: off.saturating_sub(on).max(1), pitch: n.pitch, velocity: n.velocity });
                }
                for c in cc.iter().filter(|c| c.start >= 0 && c.start < r.length) {
                    part.cc.push(SmfCc { start: ticks(r.start + c.start), kind: c.kind, value: c.value });
                }
            }
            if part.notes.is_empty() && part.cc.is_empty() {
                continue;
            }
            part.notes.sort_by_key(|n| (n.start, n.pitch));
            part.cc.sort_by_key(|c| c.start);
            parts.push(part);
        }
        if parts.is_empty() {
            return Err(anyhow!("There are no notes to export"));
        }
        // The tempo track: every change, with ramps written as a step every sixteenth note
        // (each step's tempo keeps its exact duration).
        let mut tempos = Vec::new();
        let ev = map.events();
        for (i, e) in ev.iter().enumerate() {
            let q0 = map.beats_at(e.pos as f64);
            match ev.get(i + 1).filter(|_| e.ramp) {
                Some(next) => {
                    let q1 = map.beats_at(next.pos as f64);
                    let steps = ((q1 - q0) / 0.25).ceil().max(1.0) as usize;
                    for k in 0..steps {
                        let qa = q0 + (q1 - q0) * k as f64 / steps as f64;
                        let qb = q0 + (q1 - q0) * (k + 1) as f64 / steps as f64;
                        let secs = (map.sample_at(qb) - map.sample_at(qa)) / map.sample_rate();
                        tempos.push(((qa * ppq).round() as u64, (qb - qa) * 60.0 / secs.max(1e-9)));
                    }
                }
                None => tempos.push(((q0 * ppq).round() as u64, e.bpm)),
            }
        }
        // A pickup is a short bar of its own at the start.
        let pickup = (map.pickup() > 0.0).then(|| {
            let (num, den) = pickup_signature(map.pickup());
            (0, num, den)
        });
        let signatures = pickup.into_iter().chain(map.signatures().iter().map(|s| ((map.bar_start_q(s.bar) * ppq).round() as u64, s.num, s.den))).collect();
        let file = SmfFile {
            ppq: smf::PPQ,
            tempo: Some(self.project.tempo),
            signature: Some((self.project.sig_num, self.project.sig_den)),
            tracks: parts,
            tempos,
            signatures,
        };
        std::fs::write(path, smf::write(&file)).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_first_bar_is_a_pickup() {
        // 1/4, then 4/4: a quarter pickup.
        assert_eq!(file_pickup(&[(0.0, 1, 4), (1.0, 4, 4)]), Some(1.0));
        // 3/8 then 6/8, a dotted-quarter pickup.
        assert_eq!(file_pickup(&[(0.0, 3, 8), (1.5, 6, 8), (7.5, 4, 4)]), Some(1.5));
        // A whole bar of its own signature that isn't shorter than the next one's, a change
        // off the first bar line, or a single signature: no pickup.
        assert_eq!(file_pickup(&[(0.0, 4, 4), (4.0, 3, 4)]), None);
        assert_eq!(file_pickup(&[(0.0, 1, 4), (2.0, 4, 4)]), None);
        assert_eq!(file_pickup(&[(0.0, 4, 4)]), None);
    }

    #[test]
    fn a_pickup_writes_as_whole_beats() {
        assert_eq!(pickup_signature(1.0), (1, 4));
        assert_eq!(pickup_signature(1.5), (3, 8));
        assert_eq!(pickup_signature(3.0), (3, 4));
        assert_eq!(pickup_signature(0.25), (1, 16));
        assert_eq!(pickup_signature(1.0 / 3.0), (3, 32));
    }
}
