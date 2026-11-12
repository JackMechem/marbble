//! Recording into take folders, and Autopunch: how a recording is set up, and turning a cycle or
//! punch recording into takes, packing what it records over into folders. Comping and the take
//! folder commands are in `comping.rs` and `folders.rs`; the comp logic itself is in
//! `crate::edit::takes`. Playback needs nothing here: `regions_dsp` expands each folder into its
//! comp pieces, so bounces and exports hear the comp too.

use crate::edit::takes::recording::RecPlan;
use crate::session::*;

/// Which recorded MIDI is kept and where its region goes: `lo..hi` on the timeline. Notes
/// that start outside the punch range are dropped and none outlasts the punch-out; in cycle
/// mode a note held across the loop point ends at the cycle end.
pub(crate) struct MidiWindow {
    pub lo: i64,
    pub hi: i64,
    wrap: i64,
    cut: i64,
    keep: Option<(i64, i64)>,
}

impl MidiWindow {
    pub fn keeps(&self, pos: i64) -> bool {
        self.keep.map(|(a, b)| pos >= a && pos < b).unwrap_or(true)
    }

    /// Where a note that went down at `on` and up at `off` ends.
    pub fn note_end(&self, on: i64, off: i64) -> i64 {
        let end = if off >= on {
            off
        } else if self.wrap < i64::MAX {
            self.wrap
        } else {
            on
        };
        end.min(self.cut)
    }
}

/// The MIDI window of a recording that started at `punch` and stopped at `stop`.
pub(crate) fn midi_window(plan: Option<RecPlan>, punch: i64, stop: i64) -> MidiWindow {
    let plain = MidiWindow { lo: punch, hi: stop, wrap: i64::MAX, cut: i64::MAX, keep: None };
    let Some(plan) = plan else { return plain };
    match (plan.cycle, plan.punch) {
        (Some((_, ce)), p) => match plan.cycle_window() {
            Some((w0, w1, _)) => MidiWindow { lo: w0, hi: w1, wrap: ce, cut: if p.is_some() { w1 } else { i64::MAX }, keep: p.map(|_| (w0, w1)) },
            None => plain,
        },
        (None, Some((a, b))) => MidiWindow { lo: punch.max(a), hi: stop.min(b), wrap: i64::MAX, cut: b, keep: Some((a, b)) },
        (None, None) => plain,
    }
}

impl Session {
    /// How a recording starting at `punch` (transport from `from`) is set up: the plan for
    /// cutting it into takes afterwards, the range where record-enabled tracks fall silent
    /// (None: throughout) and the engine positions input is captured between.
    #[allow(clippy::type_complexity)] // Three named in the doc comment, used once at the one call.
    pub(crate) fn record_plan(&self, from: i64, punch: i64, cycle: Option<(i64, i64)>) -> Result<(RecPlan, Option<(i64, i64)>, (i64, i64))> {
        let plan = RecPlan {
            from,
            cycle,
            punch: self.project.punch.range(),
            latency: self.record_latency_frames(),
            midi_latency: self.midi_latency_frames(),
            min_take: self.tempo_map().samples_per_beat_at(punch as f64).round() as i64,
        };
        let Some(capture) = plan.capture(punch) else {
            return Err(anyhow!(if cycle.is_some() { "The Autopunch range is outside the cycle" } else { "The playhead is past the punch-out point" }));
        };
        let quiet = match (plan.punch, cycle) {
            (Some(_), Some(_)) => plan.cycle_window().map(|(a, b, _)| (a, b)),
            (p, None) => p,
            (None, Some(_)) => None,
        };
        Ok((plan, quiet, capture))
    }

