//! What the last recording made: Discard Last Recording, and the keys that mark its take good
//! or bad.
//!
//! `place_recording` (`transport.rs`) notes each recording in a [`LastRecording`]: every track
//! it changed, with its regions before and after, and the clips and MIDI it made. Discarding
//! puts back the tracks that are still as the recording left them, and on a track edited
//! since takes out just what was recorded. The discarded files are deleted once nothing can
//! need them ([`Session::purge_discarded`]).

use crate::edit::takes::recording::TAKE_COLORS;
use crate::session::*;

/// What a recording made, for Discard Last Recording and the rating keys.
#[derive(Debug, Clone)]
pub(crate) struct LastRecording {
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
    for sub in [crate::session::project::backups::BACKUPS_DIR, crate::session::project::alternatives::ALTERNATIVES_DIR] {
        json_in(&dir.join(sub), 2, &mut out);
    }
    out
}

impl Session {
    /// After `place_recording` has placed a recording on the `touched` tracks (`before`: the
    /// regions of the tracks it recorded to, as they were, and the `clips` and `midi` it made):
    /// notes it as the last recording, gives its take the rating marked while recording and,
    /// with Settings › Recording › Color takes, colours the takes of its folders.
    pub(crate) fn note_recording(&mut self, recording: Id, before: Vec<(Id, Vec<Region>)>, touched: &[Id], clips: Vec<Id>, midi: Vec<Id>) {
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
            crate::edit::takes::midi::save_midi_take(r);
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
            crate::edit::takes::midi::load_midi_take(r);
        }
        t.regions.retain(|r| r.takes.as_ref().is_none_or(|f| !f.takes.is_empty()));
        changed
    }

    /// Deletes the files of discarded recordings that nothing can need any more: not the
    /// project, an undo or redo step, nor a document in the project folder (project.json, its
    /// autosave, the backups or another alternative). `closing`: the project is being closed,
    /// so its undo history is going and only the documents on disk count; files they still
    /// name are kept for good.
    pub(crate) fn purge_discarded(&mut self, closing: bool) {
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
    pub(crate) fn close_recordings(&mut self) {
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
mod tests;
