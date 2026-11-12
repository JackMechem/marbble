//! Where a finished recording goes over what is already there, as Settings › Recording's
//! Overlapping Recordings rules say: audio takes, and MIDI merged into a region or replacing
//! what it recorded over.

use crate::model::{CcEvent, Id, Note, Region, RegionContent};
use crate::prefs::AudioOverlap;
use crate::session::*;

impl Session {
    /// Puts a finished audio take on `track`, following Settings › Recording › Overlapping
    /// Recordings. Returns the track it landed on.
    /// `passes` are the takes' `Take::pass`es (see `RecordingPasses`).
    pub(crate) fn place_take(&mut self, track: Id, region: Region, passes: &mut crate::session::takes::recording::RecordingPasses) -> Option<Id> {
        let (rs, re) = (region.start, region.start + region.length);
        let overlaps = |r: &Region| matches!(r.content, RegionContent::Audio { .. }) && r.start < re && r.start + r.length > rs;
        let any = self.project.track(track)?.regions.iter().any(overlaps);
        let dest = match self.prefs.recording.audio_mode() {
            AudioOverlap::TakeFolders if any => {
                self.place_audio_takes(track, region, passes);
                return Some(track);
            }
            AudioOverlap::NewTrack if any => {
                // A track with the same settings and plug-ins, right below, not record-enabled.
                *self.duplicate_tracks(&[track], false).first()?
            }
            AudioOverlap::Replace if any => {
                let ids: Vec<Id> = self.project.track(track)?.regions.iter().filter(|r| overlaps(r)).map(|r| r.id).collect();
                for id in ids {
                    let right_id = self.project.alloc_id();
                    let t = self.project.track_mut(track)?;
                    let Some(i) = t.regions.iter().position(|r| r.id == id) else { continue };
                    let (start, end) = (t.regions[i].start, t.regions[i].start + t.regions[i].length);
                    if start >= rs && end <= re {
                        t.regions.remove(i);
                        continue;
                    }
                    let r = &mut t.regions[i];
                    if start < rs && end > re {
                        // The take lands in the middle: keep both ends, the right one where the
                        // original sat so the layering doesn't change.
                        let mut right = r.clone();
                        right.id = right_id;
                        right.start = re;
                        right.length = end - re;
                        if let RegionContent::Audio { offset, fade_in, .. } = &mut right.content {
                            *offset += re - start;
                            *fade_in = 0;
                        }
                        let len = rs - start;
                        r.length = len;
                        if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                            *fade_out = 0;
                            *fade_in = (*fade_in).min(len);
                        }
                        t.regions.insert(i + 1, right);
                    } else if start < rs {
                        let len = rs - start;
                        r.length = len;
                        if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                            *fade_out = (*fade_out).min(len);
                            *fade_in = (*fade_in).min(len - *fade_out);
                        }
                    } else {
                        let len = end - re;
                        r.start = re;
                        r.length = len;
                        if let RegionContent::Audio { offset, fade_in, fade_out, .. } = &mut r.content {
                            *offset += re - start;
                            *fade_in = (*fade_in).min(len);
                            *fade_out = (*fade_out).min(len - *fade_in);
                        }
                    }
                }
                track
            }
            _ => track,
        };
        self.project.track_mut(dest)?.regions.push(region);
        Some(dest)
    }

    /// Merges recorded notes and controllers (relative to `start`) into the MIDI region they
    /// overlap on `track`, growing it to cover them. False when there is none to merge into.
    pub(crate) fn merge_midi_take(&mut self, track: Id, start: i64, end: i64, notes: &[Note], cc: &[CcEvent]) -> bool {
        let Some(t) = self.project.track_mut(track) else { return false };
        let Some(r) = t.regions.iter_mut().rev().find(|r| matches!(r.content, RegionContent::Midi { .. }) && r.start < end && r.start + r.length > start)
        else {
            return false;
        };
        let new_start = r.start.min(start);
        let new_end = (r.start + r.length).max(end);
        let RegionContent::Midi { notes: existing, cc: existing_cc } = &mut r.content else { return false };
        let (existing, existing_cc) = (Arc::make_mut(existing), Arc::make_mut(existing_cc));
        let shift_old = r.start - new_start;
        for n in existing.iter_mut() {
            n.start += shift_old;
        }
        for c in existing_cc.iter_mut() {
            c.start += shift_old;
        }
        existing.extend(notes.iter().map(|n| Note { start: n.start + start - new_start, ..*n }));
        existing.sort_by_key(|n| n.start);
        existing_cc.extend(cc.iter().map(|c| CcEvent { start: c.start + start - new_start, ..*c }));
        existing_cc.sort_by_key(|c| c.start);
        r.start = new_start;
        r.length = new_end - new_start;
        true
    }

    /// Record in Replace mode: clears `range` (timeline samples) in every MIDI region on `track`
    /// that it overlaps, drops regions left empty inside it, and merges the recording (relative
    /// to `start`) into the region it overlaps. False when there is none to merge into, so the
    /// recording becomes a region of its own. Only a region's own length counts, not its loop
    /// repetitions; the stored notes are edited, not the ones its playback parameters make.
    pub(crate) fn replace_midi_take(&mut self, track: Id, range: (i64, i64), start: i64, end: i64, notes: &[Note], cc: &[CcEvent]) -> bool {
        let (lo, hi) = range;
        let Some(t) = self.project.track_mut(track) else { return false };
        t.regions.retain_mut(|r| {
            let (rs, re) = (r.start, r.start + r.length);
            let RegionContent::Midi { notes, cc } = &mut r.content else { return true };
            if rs >= hi || re <= lo {
                return true;
            }
            crate::edit::midi_content::erase_range(Arc::make_mut(notes), Arc::make_mut(cc), lo - rs, hi - rs, re - rs);
            let emptied = notes.is_empty() && cc.is_empty() && rs >= lo && re <= hi;
            !emptied
        });
        self.merge_midi_take(track, start, end, notes, cc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::scratch_session as session;

    #[test]
    fn replace_trims_overlapped_regions() {
        let (mut s, _scratch) = session("replace");
        s.prefs.recording.audio_overlap = AudioOverlap::Replace;
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let audio = |id, start, length| Region {
            id,
            name: "r".into(),
            start,
            length,
            muted: false,
            color: None,
            content: RegionContent::Audio { clip: 0, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() },
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        };
        s.project.track_mut(t).unwrap().regions.push(audio(100, 0, 1000));
        let mut passes = crate::session::takes::recording::RecordingPasses::new(0);
        s.place_take(t, audio(101, 400, 200), &mut passes).unwrap();
        let mut spans: Vec<(i64, i64, i64)> = s
            .project
            .track(t)
            .unwrap()
            .regions
            .iter()
            .map(|r| match r.content {
                RegionContent::Audio { offset, .. } => (r.start, r.length, offset),
                _ => unreachable!(),
            })
            .collect();
        spans.sort();
        assert_eq!(spans, vec![(0, 400, 0), (400, 200, 0), (600, 400, 600)]);

        s.prefs.recording.audio_overlap = AudioOverlap::NewTrack;
        let n = s.project.tracks.len();
        let dest = s.place_take(t, audio(102, 500, 100), &mut passes).unwrap();
        assert_ne!(dest, t);
        assert_eq!(s.project.tracks.len(), n + 1);
        assert!(!s.project.track(dest).unwrap().armed);
    }

    #[test]
    fn midi_merge_grows_the_region() {
        let (mut s, _scratch) = session("merge");
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let note = |start| Note { start, length: 10, pitch: 60, velocity: 100, muted: false };
        s.project.track_mut(t).unwrap().regions.push(Region {
            id: 500,
            name: "m".into(),
            start: 1000,
            length: 1000,
            muted: false,
            color: None,
            content: RegionContent::midi(vec![note(0)], Vec::new()),
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        });
        assert!(s.merge_midi_take(t, 500, 1500, &[note(0), note(900)], &[]));
        let r = &s.project.track(t).unwrap().regions[0];
        assert_eq!((r.start, r.length), (500, 1500));
        let RegionContent::Midi { notes, .. } = &r.content else { unreachable!() };
        let starts: Vec<i64> = notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, vec![0, 500, 900]);
        assert!(!s.merge_midi_take(t, 5000, 6000, &[note(0)], &[]));
    }

    #[test]
    fn midi_replace_clears_the_recorded_range() {
        let (mut s, _scratch) = session("replace-midi");
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let note = |start, pitch| Note { start, length: 10, pitch, velocity: 100, muted: false };
        let modw = |start, value| CcEvent { start, kind: crate::model::CcKind::Controller(1), value };
        let region = |id, start, length, notes: Vec<Note>, cc: Vec<CcEvent>| Region {
            id,
            name: "m".into(),
            start,
            length,
            muted: false,
            color: None,
            content: RegionContent::midi(notes, cc),
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        };
        let regions = &mut s.project.track_mut(t).unwrap().regions;
        regions.push(region(500, 1000, 2000, vec![note(0, 60), note(400, 62), note(600, 64), note(1500, 66)], vec![modw(0, 5), modw(450, 90)]));
        // A region wholly inside the range goes; one past it keeps what's outside.
        regions.push(region(501, 3000, 200, vec![note(0, 70)], vec![]));
        regions.push(region(502, 3200, 1000, vec![note(0, 71), note(900, 72)], vec![]));
        let take = vec![note(100, 65)];
        // Recorded from 1300 to 3300; the take's region spans 1000..4000.
        assert!(s.replace_midi_take(t, (1300, 3300), 1000, 4000, &take, &[modw(100, 40)]));
        let regs = &s.project.track(t).unwrap().regions;
        assert_eq!(regs.iter().map(|r| r.id).collect::<Vec<_>>(), vec![500, 502]);
        // The recording merges into the newest overlapping region, which grew to hold it.
        let r = &regs[1];
        assert_eq!((r.start, r.length), (1000, 3200));
        let RegionContent::Midi { notes, .. } = &r.content else { unreachable!() };
        assert_eq!(notes.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(), vec![(100, 65), (3100, 72)]);
        let RegionContent::Midi { notes, cc } = &regs[0].content else { unreachable!() };
        assert_eq!(notes.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(), vec![(0, 60)]);
        // The region ends inside the range: the mod change there is just gone.
        assert_eq!(**cc, vec![modw(0, 5)]);
        // Nothing on the track there: no region to merge into.
        assert!(!s.replace_midi_take(t, (9000, 9500), 9000, 10000, &take, &[]));
    }
}
