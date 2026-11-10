//! The edits of a take folder, on its model types: its takes, its comps and their sections, as
//! Quick Swipe Comping and the take commands change them. Everything here is in take time.

use crate::model::{CcEvent, Comp, CompSegment, Id, Note, Take, TakeFolder};

impl Take {
    // A length in frames on the timeline, not a container's: "empty" means nothing here.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> i64 {
        (self.end - self.start).max(0)
    }
}

impl TakeFolder {
    /// An empty folder.
    pub fn new() -> TakeFolder {
        TakeFolder { takes: Vec::new(), comp: Vec::new(), comps: Vec::new(), current: 0, offset: 0 }
    }

    pub fn take(&self, id: Id) -> Option<&Take> {
        self.takes.iter().find(|t| t.id == id)
    }

    pub fn take_mut(&mut self, id: Id) -> Option<&mut Take> {
        self.takes.iter_mut().find(|t| t.id == id)
    }

    /// Hands take time `start..end` to `take` (clipped to where the take has audio), as a
    /// swipe across a take row does. Whatever the comp had there is replaced.
    pub fn select(&mut self, take: Id, start: i64, end: i64) {
        let Some(t) = self.take(take) else { return };
        let (a, b) = (start.max(t.start), end.min(t.end));
        if a >= b {
            return;
        }
        self.clear(a, b);
        self.comp.push(CompSegment { start: a, end: b, take });
        self.normalize();
    }

    /// Makes `take` the only one heard, over all of its audio.
    pub fn select_whole(&mut self, take: Id) {
        if let Some(t) = self.take(take) {
            self.comp = vec![CompSegment { start: t.start, end: t.end, take }];
        }
    }

    /// Silences take time `start..end` in the comp.
    pub fn clear(&mut self, start: i64, end: i64) {
        let mut out = Vec::with_capacity(self.comp.len() + 1);
        for s in self.comp.drain(..) {
            if s.end <= start || s.start >= end {
                out.push(s);
                continue;
            }
            if s.start < start {
                out.push(CompSegment { end: start, ..s });
            }
            if s.end > end {
                out.push(CompSegment { start: end, ..s });
            }
        }
        self.comp = out;
    }

    /// The comp a fresh recording gets, as in Logic: the newest take wherever it has audio,
    /// older takes filling in where it has none (a last pass stopped part-way through).
    pub fn default_comp(&mut self) {
        self.comp.clear();
        let ids: Vec<Id> = self.takes.iter().map(|t| t.id).collect();
        for id in ids {
            let (s, e) = self.take(id).map(|t| (t.start, t.end)).unwrap_or((0, 0));
            self.select(id, s, e);
        }
    }

    /// The take heard throughout when the comp is one whole take (what the take menu ticks).
    pub fn whole_take(&self) -> Option<Id> {
        match self.comp.as_slice() {
            [s] => self.take(s.take).filter(|t| t.start == s.start && t.end == s.end).map(|t| t.id),
            _ => None,
        }
    }

    /// Takes the comp uses somewhere.
    pub fn used(&self) -> Vec<Id> {
        self.takes.iter().filter(|t| self.comp.iter().any(|s| s.take == t.id)).map(|t| t.id).collect()
    }

    /// Removes a take, and takes its comp pieces with it.
    pub fn remove_take(&mut self, id: Id) {
        self.takes.retain(|t| t.id != id);
        self.normalize();
    }

    /// Delete Unused Takes. Keeps everything when the comp is empty, rather than leaving an
    /// empty folder.
    pub fn remove_unused(&mut self) {
        let used = self.used();
        if !used.is_empty() {
            self.takes.retain(|t| used.contains(&t.id));
        }
    }

    /// Sorts the comp, drops empty or orphaned segments and merges touching ones of one take.
    pub fn normalize(&mut self) {
        let takes: Vec<(Id, i64, i64)> = self.takes.iter().map(|t| (t.id, t.start, t.end)).collect();
        let mut segs: Vec<CompSegment> = self
            .comp
            .iter()
            .filter_map(|s| {
                let &(_, ts, te) = takes.iter().find(|t| t.0 == s.take)?;
                let (a, b) = (s.start.max(ts), s.end.min(te));
                (a < b).then_some(CompSegment { start: a, end: b, take: s.take })
            })
            .collect();
        segs.sort_by_key(|s| s.start);
        let mut out: Vec<CompSegment> = Vec::with_capacity(segs.len());
        for s in segs {
            match out.last_mut() {
                Some(l) if l.take == s.take && l.end == s.start => l.end = s.end,
                // Overlaps can only come from a hand-edited file; the earlier one wins.
                Some(l) if s.start < l.end => {
                    let end = l.end;
                    if s.end > end {
                        out.push(CompSegment { start: end, ..s });
                    }
                }
                _ => out.push(s),
            }
        }
        self.comp = out;
    }

    // ---- grouped folders and Previous/Next Take --------------------------------------------

