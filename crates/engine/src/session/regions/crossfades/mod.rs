//! Fade shapes and crossfades on audio regions: the inspector's curve and type settings, the
//! Crossfade command, and Logic's X-Fade drag mode, which crossfades regions that a move, trim
//! or paste leaves overlapping. The shapes themselves are in `edit/fades/`.

use crate::edit::fades::{following, Crossfade};
use crate::session::*;

impl Session {
    /// Settings › Editing › Crossfade Time, in frames.
    pub fn crossfade_frames(&self) -> i64 {
        self.ms_frames(self.prefs.editing.crossfade_ms.clamp(0.0, 10_000.0) as f64)
    }

    /// Sets fade curves (-1..1) and types; None leaves that setting alone. Choosing a
    /// crossfade type for a region that has no fade-out but meets the next region gives it
    /// one: the overlap, or the Crossfade Time where the two just touch.
    pub fn set_region_fade_shape(
        &mut self,
        ids: &[Id],
        in_curve: Option<f32>,
        out_curve: Option<f32>,
        in_type: Option<FadeInType>,
        out_type: Option<FadeOutType>,
    ) {
        let default_len = self.crossfade_frames();
        let starting: Vec<(Id, i64)> = ids
            .iter()
            .filter_map(|&id| {
                let (track, idx) = self.find_region(id)?;
                let regions = &self.project.track(track)?.regions;
                let a = &regions[idx];
                let b = following(regions, a)?;
                Some((id, (a.end() - b.start).max(0)))
            })
            .collect();
        self.edit_regions(ids, |r, _| {
            let id = r.id;
            let len = r.length;
            let RegionContent::Audio { fade_out, fade_shape: s, .. } = &mut r.content else { return };
            if let Some(c) = in_curve {
                s.in_curve = c.clamp(-1.0, 1.0);
            }
            if let Some(c) = out_curve {
                s.out_curve = c.clamp(-1.0, 1.0);
            }
            if let Some(t) = in_type {
                s.in_type = t;
            }
            if let Some(t) = out_type {
                s.out_type = t;
                if t.is_crossfade() && *fade_out == 0 {
                    if let Some(&(_, overlap)) = starting.iter().find(|(r, _)| *r == id) {
                        *fade_out = (if overlap > 0 { overlap } else { default_len }).min(len);
                    }
                }
            }
        });
    }

    /// Crossfades each region with the one that follows it on its track (adjacent or
    /// overlapping) over `length` frames, as `kind` (by default the region's crossfade type,
    /// or EqP). A length of 0 removes the crossfade. Regions followed by a gap are left alone.
    pub fn crossfade_regions(&mut self, ids: &[Id], length: i64, kind: Option<FadeOutType>) {
        let meets: Vec<Id> = ids
            .iter()
            .copied()
            .filter(|&id| {
                let Some((track, idx)) = self.find_region(id) else { return false };
                let regions = &self.project.track(track).unwrap().regions;
                following(regions, &regions[idx]).is_some()
            })
            .collect();
        self.edit_regions(&meets, |r, _| {
            let len = r.passes().last().map(|p| p.1).unwrap_or(r.length);
            let RegionContent::Audio { fade_out, fade_shape: s, .. } = &mut r.content else { return };
            if length <= 0 {
                *fade_out = 0;
                s.out_type = FadeOutType::Out;
            } else {
                *fade_out = length.min(len);
                s.out_type = kind.filter(|k| k.is_crossfade()).unwrap_or(if s.out_type.is_crossfade() { s.out_type } else { FadeOutType::EqP });
            }
        });
    }

    /// Turns the crossfades between a looping region's repetitions on or off (see
    /// [`crate::edit::fades::FadeShape::loop_xfade`]).
    pub fn set_loop_crossfade(&mut self, ids: &[Id], on: bool) {
        self.edit_regions(ids, |r, _| {
            if let RegionContent::Audio { fade_shape, .. } = &mut r.content {
                fade_shape.loop_xfade = on;
            }
        });
    }

    /// Chooses the drag mode (Logic's Drag pop-up). Not an undoable edit.
    pub fn set_drag_mode(&mut self, mode: DragMode) {
        self.project.drag_mode = mode;
        self.dirty = true;
    }

    /// Every crossfade in the project, as playback hears it (for drawing them).
    pub fn crossfades(&self) -> Vec<Crossfade> {
        let frames = |c: Id| self.clips.get(&c).map(|c| c.frames() as i64);
        self.project.tracks.iter().filter(|t| t.kind == TrackKind::Audio).flat_map(|t| crate::edit::fades::crossfades(&t.regions, frames)).collect()
    }

