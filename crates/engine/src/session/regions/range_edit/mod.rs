//! Edits for the Audio Track editor: splitting regions at several points in one step, and the
//! actions on a selected time range (trim, silence, delete, normalize, reverse, fades). They
//! are all non-destructive: they split regions and change region settings, and only Reverse
//! writes a new file (as Reverse on a whole region does), leaving the original intact.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::audio_file::clip::Clip;
use crate::model::*;
use crate::session::Session;

/// What `Session::edit_range` does with the selected range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeEdit {
    /// Split at both ends of the range.
    Split,
    /// Trim the regions to the range.
    Trim,
    /// Split, then mute the part inside the range. Unmute it to undo the silence later.
    Silence,
    /// Split, then delete the part inside the range, leaving a gap.
    Delete,
    /// Split, then set the inside part's gain so its peak reaches 0 dBFS.
    Normalize,
    /// Split, then reverse the inside part.
    Reverse,
    /// Fade in over the range: split at its start (if inside a region) and set that piece's
    /// fade-in to end where the range ends.
    FadeIn,
    /// Fade out over the range: split at its end and set the fade-out to start at its start.
    FadeOut,
}

impl RangeEdit {
    /// The undo step's name.
    pub fn label(self) -> &'static str {
        match self {
            RangeEdit::Split => "Split at Selection",
            RangeEdit::Trim => "Trim to Selection",
            RangeEdit::Silence => "Silence Selection",
            RangeEdit::Delete => "Delete Selection",
            RangeEdit::Normalize => "Normalize Selection",
            RangeEdit::Reverse => "Reverse Selection",
            RangeEdit::FadeIn => "Fade In to Selection",
            RangeEdit::FadeOut => "Fade Out to Selection",
        }
    }
}

/// What the editor's info readout shows about a clip.
#[derive(Debug, Clone, Serialize)]
pub struct ClipInfo {
    pub frames: i64,
    pub channels: usize,
    pub sample_rate: u32,
    /// As stored in the project: relative to the project folder when it's inside it.
    pub path: String,
}

/// The part of `r` between timeline positions `a` and `b` (clamped to the region), with id
/// `id`. Fades are kept only on the original's own ends.
pub(crate) fn sub_region(r: &Region, a: i64, b: i64, id: Id) -> Region {
    let (rs, re) = (r.start, r.start + r.length);
    let (a, b) = (a.max(rs), b.min(re));
    let cut = a - rs;
    let length = (b - a).max(1);
    let mut out = Region { id, start: a, length, ..r.clone() };
    match &mut out.content {
        RegionContent::Audio { offset, fade_in, fade_out, fade_shape, .. } => {
            *offset += cut;
            // A cut end loses its fade, and its fade type (a speed fade or a crossfade).
            if a > rs {
                *fade_in = 0;
                fade_shape.in_type = Default::default();
            }
            if b < re {
                *fade_out = 0;
                fade_shape.out_type = Default::default();
            }
            *fade_in = (*fade_in).min(length);
            *fade_out = (*fade_out).min(length - *fade_in);
        }
        RegionContent::Midi { notes, cc } => {
            notes.retain(|n| n.start >= cut && n.start < cut + length);
            notes.iter_mut().for_each(|n| n.start -= cut);
            // Controllers start from the values in effect at the cut.
            *cc = crate::edit::midi_content::cc_from(cc, cut).into_iter().filter(|c| c.start < length).collect();
        }
    }
    // Each piece of a MIDI take folder is a folder over its part of the takes.
    if let (Some(f), RegionContent::Midi { .. }) = (&mut out.takes, &out.content) {
        f.offset += cut;
    }
    out
}

impl Session {
    /// A region's timeline span, start to end.
    fn region_span(&self, id: Id) -> Option<(i64, i64)> {
        let (t, i) = self.find_region(id)?;
        let r = &self.project.track(t)?.regions[i];
        Some((r.start, r.start + r.length))
    }