    /// A finished audio recording, as `finish_recording` made it (`plain`, from the file's
    /// `frames` captured from engine position `start`), cut to what the plan says: cycle passes
    /// become a take folder (or more takes in the folder already there), an autopunch take is
    /// trimmed to the punch range. None when there's nothing new to place. `recording` is the
    /// same for every track recorded at once (see `Take::pass`).
    pub(crate) fn shape_take(&mut self, plan: Option<RecPlan>, track: Id, plain: Region, start: i64, frames: i64, recording: Id) -> Option<Region> {
        let Some(plan) = plan else { return Some(plain) };
        let RegionContent::Audio { clip, .. } = plain.content else { return Some(plain) };
        if plan.cycle.is_none() {
            if plan.punch.is_none() {
                return Some(plain);
            }
            let (at, length, offset) = plan.punch_span(start, frames)?;
            return Some(self.trimmed(plain, at, length, offset));
        }
        let (fs, len, takes) = plan.cycle_takes(frames)?;
        // Recording the same cycle again adds takes to the folder there, as in Logic.
        let existing = self.project.track(track)?.regions.iter().position(|r| {
            r.takes.is_some() && r.start == fs && r.length == len && !r.is_looped() && matches!(r.content, RegionContent::Audio { offset: 0, .. })
        });
        if takes.is_empty() || (takes.len() == 1 && existing.is_none()) {
            let &(offset, a, b) = takes.first()?;
            return Some(self.trimmed(plain, fs + a, b - a, offset + a));
        }
        let mut folder = existing.and_then(|i| self.project.track(track).unwrap().regions[i].takes.clone()).unwrap_or_default();
        let numbered = folder.takes.len();
        for (k, (offset, a, b)) in takes.into_iter().enumerate() {
            let id = self.project.alloc_id();
            folder.takes.push(Take {
                id,
                name: format!("Take {}", numbered + k + 1),
                clip,
                offset,
                start: a,
                end: b,
                gain_db: 0.0,
                pass: Some((recording, k as u32)),
                ..Default::default()
            });
        }
        folder.default_comp();
        folder.store_comp();
        if let Some(i) = existing {
            let r = &mut self.project.track_mut(track)?.regions[i];
            if let RegionContent::Audio { clip: c, .. } = &mut r.content {
                *c = clip;
            }
            r.takes = Some(folder);
            return None;
        }
        let fade = self.default_fade(len);
        Some(Region {
            start: fs,
            length: len,
            content: RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
            takes: Some(folder),
            ..plain
        })
    }

    /// `plain` moved to `start..start + length`, from clip frame `offset`, with fresh fades.
    fn trimmed(&self, plain: Region, start: i64, length: i64, offset: i64) -> Region {
        let fade = self.default_fade(length);
        let mut r = Region { start, length, ..plain };
        if let RegionContent::Audio { offset: o, fade_in, fade_out, .. } = &mut r.content {
            *o = offset;
            *fade_in = fade;
            *fade_out = fade;
        }
        r
    }

    /// Places an audio recording made with "Create take folders" (Settings › Recording ›
    /// Overlapping Recordings › Audio): `region`, a plain take or a cycle recording's new folder.
    /// Recording over a take folder adds its takes to that folder; recording over audio regions
    /// packs them into a new folder with it. The new takes are heard wherever they have audio,
    /// the comp from before elsewhere, and the folder goes on top, as a new take would. Muted
    /// and looping regions are left as they are.
    pub(crate) fn place_audio_takes(&mut self, track: Id, region: Region, passes: &mut RecordingPasses) {
        let Some(t) = self.project.track(track) else { return };
        let (rs, re) = (region.start, region.start + region.length);
        let overlaps = |r: &Region| matches!(r.content, RegionContent::Audio { .. }) && !r.muted && !r.is_looped() && r.start < re && r.start + r.length > rs;
        let folder_at = t.regions.iter().position(|r| r.takes.is_some() && overlaps(r));
        let packed: Vec<Region> =
            if folder_at.is_some() { Vec::new() } else { t.regions.iter().filter(|r| r.takes.is_none() && overlaps(r)).cloned().collect() };
        let t = self.project.track_mut(track).unwrap();
        if folder_at.is_none() && packed.is_empty() {
            t.regions.push(region);
            return;
        }
        // The folder: the one recorded over, or a new one where the old regions were.
        let mut folder = match folder_at {
            Some(i) => t.regions.remove(i),
            None => Region {
                color: packed.first().and_then(|r| r.color.clone()),
                content: RegionContent::Audio { clip: 0, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() },
                takes: Some(TakeFolder::new()),
                ..region.clone()
            },
        };
        t.regions.retain(|r| !packed.iter().any(|p| p.id == r.id));
        // It covers the recording and everything packed into it.
        let lo = packed.iter().map(|r| r.start).chain([rs, folder.start]).min().unwrap();
        let hi = packed.iter().map(|r| r.start + r.length).chain([re, folder.start + folder.length]).max().unwrap();
        let RegionContent::Audio { offset: tt0, .. } = folder.content else { return };
        let f = folder.takes.as_mut().unwrap();
        // Take time at timeline position p is p + to_tt; the folder never starts at negative take time.
        let mut to_tt = tt0 - folder.start;
        if lo + to_tt < 0 {
            f.shift(-(lo + to_tt));
            // An audio folder keeps its take time in its content's offset.
            f.offset = 0;
            to_tt = -lo;
        }
        // The packed regions and what was recorded, with the passes grouped folders match by.
        let mut incoming = Vec::new();
        for (k, mut t) in packed.iter().flat_map(timeline_takes).enumerate() {
            t.pass = Some(passes.packed(k, &mut self.project));
            incoming.push(t);
        }
        for mut t in timeline_takes(&region) {
            t.pass = t.pass.or(Some((passes.recording, 0)));
            incoming.push(t);
        }
        // Oldest first, each heard wherever it has audio, so the recording ends up on top.
        for k in incoming {
            let id = self.project.alloc_id();
            let (start, end) = (k.start + to_tt, k.end + to_tt);
            let name = format!("Take {}", f.takes.len() + 1);
            f.takes.push(Take { id, name, offset: k.offset - to_tt, start, end, ..k });
            f.select(id, start, end);
        }
        f.store_comp();
        let newest = f.takes.last().map(|k| k.clip).unwrap_or(0);
        let fade = self.default_fade(hi - lo);
        folder.start = lo;
        folder.length = hi - lo;
        // The folder's own clip follows its newest take (see `TakeFolder`).
        if let RegionContent::Audio { clip, offset, fade_in, fade_out, .. } = &mut folder.content {
            *clip = newest;
            *offset = lo + to_tt;
            if folder_at.is_none() {
                (*fade_in, *fade_out) = (fade, fade);
            }
        }
        self.project.track_mut(track).unwrap().regions.push(folder);
    }
}

