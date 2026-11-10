//! What a take folder plays: its comp expanded into [`CompPiece`]s, one per stretch of a take,
//! with short crossfades where one take hands over to the next.

use crate::model::{Id, Region, RegionContent, Take, TakeFolder};

/// A stretch of one take as a folder plays it: timeline `start..end`, from clip frame
/// `offset`, with fades as `RegionDsp::Audio` takes them (timeline start, length).
#[derive(Debug, Clone, PartialEq)]
pub struct CompPiece {
    pub take: Id,
    pub clip: Id,
    pub gain_db: f32,
    pub start: i64,
    pub end: i64,
    pub offset: i64,
    pub fade_in: (i64, i64),
    pub fade_out: (i64, i64),
}

/// The comp's stretches within one pass of the folder: take time `tt0..tt0 + len`, where
/// `tt0` is the folder's offset. Unclipped to the takes' audio means a stretch is silent.
fn raw_pieces(f: &TakeFolder, tt0: i64, len: i64) -> Vec<(&Take, i64, i64)> {
    let mut out = Vec::new();
    for s in &f.comp {
        let Some(t) = f.take(s.take) else { continue };
        let (a, b) = (s.start.max(t.start).max(tt0), s.end.min(t.end).min(tt0 + len));
        if a < b {
            out.push((t, a, b));
        }
    }
    out
}

/// Where a take folder pass meets a neighbouring region it crossfades with: the folder plays
/// `lead` frames before the pass and `tail` frames after it, and the crossfade's timeline
/// (start, length) replaces the folder's own fade there.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PassEdges {
    pub lead: i64,
    pub tail: i64,
    pub fade_in: Option<(i64, i64)>,
    pub fade_out: Option<(i64, i64)>,
}

/// What a take folder plays over its pass at timeline `ps` (length `pl`; see
/// [`Region::passes`]). Where two stretches touch, each reaches `xfade / 2` past the boundary
/// (as far as its take has audio) and they crossfade over it. A stretch next to silence fades
/// in or out within itself. The folder's own fades shape its first and last stretch.
pub fn pass_pieces(r: &Region, f: &TakeFolder, ps: i64, pl: i64, xfade: i64) -> Vec<CompPiece> {
    pass_pieces_edged(r, f, ps, pl, xfade, PassEdges::default())
}

/// [`pass_pieces`] for a pass that crossfades with a neighbour at either end (`edges`): the
/// stretches heard at that end reach into the crossfade and fade with it.
pub fn pass_pieces_edged(r: &Region, f: &TakeFolder, ps: i64, pl: i64, xfade: i64, edges: PassEdges) -> Vec<CompPiece> {
    let RegionContent::Audio { offset: tt0, fade_in, fade_out, .. } = r.content else { return Vec::new() };
    let (lead, tail) = (edges.lead.max(0), edges.tail.max(0));
    let raw = raw_pieces(f, tt0 - lead, pl + lead + tail);
    let end = r.end();
    let first_pass = ps == r.start;
    let last_pass = ps + pl >= end;
    let last_len = r.passes().last().map(|p| p.1).unwrap_or(r.length);
    let fi = fade_in.clamp(0, r.length);
    let fo = if r.is_looped() { fade_out.clamp(0, last_len) } else { fade_out.clamp(0, r.length - fi) };
    let own_in = edges.fade_in.unwrap_or((r.start, fi));
    let own_out = edges.fade_out.unwrap_or((end - fo, fo));
    let h = (xfade / 2).max(0);
    let tl = |t: i64| ps + (t - tt0);
    // Half-width of the crossfade at the boundary in front of piece i (0 if none).
    let boundary = |i: usize| -> i64 {
        if i == 0 || i >= raw.len() || raw[i - 1].2 != raw[i].1 {
            return 0;
        }
        h.min((raw[i - 1].2 - raw[i - 1].1) / 2).min((raw[i].2 - raw[i].1) / 2)
    };
    raw.iter()
        .enumerate()
        .map(|(i, &(t, a, b))| {
            let short = h.min((b - a) / 2);
            let (hb_in, hb_out) = (boundary(i), boundary(i + 1));
            let joined_in = i > 0 && raw[i - 1].2 == a;
            let joined_out = i + 1 < raw.len() && raw[i + 1].1 == b;
            // How far past each boundary this take's audio reaches.
            let ext_in = if joined_in { hb_in.min(a - t.start) } else { 0 };
            let ext_out = if joined_out { hb_out.min(t.end - b) } else { 0 };
            let (start, stop) = (tl(a) - ext_in, tl(b) + ext_out);
            let fade_in = if joined_in {
                (start, hb_in + ext_in)
            } else if a == tt0 - lead {
                if first_pass {
                    own_in
                } else {
                    (start, 0)
                }
            } else {
                (start, short)
            };
            let fade_out = if joined_out {
                (tl(b) - hb_out, hb_out + ext_out)
            } else if b == tt0 + pl + tail {
                if last_pass {
                    own_out
                } else {
                    (stop, 0)
                }
            } else {
                (stop - short, short)
            };
            CompPiece { take: t.id, clip: t.clip, gain_db: t.gain_db, start, end: stop, offset: t.offset + a - ext_in, fade_in, fade_out }
        })
        .collect()
}

/// The comp of a folder's first pass as plain stretches (no crossfade overlap): timeline
/// `(start, length)`, the take, and the clip frame at `start`. What Flatten turns into regions.
pub fn flat_pieces(r: &Region, f: &TakeFolder) -> Vec<(i64, i64, Id, i64)> {
    let RegionContent::Audio { offset: tt0, .. } = r.content else { return Vec::new() };
    raw_pieces(f, tt0, r.length).into_iter().map(|(t, a, b)| (r.start + a - tt0, b - a, t.id, t.offset + a)).collect()
}
