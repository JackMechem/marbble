//! Track groups, and changing a track's settings: `update_track` and `update_track_linked`, which
//! applies an edit to the rest of the group too (volume and pan as relative moves:
//! `linked_move.rs`). Also the analyzer's target and the selected track.

pub(crate) mod linked_move;
#[cfg(test)]
mod tests;

use crate::session::*;
use linked_move::{Level, LinkedMove};

impl Session {
    /// A new group holding `ids` (leaving any group they were in). Returns its id.
    pub fn create_group(&mut self, ids: &[Id]) -> Id {
        let id = self.project.alloc_id();
        let n = (1..).find(|n| !self.project.groups.iter().any(|g| g.name == format!("Group {n}"))).unwrap();
        let colors = ["#e0a030", "#d9534f", "#5cb85c", "#5bc0de", "#b07cd8", "#e0709a"];
        self.project.groups.push(Group {
            id,
            name: format!("Group {n}"),
            color: colors[(n as usize - 1) % colors.len()].into(),
            enabled: true,
            volume: true,
            pan: false,
            mute: true,
            solo: true,
            record: true,
            select: true,
            editing: true,
        });
        self.set_track_group(ids, Some(id));
        id
    }

    pub fn update_group(&mut self, g: Group) {
        if let Some(cur) = self.project.groups.iter_mut().find(|x| x.id == g.id) {
            *cur = g;
            self.dirty = true;
        }
    }

    pub fn delete_group(&mut self, id: Id) {
        for t in self.project.tracks.iter_mut().filter(|t| t.group == Some(id)) {
            t.group = None;
        }
        self.project.groups.retain(|g| g.id != id);
        self.dirty = true;
    }

    pub fn set_track_group(&mut self, ids: &[Id], group: Option<Id>) {
        let group = group.filter(|g| self.project.group(*g).is_some());
        for t in self.project.tracks.iter_mut().filter(|t| ids.contains(&t.id)) {
            t.group = group;
        }
        self.prune_groups();
        self.dirty = true;
    }

    /// Drops groups nobody is in.
    pub(crate) fn prune_groups(&mut self) {
        let used: Vec<Id> = self.project.tracks.iter().filter_map(|t| t.group).collect();
        self.project.groups.retain(|g| used.contains(&g.id));
    }

    /// The other tracks a change to `id` reaches through its group, if `link` is shared.
    pub(crate) fn group_peers(&self, id: Id, link: impl Fn(&Group) -> bool) -> Vec<Id> {
        let Some(g) = self.project.track(id).and_then(|t| t.group).and_then(|g| self.project.group(g)) else { return Vec::new() };
        if !g.enabled || !link(g) {
            return Vec::new();
        }
        self.project.tracks.iter().filter(|t| t.group == Some(g.id) && t.id != id).map(|t| t.id).collect()
    }

    /// `update_track` as the user means it: groups pass shared changes on to their other
    /// members (volume and pan as relative moves, the switches as the same state).
    pub fn update_track_linked(&mut self, id: Id, patch: TrackPatch) {
        let Some(t) = self.project.track(id).cloned() else { return };
        let volumes = patch.volume_db.map(|v| self.linked_levels(id, Level::Volume, t.volume_db, v)).unwrap_or_default();
        let pans = patch.pan.map(|v| self.linked_levels(id, Level::Pan, t.pan, v)).unwrap_or_default();
        let mut peers: Vec<(Id, TrackPatch)> = Vec::new();
        let mut add = |ids: Vec<Id>, f: &dyn Fn(&mut TrackPatch, &Track)| {
            for pid in ids {
                let Some(pt) = self.project.track(pid) else { continue };
                let i = match peers.iter().position(|(x, _)| *x == pid) {
                    Some(i) => i,
                    None => {
                        peers.push((pid, TrackPatch::default()));
                        peers.len() - 1
                    }
                };
                f(&mut peers[i].1, pt);
            }
        };
        let at = |to: &[(Id, f32)], id: Id| to.iter().find(|x| x.0 == id).map(|x| x.1);
        add(volumes.iter().map(|x| x.0).collect(), &|p, pt| p.volume_db = at(&volumes, pt.id));
        add(pans.iter().map(|x| x.0).collect(), &|p, pt| p.pan = at(&pans, pt.id));
        if let Some(v) = patch.mute {
            add(self.group_peers(id, |g| g.mute), &|p, _| p.mute = Some(v));
        }
        if let Some(v) = patch.solo {
            add(self.group_peers(id, |g| g.solo), &|p, _| p.solo = Some(v));
        }
        if let Some(v) = patch.armed {
            add(self.group_peers(id, |g| g.record), &|p, pt| p.armed = Some(v && !pt.kind.is_stack()));
        }
        // The values before the move, for automation writing.
        let before = |p: &Project, tid: Id, patch: &TrackPatch| {
            let t = p.track(tid);
            let was = |on: bool, f: &dyn Fn(&Track) -> f32| if on { t.map(f) } else { None };
            (tid, [was(patch.volume_db.is_some(), &|t| t.volume_db), was(patch.pan.is_some(), &|t| t.pan), was(patch.mute.is_some(), &|t| t.mute as u8 as f32)])
        };
        let mut moved = vec![before(&self.project, id, &patch)];
        moved.extend(peers.iter().map(|(pid, p)| before(&self.project, *pid, p)));
        self.update_track(id, patch);
        for (pid, p) in peers {
            self.update_track(pid, p);
        }
        // While playing in Touch, Latch or Write, the moves are recorded as automation.
        for (tid, was) in moved {
            self.auto_moved(tid, was);
        }
    }