    /// The crossfades `ids` take part in, as (outgoing, incoming), before an edit that may
    /// pull them apart (see [`Session::auto_crossfade`]).
    pub(crate) fn crossfade_pairs(&self, ids: &[Id]) -> Vec<(Id, Id)> {
        if self.project.drag_mode != DragMode::XFade {
            return Vec::new();
        }
        let mut out = Vec::new();
        for t in &self.project.tracks {
            for a in &t.regions {
                let RegionContent::Audio { fade_out, fade_shape, .. } = &a.content else { continue };
                if !fade_shape.out_type.is_crossfade() || *fade_out == 0 {
                    continue;
                }
                if let Some(b) = following(&t.regions, a) {
                    if ids.contains(&a.id) || ids.contains(&b.id) {
                        out.push((a.id, b.id));
                    }
                }
            }
        }
        out
    }

    /// After a move, trim or paste of `ids` in the X-Fade drag mode: audio regions that now
    /// overlap crossfade over the overlap, and ones that now just touch over the Crossfade
    /// Time (X with the Crossfade Curve, unless the region already had a crossfade type).
    /// Crossfades from `before` whose regions no longer meet are removed. Does nothing in the
    /// Overlap mode.
    pub(crate) fn auto_crossfade(&mut self, ids: &[Id], before: Vec<(Id, Id)>) {
        if self.project.drag_mode != DragMode::XFade {
            return;
        }
        let (touching, curve) = (self.crossfade_frames(), self.prefs.editing.crossfade_curve.clamp(-1.0, 1.0));
        // (region, new fade-out length) for every change.
        let mut set: Vec<(Id, i64)> = Vec::new();
        for (a, _) in before {
            let Some((track, idx)) = self.find_region(a) else { continue };
            let regions = &self.project.track(track).unwrap().regions;
            if following(regions, &regions[idx]).is_none() {
                set.push((a, 0));
            }
        }
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let regions = &self.project.track(track).unwrap().regions;
            let r = &regions[idx];
            if !matches!(r.content, RegionContent::Audio { .. }) {
                continue;
            }
            for o in regions.iter().filter(|o| o.id != id) {
                let (a, b) = if o.start < r.start { (o, r) } else { (r, o) };
                if following(regions, a).map(|f| f.id) == Some(b.id) {
                    let overlap = a.end() - b.start;
                    // Regions that already crossfade keep their length where they just touch.
                    let had = matches!(&a.content, RegionContent::Audio { fade_out, fade_shape, .. } if *fade_out > 0 && fade_shape.out_type.is_crossfade());
                    if overlap <= 0 && had {
                        set.retain(|(x, _)| *x != a.id);
                        continue;
                    }
                    set.retain(|(x, _)| *x != a.id);
                    set.push((a.id, if overlap > 0 { overlap } else { touching }));
                }
            }
        }
        let ids: Vec<Id> = set.iter().map(|(id, _)| *id).collect();
        if ids.is_empty() {
            return;
        }
        self.edit_regions(&ids, |r, _| {
            let len = r.passes().last().map(|p| p.1).unwrap_or(r.length);
            let Some(&(_, l)) = set.iter().find(|(id, _)| *id == r.id) else { return };
            let RegionContent::Audio { fade_out, fade_shape: s, .. } = &mut r.content else { return };
            if l == 0 {
                *fade_out = 0;
                s.out_type = FadeOutType::Out;
            } else {
                *fade_out = l.min(len);
                if !s.out_type.is_crossfade() {
                    s.out_type = FadeOutType::X;
                    s.out_curve = curve;
                }
            }
        });
    }

    /// `picked` regions of track `t` mixed over `lo..hi` as they play, crossfades included
    /// (for Join): `channels` lists of samples.
    pub(crate) fn mix_regions(&self, t: &Track, picked: &[Region], lo: i64, hi: i64, channels: usize) -> Vec<Vec<f32>> {
        let only = Track { regions: picked.to_vec(), frozen: None, ..t.clone() };
        let list = self.regions_dsp(&only);
        let n = (hi - lo).max(0) as usize;
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        crate::dsp::mix_regions(&list, lo, &mut l, &mut r);
        if channels == 1 {
            vec![l]
        } else {
            vec![l, r]
        }
    }

    fn ms_frames(&self, ms: f64) -> i64 {
        (ms * self.project.sample_rate as f64 / 1000.0).round() as i64
    }
}

#[cfg(test)]
mod tests;