/// The `Take::pass`es one recording gives the takes it makes on every track: its own for what
/// it recorded, and one per region it packs into a folder (the first region packed on each
/// track shares one, the second another, and so on), so grouped folders match them across the
/// tracks recorded together.
pub(crate) struct RecordingPasses {
    pub recording: Id,
    packed: Vec<Id>,
}

impl RecordingPasses {
    pub fn new(recording: Id) -> RecordingPasses {
        RecordingPasses { recording, packed: Vec::new() }
    }

    /// The pass of the `k`th region packed on a track.
    fn packed(&mut self, k: usize, project: &mut Project) -> (Id, u32) {
        while self.packed.len() <= k {
            self.packed.push(project.alloc_id());
        }
        (self.packed[k], 0)
    }
}

/// The takes of audio region `r` with the timeline as take time: a plain region is one take
/// spanning it, a take folder's takes lie where they play.
fn timeline_takes(r: &Region) -> Vec<Take> {
    let RegionContent::Audio { clip, offset, gain_db, .. } = r.content else { return Vec::new() };
    match &r.takes {
        Some(f) => {
            let to_tt = offset - r.start;
            f.takes.iter().map(|t| Take { start: t.start - to_tt, end: t.end - to_tt, offset: t.offset + to_tt, ..t.clone() }).collect()
        }
        None => {
            vec![Take { id: 0, name: r.name.clone(), clip, offset: offset - r.start, start: r.start, end: r.start + r.length, gain_db, ..Default::default() }]
        }
    }
}

/// The notes and controller events of one recorded MIDI pass (`events`, engine positions, in
/// order), relative to the region start `start`: note-ons paired with their note-offs, notes
/// still held ending at `stop` (or as the window says), and only what the window keeps.
pub(crate) fn midi_take(events: &[(i64, [u8; 3])], w: &MidiWindow, start: i64, stop: i64) -> (Vec<Note>, Vec<CcEvent>) {
    let mut held: [Option<(i64, u8)>; 128] = [None; 128];
    let mut notes = Vec::new();
    let mut cc = Vec::new();
    for &(pos, d) in events {
        let status = d[0] & 0xF0;
        let pitch = d[1] as usize & 127;
        if let Some((kind, value)) = CcKind::from_message(d) {
            if w.keeps(pos) {
                cc.push(CcEvent { start: pos - start, kind, value });
            }
        } else if status == 0x90 && d[2] > 0 {
            if w.keeps(pos) {
                held[pitch] = Some((pos, d[2]));
            }
        } else if status == 0x80 || status == 0x90 {
            if let Some((on, vel)) = held[pitch].take() {
                notes.push(Note { start: on - start, length: (w.note_end(on, pos) - on).max(1), pitch: pitch as u8, velocity: vel, muted: false });
            }
        }
    }
    for (pitch, h) in held.iter().enumerate() {
        if let Some((on, vel)) = h {
            notes.push(Note { start: on - start, length: (w.note_end(*on, stop) - on).max(1), pitch: pitch as u8, velocity: *vel, muted: false });
        }
    }
    notes.sort_by_key(|n| n.start);
    cc.sort_by_key(|c| c.start);
    (notes, cc)
}

