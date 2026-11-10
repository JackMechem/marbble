//! Play, stop, locate and record, and the project-wide values that go with them: cycle, tempo,
//! signature, metronome, count-in, master volume.
//!
//! Recording ends here too: `place_recording` turns what `crate::audio_io::recorder` wrote into regions or,
//! with a cycle or punch, take folders (see `takes/`). The tempo map's edits are in `global_tracks/tempo.rs`.

use super::*;
use crate::edit::takes::recording::RecPlan;

impl Session {
    /// The playhead. Until the engine has applied the last locate (or record start), that's
    /// where it was sent: an audio callback already under way when it was sent still publishes
    /// the old position, so `status.pos` alone could jump back to it for a block, and a
    /// `play()` or `record()` right after a locate would start from there.
    pub fn position(&self) -> i64 {
        match self.pending_pos {
            Some((seq, pos)) if self.status.cmds_done.load(Ordering::Acquire) < seq => pos,
            _ => self.status.pos.load(Ordering::Relaxed),
        }
    }

    pub fn is_playing(&self) -> bool {
        self.status.playing.load(Ordering::Relaxed)
    }

    pub fn is_recording(&self) -> bool {
        self.rec_punch.is_some()
    }

    /// Plays from the playhead. Does nothing if it is playing already.
    pub fn play(&mut self) {
        if self.is_playing() {
            return;
        }
        let from = self.position();
        self.last_play_from = from;
        self.send(Cmd::Play { from });
        self.auto_play_started(from);
    }

    /// Stops, placing any recording in the project. A second Stop returns to the left locator with Cycle
    /// on, otherwise to the start.
    pub fn stop(&mut self) {
        if !self.is_playing() && self.rec_punch.is_none() {
            // A second Stop returns to the left locator with Cycle on, otherwise the start.
            let to = if self.project.cycle.enabled { self.project.cycle.start } else { 0 };
            self.locate(to);
            return;
        }
        let stopped_at = self.position();
        self.send(Cmd::Stop);
        if self.rec_punch.is_some() {
            self.wait_for_engine();
            self.finish_recording();
        }
        // After the takes are in, so undoing the written automation keeps them.
        self.auto_stopped(stopped_at);
    }

    pub fn toggle_play(&mut self) {
        if self.is_playing() {
            self.stop();
        } else {
            self.play();
        }
    }

    /// Moves the playhead, which [`Session::position`] reports at once, ahead of the engine.
    pub fn locate(&mut self, pos: i64) {
        let from = self.position();
        self.send(Cmd::Locate(pos.max(0)));
        self.pending_pos = Some((self.cmds_sent, pos.max(0)));
        self.auto_jumped(from, pos.max(0));
    }

