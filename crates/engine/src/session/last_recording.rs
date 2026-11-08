//! What the last recording made: Discard Last Recording, and the keys that mark its take good
//! or bad.
//!
//! `place_recording` (`transport.rs`) notes each recording in a [`LastRecording`]: every track
//! it changed, with its regions before and after, and the clips and MIDI it made. Discarding
//! puts back the tracks that are still as the recording left them, and on a track edited
//! since takes out just what was recorded. The discarded files are deleted once nothing can
//! need them ([`Session::purge_discarded`]).

use super::*;
use crate::takes::TAKE_COLORS;

/// What a recording made, for Discard Last Recording and the rating keys.
#[derive(Debug, Clone)]
pub(super) struct LastRecording {
    /// Its `Take::pass` id.
    recording: Id,
    /// Where playback started.
    from: i64,
    tracks: Vec<RecordedTrack>,
    /// Its audio files.
    clips: Vec<Id>,
    /// Its MIDI regions and takes.
    midi: Vec<Id>,
}

#[derive(Debug, Clone)]
struct RecordedTrack {
    track: Id,
    /// The track's regions before the recording; None when the recording made the track
    /// (Overlapping Recordings › Create a new track).
    before: Option<Vec<Region>>,
    /// And as the recording left them.
    after: Vec<Region>,
}

impl LastRecording {
    /// Whether the region or take `id` (playing `clip`, for audio) is the recording's.
    fn made(&self, id: Id, clip: Option<Id>) -> bool {
        made(&self.clips, &self.midi, id, clip)
    }
}

fn made(clips: &[Id], midi: &[Id], id: Id, clip: Option<Id>) -> bool {
    midi.contains(&id) || clip.is_some_and(|c| clips.contains(&c))
}

fn audio_clip(r: &Region) -> Option<Id> {
    match r.content {
        RegionContent::Audio { clip, .. } => Some(clip),
        RegionContent::Midi { .. } => None,
    }
}

fn same_regions(a: &[Region], b: &[Region]) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

/// The project documents in `dir` that can name a file: project.json and its autosave and
/// backup, the backups (Backups/) and the other project alternatives (Alternatives/).
fn project_docs(dir: &Path) -> Vec<String> {
    fn json_in(dir: &Path, depth: usize, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() && depth > 0 {
                json_in(&p, depth - 1, out);
            } else if p.extension().is_some_and(|x| x == "json" || x == "bak") {
                out.extend(std::fs::read_to_string(&p).ok());
            }
        }
    }
    let mut out = Vec::new();
    json_in(dir, 0, &mut out);
    for sub in [super::backups::BACKUPS_DIR, super::alternatives::ALTERNATIVES_DIR] {
        json_in(&dir.join(sub), 2, &mut out);
    }
    out
}

impl Session {
    /// After `place_recording` has placed a recording on the `touched` tracks (`before`: the
    /// regions of the tracks it recorded to, as they were, and the `clips` and `midi` it made):
    /// notes it as the last recording, gives its take the rating marked while recording and,
    /// with Settings › Recording › Color takes, colours the takes of its folders.
    pub(super) fn note_recording(&mut self, recording: Id, before: Vec<(Id, Vec<Region>)>, touched: &[Id], clips: Vec<Id>, midi: Vec<Id>) {
        let rating = self.rec_rating.take();
        if touched.is_empty() {
            self.last_recording = None;
            return;
        }
        let tracks = touched
            .iter()
            .map(|&t| RecordedTrack { track: t, before: before.iter().find(|(id, _)| *id == t).map(|(_, r)| r.clone()), after: Vec::new() })
            .collect();
        self.last_recording = Some(LastRecording { recording, from: self.rec_play_from, tracks, clips, midi });
        if let Some(r) = rating {
            self.rate_recorded(r);
        }
        if self.prefs.recording.color_takes {
            self.color_recorded();
        }
        let rec = self.last_recording.as_mut().unwrap();
        for t in &mut rec.tracks {
            t.after = self.project.track(t.track).map(|x| x.regions.clone()).unwrap_or_default();
        }
    }