    /// Splits the given regions at every position that falls inside them, as one edit. Each
    /// region is replaced where it is in its track's list (so it keeps its place in what hides
    /// what) by its pieces, the first keeping its id. Unlike `split_at`, only the first piece
    /// keeps the fade-in and only the last the fade-out. Returns every piece's id, the regions
    /// not cut included.
    pub fn split_regions_at(&mut self, ids: &[Id], positions: &[i64]) -> Vec<Id> {
        let ids = &self.unloop(ids);
        let mut cuts = positions.to_vec();
        cuts.sort_unstable();
        cuts.dedup();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut touched = Vec::new();
        for &id in ids.iter().filter(|id| seen.insert(**id)) {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let r = self.project.track(track).unwrap().regions[idx].clone();
            let (rs, re) = (r.start, r.start + r.length);
            let mut edges = vec![rs];
            // Snap Edits to Zero Crossings moves each cut to this region's nearest crossing.
            let mut inner: Vec<i64> = cuts.iter().filter(|&&p| p > rs && p < re).map(|&p| self.zero_cut(&r, p)).collect();
            inner.sort_unstable();
            inner.dedup();
            edges.extend(inner);
            edges.push(re);
            if edges.len() == 2 {
                out.push(id);
                continue;
            }
            let mut pieces = Vec::with_capacity(edges.len() - 1);
            for (k, w) in edges.windows(2).enumerate() {
                let pid = if k == 0 { id } else { self.project.alloc_id() };
                pieces.push(sub_region(&r, w[0], w[1], pid));
            }
            out.extend(pieces.iter().map(|p| p.id));
            self.project.track_mut(track).unwrap().regions.splice(idx..=idx, pieces);
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        // One engine update per track, however many cuts.
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        out
    }

    /// Splits the regions at `a` and `b` and returns the pieces inside the range.
    fn split_range(&mut self, ids: &[Id], a: i64, b: i64) -> Vec<Id> {
        let pieces = self.split_regions_at(ids, &[a, b]);
        pieces.into_iter().filter(|&id| self.region_span(id).is_some_and(|(s, e)| s >= a && e <= b)).collect()
    }

    /// Applies `action` to the parts of the given regions between timeline positions `start`
    /// and `end`. Returns the regions that now hold the range (to select them); empty for
    /// Delete, or when no region overlaps the range. With Snap Edits to Zero Crossings each
    /// region's range ends move to its own nearest crossings.
    pub fn edit_range(&mut self, ids: &[Id], start: i64, end: i64, action: RangeEdit) -> Result<Vec<Id>> {
        if !self.project.snap_zero_crossings {
            return self.edit_range_at(ids, start, end, action);
        }
        let ids = self.unloop(ids);
        let (a, b) = (start.min(end).max(0), start.max(end));
        let mut out = Vec::new();
        for ((ga, gb), group) in self.zero_range_groups(&ids, a, b) {
            out.extend(self.edit_range_at(&group, ga, gb, action)?);
        }
        Ok(out)
    }

    /// [`Session::edit_range`] at exactly `start` and `end`.
    fn edit_range_at(&mut self, ids: &[Id], start: i64, end: i64, action: RangeEdit) -> Result<Vec<Id>> {
        let ids = &self.unloop(ids);
        let (a, b) = (start.min(end).max(0), start.max(end));
        let picked: Vec<Id> = ids.iter().copied().filter(|&id| self.region_span(id).is_some_and(|(s, e)| s < b && e > a)).collect();
        if b <= a || picked.is_empty() {
            return Ok(Vec::new());
        }
        Ok(match action {
            RangeEdit::Split => self.split_range(&picked, a, b),
            RangeEdit::Trim => {
                self.edit_regions(&picked, |r, _| *r = sub_region(r, a, b, r.id));
                picked
            }
            RangeEdit::Silence => {
                let inside = self.split_range(&picked, a, b);
                self.edit_regions(&inside, |r, _| r.muted = true);
                inside
            }
            RangeEdit::Delete => {
                let inside = self.split_range(&picked, a, b);
                self.delete_regions(&inside);
                Vec::new()
            }
            RangeEdit::Normalize => {
                let inside = self.split_range(&picked, a, b);
                self.normalize_regions(&inside);
                inside
            }
            RangeEdit::Reverse => {
                let inside = self.split_range(&picked, a, b);
                self.reverse_regions(&inside)?;
                inside
            }
            RangeEdit::FadeIn => {
                let pieces = self.split_regions_at(&picked, &[a]);
                let faded: Vec<Id> = pieces.into_iter().filter(|&id| self.region_span(id).is_some_and(|(s, _)| s >= a && s < b)).collect();
                self.edit_regions(&faded, |r, _| {
                    let len = (b - r.start).min(r.length);
                    if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                        *fade_in = len;
                        *fade_out = (*fade_out).min(r.length - len);
                    }
                });
                faded
            }
            RangeEdit::FadeOut => {
                let pieces = self.split_regions_at(&picked, &[b]);
                let faded: Vec<Id> = pieces.into_iter().filter(|&id| self.region_span(id).is_some_and(|(_, e)| e <= b && e > a)).collect();
                self.edit_regions(&faded, |r, _| {
                    let len = (r.start + r.length - a).min(r.length);
                    if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                        *fade_out = len;
                        *fade_in = (*fade_in).min(r.length - len);
                    }
                });
                faded
            }
        })
    }

    /// The clip's audio, shared, so it can be analysed without holding the session.
    /// Range edits work on real regions: looping ones among `ids` become real copies first (as
    /// Split does). Returns `ids` plus the copies.
    pub(crate) fn unloop(&mut self, ids: &[Id]) -> Vec<Id> {
        let looped: Vec<Id> =
            ids.iter().copied().filter(|id| self.find_region(*id).is_some_and(|(t, i)| self.project.track(t).unwrap().regions[i].is_looped())).collect();
        let mut all = ids.to_vec();
        if !looped.is_empty() {
            all.extend(self.convert_loops(&looped));
        }
        all
    }

    pub fn clip_audio(&self, id: Id) -> Option<Arc<Clip>> {
        self.clips.get(&id).cloned()
    }

    pub fn clip_info(&self, id: Id) -> Option<ClipInfo> {
        let c = self.clips.get(&id)?;
        let path = self.project.clips.iter().find(|r| r.id == id).map(|r| r.path.clone()).unwrap_or_else(|| c.path.to_string_lossy().to_string());
        Some(ClipInfo { frames: c.frames() as i64, channels: c.channels.len(), sample_rate: c.sample_rate, path })
    }
}

#[cfg(test)]
mod tests;