    /// The take of `other` that goes with this folder's `take`, for grouped folders that switch
    /// and comp together. Folders recorded together (sharing a recording) match pass by pass,
    /// so a take only one of them recorded has no match; others (packed by hand) match by
    /// position.
    pub fn matching_take(&self, take: Id, other: &TakeFolder) -> Option<Id> {
        let t = self.take(take)?;
        let recordings = |f: &TakeFolder| f.takes.iter().filter_map(|t| t.pass.map(|p| p.0)).collect::<Vec<_>>();
        let ours = recordings(self);
        if recordings(other).iter().any(|r| ours.contains(r)) {
            return other.takes.iter().find(|o| t.pass.is_some() && o.pass == t.pass).map(|o| o.id);
        }
        let k = self.takes.iter().position(|x| x.id == take)?;
        other.takes.get(k).map(|o| o.id)
    }

    /// The take Previous and Next Take step from: the one heard throughout, or in a comp the
    /// one heard longest.
    pub fn heard_take(&self) -> Option<Id> {
        let mut best: Option<(Id, i64)> = None;
        for t in &self.takes {
            let heard: i64 = self.comp.iter().filter(|s| s.take == t.id).map(|s| s.end - s.start).sum();
            if heard > 0 && best.is_none_or(|(_, b)| heard > b) {
                best = Some((t.id, heard));
            }
        }
        best.map(|(id, _)| id)
    }

    /// The take `delta` places after (before, when negative) the one heard, for Previous and
    /// Next Take. None past the first or last take. With nothing heard, Next starts at the
    /// first take and Previous at the last.
    pub fn stepped_take(&self, delta: i64) -> Option<Id> {
        let n = self.takes.len() as i64;
        let at = match self.heard_take().and_then(|id| self.takes.iter().position(|t| t.id == id)) {
            Some(k) => k as i64 + delta,
            None if delta > 0 => delta - 1,
            None => n + delta,
        };
        (0..n).contains(&at).then(|| self.takes[at as usize].id)
    }

    // ---- comp sections (Logic's comp section editing) ----------------------------------------

    /// The comp section heard at take time `at`.
    pub fn section_at(&self, at: i64) -> Option<CompSegment> {
        self.comp.iter().find(|s| s.start <= at && at < s.end).copied()
    }

    /// Moves the comp edge at take time `at` to `to`. Where one section hands over to the next
    /// both move; an outer edge (next to a gap or the end) moves its own section alone. No
    /// section passes its neighbour, vanishes or reaches past its take's audio. Nothing changes
    /// when no section edge is at `at`.
    pub fn move_edge(&mut self, at: i64, to: i64) {
        let left = self.comp.iter().position(|s| s.end == at);
        let right = self.comp.iter().position(|s| s.start == at);
        let audio = |s: &CompSegment| self.take(s.take).map(|t| (t.start, t.end)).unwrap_or((s.start, s.end));
        let (mut lo, mut hi) = (i64::MIN, i64::MAX);
        if let Some(l) = left {
            lo = self.comp[l].start + 1;
            hi = hi.min(audio(&self.comp[l]).1);
            if right.is_none() {
                hi = hi.min(self.comp.get(l + 1).map_or(i64::MAX, |n| n.start));
            }
        }
        if let Some(r) = right {
            lo = lo.max(audio(&self.comp[r]).0);
            if left.is_none() && r > 0 {
                lo = lo.max(self.comp[r - 1].end);
            }
            hi = hi.min(self.comp[r].end - 1);
        }
        if (left.is_none() && right.is_none()) || lo > hi {
            return;
        }
        let to = to.clamp(lo, hi);
        if let Some(l) = left {
            self.comp[l].end = to;
        }
        if let Some(r) = right {
            self.comp[r].start = to;
        }
        self.normalize();
    }

    /// The take `delta` rows from section `s`'s take (up when negative) that has audio
    /// somewhere in it, where moving the section to the take above or below sends it.
    pub fn neighbour_take(&self, s: &CompSegment, delta: i64) -> Option<Id> {
        let k = self.takes.iter().position(|t| t.id == s.take)? as i64;
        let n = self.takes.len() as i64;
        let mut i = k + delta;
        while delta != 0 && (0..n).contains(&i) {
            let t = &self.takes[i as usize];
            if t.start < s.end && t.end > s.start {
                return Some(t.id);
            }
            i += delta.signum();
        }
        None
    }

    // ---- comps ------------------------------------------------------------------------------

    /// The comps by name, the current one at `current`. A folder that never had a second comp
    /// has one, "Comp 1".
    pub fn comp_names(&self) -> Vec<String> {
        if self.comps.is_empty() {
            return vec!["Comp 1".into()];
        }
        self.comps.iter().map(|c| c.name.clone()).collect()
    }

    /// The name of the comp being heard.
    pub fn comp_name(&self) -> String {
        let mut names = self.comp_names();
        let i = self.current.min(names.len() - 1);
        names.swap_remove(i)
    }

    /// Copies the comp being edited into its place in the list (after every edit).
    pub fn store_comp(&mut self) {
        let comp = self.comp.clone();
        if let Some(c) = self.comps.get_mut(self.current) {
            c.comp = comp;
        }
    }