    /// The rating keys: while recording, marks the take being recorded, which lands rated.
    /// Otherwise marks the last recording's newest take on each track it recorded to, or,
    /// with that gone, the take heard in each of the take folders `regions`. Returns whether
    /// anything was marked.
    pub fn mark_take(&mut self, rating: TakeRating, regions: &[Id]) -> bool {
        if self.is_recording() {
            self.rec_rating = Some(rating);
            return true;
        }
        if self.rate_recorded(rating) {
            return true;
        }
        let mut marked = false;
        for &id in regions {
            let Some(f) = self.region(id).and_then(|r| r.takes.as_ref()) else { continue };
            let Some(take) = f.whole_take().or(f.comp.first().map(|s| s.take)).or(f.takes.last().map(|t| t.id)) else { continue };
            self.set_take_rating(id, take, rating);
            marked = true;
        }
        marked
    }

    /// Rates the last recording's newest take on each of its tracks. A recording that landed
    /// as a plain region becomes a take folder of one take, so the mark has somewhere to go.
    /// Returns whether there was a take to rate.
    fn rate_recorded(&mut self, rating: TakeRating) -> bool {
        let Some(mut rec) = self.last_recording.take() else { return false };
        let mut rated = false;
        let LastRecording { recording, tracks, clips, midi, .. } = &mut rec;
        for rt in tracks.iter_mut() {
            let Some(t) = self.project.track(rt.track) else { continue };
            // A track still as the recording left it stays that way, for Discard.
            let as_recorded = same_regions(&t.regions, &rt.after);
            let plain: Vec<usize> =
                (0..t.regions.len()).filter(|&i| t.regions[i].takes.is_none() && made(clips, midi, t.regions[i].id, audio_clip(&t.regions[i]))).collect();
            for i in plain {
                let id = self.project.alloc_id();
                wrap_in_folder(&mut self.project.track_mut(rt.track).unwrap().regions[i], id, (*recording, 0));
                // The take is the recording's, wherever its content came from.
                midi.push(id);
            }
            let t = self.project.track_mut(rt.track).unwrap();
            for r in &mut t.regions {
                let is_midi = audio_clip(r).is_none();
                let Some(f) = r.takes.as_mut() else { continue };
                if let Some(k) = f.takes.iter_mut().rev().find(|k| made(clips, midi, k.id, (!is_midi).then_some(k.clip))) {
                    k.rating = rating;
                    rated = true;
                }
            }
            if as_recorded {
                rt.after = t.regions.clone();
            }
        }
        let tracks: Vec<Id> = rec.tracks.iter().map(|t| t.track).collect();
        self.last_recording = Some(rec);
        if rated {
            for t in tracks {
                self.push_regions(t);
            }
            self.dirty = true;
        }
        rated
    }

    /// Settings › Recording › Color takes: every take without a colour in the folders the last
    /// recording added to gets the next of [`TAKE_COLORS`], by its place in the folder.
    fn color_recorded(&mut self) {
        let Some(rec) = self.last_recording.take() else { return };
        for rt in &rec.tracks {
            let Some(t) = self.project.track_mut(rt.track) else { continue };
            for r in &mut t.regions {
                let midi = audio_clip(r).is_none();
                let Some(f) = r.takes.as_mut() else { continue };
                if !f.takes.iter().any(|k| rec.made(k.id, (!midi).then_some(k.clip))) {
                    continue;
                }
                for (i, k) in f.takes.iter_mut().enumerate() {
                    k.color.get_or_insert_with(|| TAKE_COLORS[i % TAKE_COLORS.len()].to_string());
                }
            }
        }
        self.last_recording = Some(rec);
    }

    /// Discard Last Recording: stops a recording in progress, then takes the last recording off
    /// every track it recorded to, as one undo step, and returns the playhead to where playback
    /// started. Its files are kept until nothing can need them ([`Session::purge_discarded`]).
    pub fn discard_last_recording(&mut self) -> Result<()> {
        if self.is_recording() {
            self.stop();
        }
        let Some(rec) = self.last_recording.take() else { return Err(anyhow!("There's no recording to discard")) };
        self.checkpoint("Discard Last Recording", None, false);
        let mut changed = false;
        let mut made_tracks = Vec::new();
        for rt in &rec.tracks {
            let Some(regions) = self.project.track(rt.track).map(|t| t.regions.clone()) else { continue };
            // A track still as the recording left it goes back to how it was before; on one
            // edited since, only the recorded takes and regions go.
            match (&rt.before, same_regions(&regions, &rt.after)) {
                (Some(before), true) => self.project.track_mut(rt.track).unwrap().regions = before.clone(),
                (None, true) => made_tracks.push(rt.track),
                _ if self.remove_recorded(rt.track, &rec, true) => {}
                _ => continue,
            }
            changed = true;
            self.push_regions(rt.track);
        }
        // Its takes packed into a folder on another track since (Pack Take Folder across tracks)
        // go too. A copy exported to a track of its own stays, with its file.
        let others: Vec<Id> = self.project.tracks.iter().map(|t| t.id).filter(|t| !rec.tracks.iter().any(|r| r.track == *t)).collect();
        for t in others {
            if self.remove_recorded(t, &rec, false) {
                changed = true;
                self.push_regions(t);
            }
        }
        if !changed {
            self.abandon_checkpoint();
            return Err(anyhow!("The last recording isn't in the project any more"));
        }
        self.delete_tracks(&made_tracks);
        // Files nothing plays any more leave the project, to be deleted when it's saved or closed.
        let used = self.project.clips_in_use();
        for c in rec.clips.iter().filter(|c| !used.contains(c)) {
            if let Some(i) = self.project.clips.iter().position(|r| r.id == *c) {
                let r = self.project.clips.remove(i);
                let path = if Path::new(&r.path).is_absolute() { PathBuf::from(&r.path) } else { self.dir.join(&r.path) };
                self.discarded.push((*c, path));
            }
        }
        self.locate(rec.from);
        self.dirty = true;
        Ok(())
    }