    /// Where the members sharing `level` with `id` go when it moves from `now` to `to`: by the
    /// same amount, each keeping its offset (see [`LinkedMove`]).
    fn linked_levels(&mut self, id: Id, level: Level, now: f32, to: f32) -> Vec<(Id, f32)> {
        let peers: Vec<(Id, f32)> = self
            .group_peers(id, |g| if level == Level::Volume { g.volume } else { g.pan })
            .into_iter()
            .filter_map(|pid| self.project.track(pid).map(|t| (pid, if level == Level::Volume { t.volume_db } else { t.pan })))
            .collect();
        if peers.is_empty() {
            return peers;
        }
        LinkedMove::step(&mut self.linked_move, id, level, now, &peers, to)
    }

    /// Applies a [`TrackPatch`] to this track alone; [`Session::update_track_linked`] takes its group along.
    pub fn update_track(&mut self, id: Id, patch: TrackPatch) {
        let solo_changed = patch.solo.is_some();
        let recording = self.rec_punch.is_some();
        let Some(t) = self.project.track_mut(id) else { return };
        if let Some(v) = patch.name {
            t.name = v;
        }
        if let Some(v) = patch.color {
            t.color = v;
        }
        if let Some(v) = patch.volume_db {
            t.volume_db = v.clamp(-96.0, 6.0);
        }
        if let Some(v) = patch.pan {
            t.pan = v.clamp(-1.0, 1.0);
        }
        if let Some(v) = patch.mute {
            t.mute = v;
        }
        if let Some(v) = patch.solo {
            t.solo = v;
        }
        if let Some(v) = patch.armed {
            t.armed = v && t.kind.can_record() && t.frozen.is_none();
        }
        if let Some(v) = patch.monitor {
            t.monitor = v;
        }
        if let Some(v) = patch.input {
            if t.kind == TrackKind::Audio {
                t.input = v;
            }
        }
        if let Some(v) = patch.collapsed {
            t.collapsed = v;
        }
        // Not while recording: `commands.rs` says so (see `Session::off_refused`).
        let off_changed = !recording && patch.off.is_some_and(|v| v != t.off);
        if off_changed {
            t.off = patch.off.unwrap_or(t.off);
        }
        // Stacks pass changes on to their subtracks, a subtrack's solo reaches its stacks, and
        // solo follows sends and outputs. On/Off changes delay compensation too.
        if t.kind.is_stack() || t.parent.is_some() || solo_changed || off_changed {
            self.push_all_params();
        } else {
            let params = Self::track_params(&self.project, self.project.track(id).unwrap());
            self.send(Cmd::Params(id, params));
        }
        self.update_midi_target();
        self.dirty = true;
    }

    /// Feeds the analyzer from the output of the insert `uid` (a Channel EQ whose editor is
    /// open), on whichever strip it is; None, or an instance that isn't there, turns it off.
    pub fn set_analyzer(&mut self, uid: Option<Id>) {
        let target = uid.and_then(|uid| self.strip_of_insert(uid).map(|track| (track, uid)));
        self.analyzer_target = target;
        self.send(Cmd::Analyzer(target));
    }

    /// The strip (MASTER_ID for Stereo Out) whose Audio FX hold the instance `uid`.
    pub fn strip_of_insert(&self, uid: Id) -> Option<Id> {
        let holds = |slots: &[Option<PluginSlot>]| slots.iter().flatten().any(|s| s.uid == uid);
        if holds(&self.project.master.inserts) {
            return Some(MASTER_ID);
        }
        self.project.tracks.iter().find(|t| holds(&t.inserts)).map(|t| t.id)
    }

    pub fn select_track(&mut self, id: Option<Id>) {
        self.selected = id;
        self.update_midi_target();
    }
}