    /// Turns the implicit single comp into a list entry, so others can be added.
    fn list_comps(&mut self) {
        if self.comps.is_empty() {
            self.comps.push(Comp { name: "Comp 1".into(), comp: self.comp.clone() });
            self.current = 0;
        }
    }

    /// "Comp N", for the lowest N above every comp named that way.
    fn next_comp_name(&self) -> String {
        let n = self.comps.iter().filter_map(|c| c.name.strip_prefix("Comp ")?.trim().parse::<usize>().ok()).max().unwrap_or(0);
        format!("Comp {}", n.max(self.comps.len()) + 1)
    }

    fn add_comp(&mut self, comp: Vec<CompSegment>) {
        self.list_comps();
        self.store_comp();
        let name = self.next_comp_name();
        self.comps.push(Comp { name, comp });
        self.current = self.comps.len() - 1;
        self.comp = self.comps[self.current].comp.clone();
        self.normalize();
        self.store_comp();
    }

    /// Create New Comp: a new comp, as a fresh recording starts out (the newest take wherever
    /// it has audio), becomes the one heard.
    pub fn create_comp(&mut self) {
        let keep = std::mem::take(&mut self.comp);
        self.default_comp();
        let fresh = std::mem::replace(&mut self.comp, keep);
        self.add_comp(fresh);
    }

    /// Duplicate Comp: a copy of the comp being heard, to change without losing it.
    pub fn duplicate_comp(&mut self) {
        let copy = self.comp.clone();
        self.add_comp(copy);
    }

    /// Makes comp `index` the one heard (and edited).
    pub fn switch_comp(&mut self, index: usize) {
        if index >= self.comps.len() || index == self.current {
            return;
        }
        self.store_comp();
        self.current = index;
        self.comp = self.comps[index].comp.clone();
        // Takes deleted since the comp was saved drop out of it.
        self.normalize();
        self.store_comp();
    }

    pub fn rename_comp(&mut self, index: usize, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() || index >= self.comp_names().len() {
            return;
        }
        self.list_comps();
        self.comps[index].name = name;
    }

    /// Delete Comp. The last comp can't go; deleting the one heard switches to its neighbour.
    pub fn delete_comp(&mut self, index: usize) {
        if self.comps.len() < 2 || index >= self.comps.len() {
            return;
        }
        self.store_comp();
        self.comps.remove(index);
        if index < self.current || self.current >= self.comps.len() {
            self.current = self.current.saturating_sub(1);
        }
        self.comp = self.comps[self.current].comp.clone();
        self.normalize();
        self.store_comp();
    }

    // ---- MIDI take folders ------------------------------------------------------------------

    /// The take a MIDI folder plays: the comp's (whole) take, or the newest.
    pub fn midi_take(&self) -> Option<Id> {
        self.comp.first().map(|s| s.take).filter(|&t| self.take(t).is_some()).or(self.takes.last().map(|t| t.id))
    }

    /// The notes and controller events of `take` heard over take time `offset..offset + len`,
    /// moved so `offset` becomes 0 (a MIDI region's content).
    pub fn midi_window(&self, take: Id, offset: i64, len: i64) -> (Vec<Note>, Vec<CcEvent>) {
        let Some(t) = self.take(take) else { return (Vec::new(), Vec::new()) };
        let inside = |p: i64| p >= offset && p < offset + len;
        let notes = t.notes.iter().filter(|n| inside(n.start)).map(|n| Note { start: n.start - offset, ..*n }).collect();
        let cc = t.cc.iter().filter(|c| inside(c.start)).map(|c| CcEvent { start: c.start - offset, ..*c }).collect();
        (notes, cc)
    }

    /// Writes a MIDI region's content back into `take` over take time `offset..offset + len`
    /// (what [`TakeFolder::midi_window`] gave, perhaps edited since). The take keeps what it
    /// has outside that window.
    pub fn store_midi(&mut self, take: Id, offset: i64, len: i64, notes: &[Note], cc: &[CcEvent]) {
        let Some(t) = self.take_mut(take) else { return };
        let inside = |p: i64| p >= offset && p < offset + len;
        t.notes.retain(|n| !inside(n.start));
        t.notes.extend(notes.iter().map(|n| Note { start: n.start + offset, ..*n }));
        t.notes.sort_by_key(|n| n.start);
        t.cc.retain(|c| !inside(c.start));
        t.cc.extend(cc.iter().map(|c| CcEvent { start: c.start + offset, ..*c }));
        t.cc.sort_by_key(|c| c.start);
    }

    /// Moves every take and comp by `delta` frames of take time, and a MIDI folder's offset
    /// with them, so it plays the same.
    pub fn shift(&mut self, delta: i64) {
        for t in &mut self.takes {
            t.start += delta;
            t.end += delta;
            t.notes.iter_mut().for_each(|n| n.start += delta);
            t.cc.iter_mut().for_each(|c| c.start += delta);
        }
        for s in self.comp.iter_mut().chain(self.comps.iter_mut().flat_map(|c| c.comp.iter_mut())) {
            s.start += delta;
            s.end += delta;
        }
        self.offset += delta;
    }
}

impl Default for TakeFolder {
    fn default() -> Self {
        TakeFolder::new()
    }
}