impl Session {
    /// Places a MIDI recording made with "Create take folders" (Settings › Recording › MIDI):
    /// `passes` (each pass's notes and controllers, relative to `start`) over `start..end`.
    /// Recording over a MIDI take folder adds the passes to it as new takes; recording over
    /// plain MIDI regions packs them into a new folder with the passes; otherwise several
    /// passes make a new folder and a single one a plain region. The newest take is heard.
    /// Returns the recorded takes (or the region).
    pub(crate) fn place_midi_takes(&mut self, track: Id, start: i64, end: i64, passes: Vec<(Vec<Note>, Vec<CcEvent>)>) -> Vec<Id> {
        let Some(t) = self.project.track(track) else { return Vec::new() };
        let name = t.name.clone();
        let overlaps = |r: &Region| matches!(r.content, RegionContent::Midi { .. }) && !r.is_looped() && r.start < end && r.start + r.length > start;
        let folder_at = t.regions.iter().position(|r| r.takes.is_some() && overlaps(r));
        let packed: Vec<Region> =
            if folder_at.is_some() { Vec::new() } else { t.regions.iter().filter(|r| r.takes.is_none() && overlaps(r)).cloned().collect() };
        if folder_at.is_none() && packed.is_empty() && passes.len() < 2 {
            let Some((notes, cc)) = passes.into_iter().next() else { return Vec::new() };
            let id = self.project.alloc_id();
            self.project.track_mut(track).unwrap().regions.push(Region::midi(id, name, start, end - start, notes, cc));
            return vec![id];
        }
        // The folder: the one recorded over, or a new one where the old regions were.
        let mut folder = match folder_at {
            Some(i) => {
                let r = &mut self.project.track_mut(track).unwrap().regions[i];
                crate::edit::takes::midi::save_midi_take(r);
                r.clone()
            }
            None => {
                let id = self.project.alloc_id();
                Region {
                    color: packed.first().and_then(|r| r.color.clone()),
                    takes: Some(TakeFolder::new()),
                    ..Region::midi(id, name.clone(), start, end - start, Vec::new(), Vec::new())
                }
            }
        };
        // It covers the recording and everything packed into it.
        let lo = packed.iter().map(|r| r.start).chain([start, folder.start]).min().unwrap();
        let hi = packed.iter().map(|r| r.start + r.length).chain([end, folder.start + folder.length]).max().unwrap();
        let f = folder.takes.as_mut().unwrap();
        // Take time at timeline position p is p + to_tt; the folder never starts at negative take time.
        let mut to_tt = f.offset - folder.start;
        if lo + to_tt < 0 {
            f.shift(-(lo + to_tt));
            to_tt = f.offset - folder.start;
        }
        f.offset = lo + to_tt;
        let add = |f: &mut TakeFolder, id: Id, at: i64, len: i64, notes: &[Note], cc: &[CcEvent]| {
            let name = format!("Take {}", f.takes.len() + 1);
            let tt = at + to_tt;
            let notes = Arc::new(notes.iter().map(|n| Note { start: n.start + tt, ..*n }).collect());
            let cc = Arc::new(cc.iter().map(|c| CcEvent { start: c.start + tt, ..*c }).collect());
            f.takes.push(Take { id, name, clip: 0, offset: 0, start: tt, end: tt + len, gain_db: 0.0, notes, cc, ..Default::default() });
        };
        for r in &packed {
            if let RegionContent::Midi { notes, cc } = &r.content {
                let id = self.project.alloc_id();
                add(f, id, r.start, r.length, notes, cc);
            }
        }
        let mut recorded = Vec::new();
        for (notes, cc) in &passes {
            let id = self.project.alloc_id();
            add(f, id, start, end - start, notes, cc);
            recorded.push(id);
        }
        let newest = f.takes.last().map(|t| t.id).unwrap();
        f.select_whole(newest);
        f.store_comp();
        folder.start = lo;
        folder.length = hi - lo;
        crate::edit::takes::midi::load_midi_take(&mut folder);
        let t = self.project.track_mut(track).unwrap();
        match folder_at {
            Some(i) => t.regions[i] = folder,
            None => {
                let at = t.regions.iter().position(|r| packed.iter().any(|p| p.id == r.id)).unwrap_or(t.regions.len());
                t.regions.retain(|r| !packed.iter().any(|p| p.id == r.id));
                t.regions.insert(at.min(t.regions.len()), folder);
            }
        }
        recorded
    }
}