    /// Takes the takes `rec` made off `track`, and its plain regions with `plain`. A folder left
    /// without takes goes; one that loses the take it played falls back on the newest wherever it
    /// has audio. Returns whether anything went.
    fn remove_recorded(&mut self, track: Id, rec: &LastRecording, plain: bool) -> bool {
        let Some(t) = self.project.track_mut(track) else { return false };
        let n = t.regions.len();
        t.regions.retain(|r| !plain || r.takes.is_some() || !rec.made(r.id, audio_clip(r)));
        let mut changed = t.regions.len() != n;
        for r in &mut t.regions {
            let midi = audio_clip(r).is_none();
            if r.takes.is_none() {
                continue;
            }
            crate::takes::save_midi_take(r);
            let f = r.takes.as_mut().unwrap();
            let k = f.takes.len();
            f.takes.retain(|x| !rec.made(x.id, (!midi).then_some(x.clip)));
            if f.takes.len() < k {
                changed = true;
                f.normalize();
                if f.comp.is_empty() {
                    f.default_comp();
                }
                f.store_comp();
                let newest = f.takes.last().map(|x| x.clip);
                if let (Some(c), RegionContent::Audio { clip, .. }) = (newest, &mut r.content) {
                    *clip = c;
                }
            }
            crate::takes::load_midi_take(r);
        }
        t.regions.retain(|r| r.takes.as_ref().is_none_or(|f| !f.takes.is_empty()));
        changed
    }

    /// Deletes the files of discarded recordings that nothing can need any more: not the
    /// project, an undo or redo step, nor a document in the project folder (project.json, its
    /// autosave, the backups or another alternative). `closing`: the project is being closed,
    /// so its undo history is going and only the documents on disk count; files they still
    /// name are kept for good.
    pub(super) fn purge_discarded(&mut self, closing: bool) {
        if self.discarded.is_empty() {
            return;
        }
        let docs = project_docs(&self.dir);
        for (clip, path) in std::mem::take(&mut self.discarded) {
            let listed = |p: &Project| p.clips.iter().any(|c| c.id == clip);
            let undoable = !closing && (listed(&self.project) || self.undo_stack.iter().chain(&self.redo_stack).any(|e| listed(&e.project)));
            let rel = path.strip_prefix(&self.dir).unwrap_or(&path).to_string_lossy().to_string();
            let quoted = serde_json::to_string(&rel).unwrap_or_default();
            if undoable || docs.iter().any(|d| d.contains(&quoted)) {
                if !closing {
                    self.discarded.push((clip, path));
                }
                continue;
            }
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("couldn't delete the discarded recording {}: {e}", path.display());
                }
            }
        }
    }

    /// Forgets the last recording when its project closes, deleting the files of the ones
    /// discarded that nothing on disk needs.
    pub(super) fn close_recordings(&mut self) {
        self.purge_discarded(true);
        self.last_recording = None;
        self.rec_rating = None;
    }
}