    /// Starts recording on the armed tracks: at the playhead, or with Cycle on from the left locator. A
    /// stopped transport first takes the count-in or pre-roll. [`Session::stop`] finishes it.
    pub fn record(&mut self) -> Result<()> {
        if self.rec_punch.is_some() {
            return Ok(());
        }
        let playing = self.is_playing();
        let playhead = self.position();
        let cycle = Some((self.project.cycle.start, self.project.cycle.end)).filter(|c| self.project.cycle.enabled && c.1 > c.0);
        // Cycle recording starts at the left locator (or carries on when already cycling), so
        // every pass lines up with the ones before it.
        let punch = match cycle {
            Some((cs, ce)) if !(playing && playhead >= cs && playhead < ce) => cs,
            _ => playhead,
        };
        let rec = &self.prefs.recording;
        let from = if playing {
            punch
        } else if self.project.count_in {
            // Whole bars at the tempo and signature in effect at the punch.
            let bar = self.tempo_map().samples_per_bar_at(punch as f64);
            punch - (bar * rec.count_in_bars.clamp(1, 4) as f64).round() as i64
        } else {
            // Pre-roll: play a moment of the song first, then drop into record at the playhead.
            punch - (rec.pre_roll_secs.clamp(0.0, 30.0) as f64 * self.sample_rate()).round() as i64
        };
        let depth = self.project.bit_depth.unwrap_or(rec.bit_depth);

        // An Off track can't record: it keeps its R button, ready for when it's back on.
        let enabled = self.project.tracks.iter().filter(|t| t.armed && t.frozen.is_none());
        let armed: Vec<Track> = enabled.clone().filter(|t| !self.project.is_off(t.id)).cloned().collect();
        if armed.is_empty() {
            return Err(anyhow!(if enabled.count() > 0 {
                "Every record-enabled track is off. Turn one on to record."
            } else {
                "No tracks are record-enabled"
            }));
        }
        let (plan, quiet, capture) = self.record_plan(from, punch, cycle)?;
        self.checkpoint("Record", None, false);
        let sr = self.sample_rate() as u32;
        for t in armed.iter().filter(|t| t.kind == TrackKind::Audio && t.input != InputSel::None) {
            let path = self.next_take_path(&t.name);
            let sink = self.recorder.begin(t.id, path, t.input.channels(), sr, capture.0, depth)?;
            self.send(Cmd::RecordSink { track: t.id, sink: Some(sink) });
        }
        self.rec_midi.start(&self.status);
        let target = self.midi_target_id();
        let armed_inst: Vec<Id> = armed.iter().filter(|t| t.kind == TrackKind::Instrument).map(|t| t.id).collect();
        self.rec_midi_track = match target {
            Some(t) if armed_inst.contains(&t) => Some(t),
            _ => armed_inst.first().copied(),
        };
        if let Some(id) = self.rec_midi_track {
            if Some(id) != target {
                self.send(Cmd::MidiTarget(Some(id)));
            }
        }
        self.rec_punch = Some(punch);
        self.rec_plan = Some(plan);
        self.start_journal();
        // Discard Last Recording returns to where playback started.
        self.rec_play_from = if playing { self.last_play_from } else { playhead };
        self.rec_rating = None;
        self.last_play_from = punch;
        self.send(Cmd::Autopunch { range: quiet, capture });
        self.send(Cmd::Record { from, punch });
        self.pending_pos = Some((self.cmds_sent, from));
        if !playing {
            self.auto_play_started(from.max(0));
        }
        Ok(())
    }

    pub(super) fn next_take_path(&self, track_name: &str) -> PathBuf {
        let clean = |n: &str| -> String { n.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect() };
        let clean = match self.prefs.recording.take_names {
            TakeNames::ProjectTrack => format!("{} - {}", clean(&self.project.name), clean(track_name)),
            _ => clean(track_name),
        };
        let dir = self.recording_dir();
        for i in 1.. {
            let p = dir.join(format!("{clean}#{i:02}.wav"));
            if !p.exists() {
                return p;
            }
        }
        unreachable!()
    }

    pub(super) fn finish_recording(&mut self) {
        if self.rec_punch.is_none() {
            return;
        }
        let takes = self.recorder.finish_all();
        // The compensation the take started with, as its cycle passes and punch range are cut
        // with: an undo during the take can change the delay compensation (a latent track back
        // on), but what was played was heard with the old one.
        let comp = self.rec_plan.map(|p| p.latency).unwrap_or_else(|| self.record_latency_frames());
        self.place_recording(takes, comp);
        self.recording_finished();
    }

    /// Turns what the recording in progress (`rec_punch`, `rec_plan`) captured into regions:
    /// the audio `takes`, moved `comp` frames earlier, and the MIDI. Crash recovery replays an
    /// interrupted recording through here too (`session/project/recovery/`).
    pub(super) fn place_recording(&mut self, takes: Vec<(Id, FinishedTake)>, comp: i64) {
        let Some(punch) = self.rec_punch.take() else { return };
        let plan = self.rec_plan.take();
        let stop_pos = self.position();
        // MIDI only travels one way: the performer reacts to audio that left late.
        let midi_comp = plan.map(|p| p.midi_latency).unwrap_or_else(|| self.midi_latency_frames());
        // What the tracks held before, and what the recording adds, for Discard Last Recording.
        let before: Vec<(Id, Vec<Region>)> =
            takes.iter().map(|(t, _)| *t).chain(self.rec_midi_track).filter_map(|t| Some((t, self.project.track(t)?.regions.clone()))).collect();
        // Shared by the takes every track gets from this recording (`Take::pass`).
        let recording = self.project.alloc_id();
        let mut touched = Vec::new();
        let clips = self.place_audio_recording(takes, comp, plan, recording, &mut touched);
        let midi = self.place_midi_recording(plan, punch, stop_pos, midi_comp, &mut touched);
        touched.sort_unstable();
        touched.dedup();
        self.note_recording(recording, before, &touched, clips, midi);
        for t in touched {
            self.push_regions(t);
        }
        self.update_midi_target();
        self.dirty = true;
    }

    /// The audio half of `place_recording`: each track's take becomes a clip and a region (or
    /// takes in a folder), lined up with what was heard. Returns the clips added; the tracks
    /// changed go in `touched`.
    fn place_audio_recording(&mut self, takes: Vec<(Id, FinishedTake)>, comp: i64, plan: Option<RecPlan>, recording: Id, touched: &mut Vec<Id>) -> Vec<Id> {
        let mut clips = Vec::new();
        let mut passes = takes::recording::RecordingPasses::new(recording);
        for (track, take) in takes {
            if let Some(e) = &take.error {
                self.warning = Some(e.clone());
            }
            let frames = take.channels.first().map(|c| c.len()).unwrap_or(0);
            self.send(Cmd::RecordSink { track, sink: None });
            if frames == 0 {
                let _ = std::fs::remove_file(&take.path);
                continue;
            }
            let clip_id = self.project.alloc_id();
            let rel = take.path.strip_prefix(&self.dir).unwrap_or(&take.path).to_string_lossy().to_string();
            let name = take.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let clip = Clip::from_channels(take.path.clone(), self.sample_rate() as u32, take.channels);
            self.clips.insert(clip_id, Arc::new(clip));
            self.project.clips.push(ClipRef { id: clip_id, path: rel });
            clips.push(clip_id);
            let region_id = self.project.alloc_id();
            // Line the take up with what was being heard; trim anything shifted before zero.
            let shifted = take.start - comp;
            let offset = (-shifted).max(0);
            let length = frames as i64 - offset;
            if length <= 0 {
                continue;
            }
            let name = match self.prefs.recording.take_names {
                TakeNames::Track => self.project.track(track).map(|t| t.name.clone()).unwrap_or(name),
                _ => name,
            };
            let fade = self.default_fade(length);
            let region = Region {
                id: region_id,
                name,
                start: shifted.max(0),
                length,
                muted: false,
                color: None,
                content: RegionContent::Audio { clip: clip_id, offset, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
                loop_end: None,
                takes: None,
                params: Default::default(),
                pattern: None,
            };
            // Cycle passes become a take folder; Autopunch trims to the punch range.
            let Some(region) = self.shape_take(plan, track, region, take.start, frames as i64, recording) else {
                touched.push(track);
                continue;
            };
            if let Some(dest) = self.place_take(track, region, &mut passes) {
                touched.push(track);
                touched.push(dest);
            }
        }
        clips
    }

    /// The MIDI half of `place_recording`: the messages recorded, moved `midi_comp` frames
    /// earlier, become notes and controllers over whole bars, placed as Settings › Recording ›
    /// MIDI says. Returns the regions or takes made; the track goes in `touched`.
    fn place_midi_recording(&mut self, plan: Option<RecPlan>, punch: i64, stop_pos: i64, midi_comp: i64, touched: &mut Vec<Id>) -> Vec<Id> {
        let mut midi = Vec::new();
        // Note-ons are paired with note-offs; notes still held end where recording stopped.
        let (mut events, lost) = self.rec_midi.take(&self.status);
        for (pos, _) in &mut events {
            *pos = (*pos - midi_comp).max(0);
        }
        if lost > 0 && self.rec_midi_track.is_some() {
            log::warn!("recorded MIDI: {lost} messages found the ring full");
            self.warning.get_or_insert_with(|| takes::midi_capture::lost_warning(lost));
        }
        let (Some(track), false) = (self.rec_midi_track.take(), events.is_empty()) else { return midi };
        let w = takes::recording::midi_window(plan, punch, stop_pos);
        let (start, end) = self.whole_bars(w.lo, w.hi);
        if self.prefs.recording.midi_mode() == MidiOverlap::TakeFolders {
            // Each cycle pass is a take; a held note ends at the cycle end, or where
            // recording stopped in the last pass.
            let passes = crate::edit::takes::recording::midi_passes(&events);
            let n = passes.len();
            let takes: Vec<(Vec<Note>, Vec<CcEvent>)> = passes
                .iter()
                .enumerate()
                .map(|(k, p)| takes::recording::midi_take(p, &w, start, if k + 1 == n { stop_pos } else { w.hi }))
                .filter(|(n, c)| !n.is_empty() || !c.is_empty())
                .collect();
            if !takes.is_empty() {
                midi.extend(self.place_midi_takes(track, start, end, takes));
                touched.push(track);
            }
            events.clear();
        }
        let (notes, cc) = takes::recording::midi_take(&events, &w, start, stop_pos);
        if !notes.is_empty() || !cc.is_empty() {
            let merged = match self.prefs.recording.midi_mode() {
                MidiOverlap::NewRegion | MidiOverlap::TakeFolders => false,
                MidiOverlap::Merge => self.merge_midi_take(track, start, end, &notes, &cc),
                // From the punch-in (or where recording started) to where it stopped.
                MidiOverlap::Replace => self.replace_midi_take(track, (w.lo, w.hi), start, end, &notes, &cc),
            };
            let id = self.project.alloc_id();
            if let (false, Some(t)) = (merged, self.project.track_mut(track)) {
                midi.push(id);
                let name = t.name.clone();
                t.regions.push(Region::midi(id, name, start, end - start, notes, cc));
            }
            touched.push(track);
        }
        midi
    }

    /// From the bar line at or before `lo` to the one at or after `hi` (a bar at least).
    fn whole_bars(&self, lo: i64, hi: i64) -> (i64, i64) {
        let map = self.tempo_map();
        let start = map.bar_start(map.bar_at(lo as f64)) as i64;
        let hi = hi.max(lo + 1) as f64;
        let last = map.bar_at(hi);
        let end = if map.bar_start(last) >= hi { map.bar_start(last) } else { map.bar_start(last + 1) } as i64;
        (start, end)
    }

    /// Sets the locators in either order. A cycle of no length is off.
    pub fn set_cycle(&mut self, enabled: bool, start: i64, end: i64) {
        let (start, end) = (start.min(end).max(0), start.max(end).max(0));
        self.project.cycle = Cycle { enabled: enabled && end > start, start, end };
        let c = self.project.cycle.clone();
        self.send(Cmd::Cycle { enabled: c.enabled, start: c.start, end: c.end });
    }

    pub fn set_tempo(&mut self, bpm: f64) {
        self.project.tempo = bpm.clamp(5.0, 990.0);
        // The base tempo is the Tempo track's first event.
        if let Some(first) = self.project.tempo_map.first_mut() {
            first.bpm = self.project.tempo;
        }
        self.dirty = true;
        self.sync_globals();
    }

    pub fn set_signature(&mut self, num: u32, den: u32) {
        self.project.sig_num = num.clamp(1, 32);
        self.project.sig_den = if [1, 2, 4, 8, 16].contains(&den) { den } else { 4 };
        if let Some(first) = self.project.signatures.first_mut() {
            first.num = self.project.sig_num;
            first.den = self.project.sig_den;
        }
        self.dirty = true;
        self.sync_globals();
    }

    pub fn set_key(&mut self, key: String) {
        self.project.key = key;
        self.dirty = true;
    }

    // Metronome and Count-in are transport switches, not undo steps (as in Logic), but they're
    // saved with the project, so they count as changes.
    pub fn set_metronome(&mut self, on: bool) {
        self.project.metronome = on;
        self.dirty = true;
        self.sync_globals();
    }

    pub fn set_count_in(&mut self, on: bool) {
        self.project.count_in = on;
        self.dirty = true;
        self.sync_globals();
    }

    pub fn set_master_volume(&mut self, db: f32) {
        self.project.master.volume_db = db.clamp(-96.0, 6.0);
        self.send(Cmd::MasterVolume(volume_gain(self.project.master.volume_db)));
    }
}