/// Makes the plain region `r` a take folder of one take, `id` (pass `pass`), that plays as it did.
fn wrap_in_folder(r: &mut Region, id: Id, pass: (Id, u32)) {
    let mut f = TakeFolder::new();
    let name = "Take 1".to_string();
    match &mut r.content {
        RegionContent::Audio { clip, offset, .. } => {
            f.takes.push(Take { id, name, clip: *clip, offset: *offset, start: 0, end: r.length, pass: Some(pass), ..Default::default() });
            *offset = 0;
        }
        RegionContent::Midi { notes, cc } => {
            f.takes.push(Take { id, name, start: 0, end: r.length, notes: notes.clone(), cc: cc.clone(), pass: Some(pass), ..Default::default() });
        }
    }
    f.select_whole(id);
    r.takes = Some(f);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(tag: &str) -> Session {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("marbble-last-rec-{tag}-{}.marbble", std::process::id()));
        let _ = std::fs::remove_dir_all(&s.dir);
        s
    }

    /// Places a recording as `finish_recording` does, one undo step: `frames` of audio from
    /// `start` on each of `tracks`, played from `from`. Its files are written, empty.
    fn record_onto(s: &mut Session, from: i64, start: i64, frames: usize, tracks: &[Id]) {
        s.checkpoint("Record", None, false);
        s.rec_punch = Some(start);
        s.rec_plan = None;
        s.rec_play_from = from;
        let dir = s.recording_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let takes = tracks
            .iter()
            .map(|&t| {
                let path = s.next_take_path(&format!("T{t}"));
                std::fs::write(&path, b"RIFF").unwrap();
                (t, FinishedTake { path, start, channels: vec![vec![0.1; frames]], error: None })
            })
            .collect();
        s.place_recording(takes, 0);
    }

    fn takes(s: &Session, track: Id) -> Vec<Take> {
        s.project.track(track).unwrap().regions.iter().flat_map(|r| r.takes.iter().flat_map(|f| f.takes.clone())).collect()
    }

    fn spans(s: &Session, track: Id) -> Vec<(i64, i64, bool)> {
        s.project.track(track).unwrap().regions.iter().map(|r| (r.start, r.length, r.takes.is_some())).collect()
    }

    #[test]
    fn discard_puts_every_track_back_and_returns_the_playhead() {
        let mut s = session("discard");
        let oh = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let kick = s.add_track(TrackKind::Audio, InputSel::Mono(1), None, None).unwrap();
        record_onto(&mut s, 0, 1000, 1000, &[oh, kick]);
        record_onto(&mut s, 500, 1000, 1000, &[oh, kick]);
        assert_eq!(takes(&s, oh).len(), 2);
        let clips = s.project.clips.len();
        s.locate(3000);
        s.discard_last_recording().unwrap();
        for t in [oh, kick] {
            assert_eq!(spans(&s, t), [(1000, 1000, false)]);
        }
        assert_eq!(s.position(), 500);
        // Its files leave the project, kept on disk while Undo can bring them back.
        assert_eq!(s.project.clips.len(), clips - 2);
        assert_eq!(s.discarded.len(), 2);
        assert!(s.discarded.iter().all(|(_, p)| p.exists()));
        // One undo step brings the take back on both tracks.
        s.undo();
        for t in [oh, kick] {
            assert_eq!(takes(&s, t).len(), 2);
        }
        assert_eq!(s.project.clips.len(), clips);
        s.redo();
        // Nothing left to discard.
        assert!(s.discard_last_recording().is_err());
    }

    #[test]
    fn discard_after_an_edit_takes_out_only_the_recording() {
        let mut s = session("edited");
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        record_onto(&mut s, 0, 0, 1000, &[t]);
        let old = takes(&s, t);
        assert!(old.is_empty());
        record_onto(&mut s, 0, 500, 1000, &[t]);
        let folder = s.project.track(t).unwrap().regions[0].id;
        s.checkpoint("Rename", None, false);
        s.rename_region(folder, "Kit".into());
        s.discard_last_recording().unwrap();
        let regions = &s.project.track(t).unwrap().regions;
        assert_eq!((regions.len(), regions[0].name.as_str()), (1, "Kit"));
        let f = regions[0].takes.as_ref().unwrap();
        // The older take is left, heard where it has audio.
        assert_eq!((f.takes.len(), f.comp.len()), (1, 1));
        assert!(matches!(regions[0].content, RegionContent::Audio { clip, .. } if clip == f.takes[0].clip));
        // A take packed onto another track since goes from the folder there.
        let other = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        record_onto(&mut s, 0, 3000, 1000, &[other]);
        let ids = [s.project.track(t).unwrap().regions[0].id, s.project.track(other).unwrap().regions[0].id];
        s.flatten_take_folder(ids[0]);
        let flat = s.project.track(t).unwrap().regions[0].id;
        s.pack_take_folder(&[flat, ids[1]]).unwrap();
        s.discard_last_recording().unwrap();
        assert_eq!(takes(&s, t).len(), 1);
        assert!(s.project.track(other).unwrap().regions.is_empty());
        // A recording placed on a new track takes the track with it.
        s.prefs.recording.audio_overlap = AudioOverlap::NewTrack;
        let n = s.project.tracks.len();
        record_onto(&mut s, 0, 0, 1000, &[t]);
        assert_eq!(s.project.tracks.len(), n + 1);
        s.discard_last_recording().unwrap();
        assert_eq!(s.project.tracks.len(), n);
    }

    #[test]
    fn takes_are_marked_while_and_after_recording() {
        let mut s = session("mark");
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        // Marked while recording: the take lands rated, a one-take folder over nothing.
        s.rec_rating = Some(TakeRating::Bad);
        record_onto(&mut s, 0, 0, 1000, &[t]);
        assert_eq!(spans(&s, t), [(0, 1000, true)]);
        assert_eq!(takes(&s, t).iter().map(|k| k.rating).collect::<Vec<_>>(), [TakeRating::Bad]);
        assert!(matches!(s.project.track(t).unwrap().regions[0].content, RegionContent::Audio { offset: 0, .. }));
        // After recording, the key marks the newest take, and Discard still puts the track back.
        record_onto(&mut s, 0, 0, 1000, &[t]);
        assert!(s.mark_take(TakeRating::Good, &[]));
        assert_eq!(takes(&s, t).iter().map(|k| k.rating).collect::<Vec<_>>(), [TakeRating::Bad, TakeRating::Good]);
        s.discard_last_recording().unwrap();
        assert_eq!(takes(&s, t).len(), 1);
        // With no last recording, the key marks the take heard in the folders given.
        let folder = s.project.track(t).unwrap().regions[0].id;
        assert!(!s.mark_take(TakeRating::Good, &[]));
        assert!(s.mark_take(TakeRating::Good, &[folder]));
        assert_eq!(takes(&s, t)[0].rating, TakeRating::Good);
    }

    #[test]
    fn takes_are_coloured_and_bad_ones_deleted() {
        let mut s = session("colour");
        s.prefs.recording.color_takes = true;
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        for _ in 0..3 {
            record_onto(&mut s, 0, 0, 1000, &[t]);
        }
        let colors: Vec<Option<String>> = takes(&s, t).into_iter().map(|k| k.color).collect();
        assert_eq!(colors, TAKE_COLORS[..3].iter().map(|c| Some(c.to_string())).collect::<Vec<_>>());
        let folder = s.project.track(t).unwrap().regions[0].id;
        let ids: Vec<Id> = takes(&s, t).iter().map(|k| k.id).collect();
        s.set_take_color(folder, ids[0], None);
        s.set_take_rating(folder, ids[0], TakeRating::Bad);
        s.set_take_rating(folder, ids[2], TakeRating::Bad);
        s.delete_bad_takes(folder);
        assert_eq!(takes(&s, t).iter().map(|k| k.id).collect::<Vec<_>>(), [ids[1]]);
        // A folder of nothing but bad takes keeps its newest.
        s.set_take_rating(folder, ids[1], TakeRating::Bad);
        s.delete_bad_takes(folder);
        assert_eq!(takes(&s, t).len(), 1);
    }

    #[test]
    fn discarded_files_go_once_nothing_needs_them() {
        let mut s = session("purge");
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        record_onto(&mut s, 0, 0, 1000, &[t]);
        s.discard_last_recording().unwrap();
        let file = s.discarded[0].1.clone();
        // Saved, but Undo could still bring the recording back: the file stays.
        s.save(None).unwrap();
        assert!(file.exists() && s.discarded.len() == 1);
        // Closing the project ends the history; the saved project doesn't name it.
        s.new_project();
        assert!(!file.exists() && s.discarded.is_empty());

        // A saved project that names the file keeps it.
        let mut s = session("purge-kept");
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        record_onto(&mut s, 0, 0, 1000, &[t]);
        s.save(None).unwrap();
        s.discard_last_recording().unwrap();
        let file = s.discarded[0].1.clone();
        s.new_project();
        assert!(file.exists());
        // Once nothing in the history needs it, saving deletes it.
        let mut s = session("purge-save");
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        record_onto(&mut s, 0, 0, 1000, &[t]);
        s.discard_last_recording().unwrap();
        let file = s.discarded[0].1.clone();
        s.clear_history();
        s.save(None).unwrap();
        assert!(!file.exists() && s.discarded.is_empty());
    }
}
