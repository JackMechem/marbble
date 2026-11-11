//! Adding, deleting, duplicating, reordering and hiding tracks, and track stacks. Sends and outputs
//! are in `routing.rs`, Track Alternatives in `alternatives.rs`, a track's settings in
//! `groups.rs`.

use crate::session::*;

impl Session {
    /// Adds a track of a kind, named and coloured by its kind, at `index` (or the end). Returns its id.
    pub fn add_track(&mut self, kind: TrackKind, input: InputSel, instrument: Option<String>, index: Option<usize>) -> Result<Id> {
        let id = self.project.alloc_id();
        let count = self.project.tracks.iter().filter(|t| t.kind == kind).count() + 1;
        let bus = if kind == TrackKind::Aux { self.free_bus() } else { 0 };
        let (name, color) = match kind {
            TrackKind::Audio => (format!("Audio {count}"), AUDIO_COLOR),
            TrackKind::Instrument => (format!("Inst {count}"), INSTRUMENT_COLOR),
            TrackKind::Folder => (format!("Folder {count}"), STACK_COLOR),
            TrackKind::Summing => (format!("Sum {count}"), STACK_COLOR),
            TrackKind::Aux => (format!("Aux {bus}"), AUX_COLOR),
        };
        let mut track = Track {
            id,
            kind,
            name,
            color: color.into(),
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            monitor: false,
            input: if kind == TrackKind::Audio { input } else { InputSel::None },
            instrument: None,
            inserts: vec![None; INSERT_SLOTS],
            regions: Vec::new(),
            legacy_eq: None,
            parent: None,
            collapsed: false,
            group: None,
            hidden: false,
            sends: vec![None; SEND_SLOTS],
            output: None,
            bus,
            automation: Vec::new(),
            auto_mode: AutoMode::Read,
            frozen: None,
            vca: None,
            alternatives: Vec::new(),
            alternative: String::new(),
            auto_lanes: Vec::new(),
            instrument_output: None,
            legacy_eq_preset: None,
            off: false,
            icon: None,
            frozen_output: None,
        };
        if kind == TrackKind::Instrument {
            let pid = instrument.unwrap_or_else(|| BUILTIN_SYNTH.to_string());
            let built_in = crate::instrument::find(&pid);
            let name = match &built_in {
                Some(i) => i.name.to_string(),
                None => self.find_plugin(&pid).map(|d| d.name.clone()).unwrap_or_else(|| "Instrument".into()),
            };
            track.name = if pid == BUILTIN_SYNTH { format!("Inst {count}") } else { name.clone() };
            let params = built_in.map(|i| i.params.iter().map(|p| (p.id, p.default)).collect()).unwrap_or_default();
            track.instrument = Some(PluginSlot {
                uid: self.project.alloc_id(),
                plugin_id: pid,
                name,
                bypass: false,
                state: None,
                params,
                sidechain: None,
                sidechain_pre: false,
                preset: None,
            });
        }
        let dsp = self.build_track(&track);
        let idx = index.unwrap_or(self.project.tracks.len()).min(self.project.tracks.len());
        // A track created among a stack's subtracks joins that stack.
        let parent = self.project.tracks.get(idx).and_then(|t| t.parent);
        self.project.tracks.insert(idx, track);
        self.project.tracks[idx].parent = parent;
        self.send(Cmd::AddTrack(dsp, idx));
        self.sync_tracks();
        self.dirty = true;
        Ok(id)
    }

    pub fn delete_track(&mut self, id: Id) {
        self.delete_tracks(&[id]);
    }

    /// Deletes tracks; a stack goes with its subtracks.
    pub fn delete_tracks(&mut self, ids: &[Id]) {
        let mut all: Vec<Id> = Vec::new();
        for &id in ids {
            if self.project.track(id).is_some() && !all.contains(&id) {
                all.push(id);
                all.extend(self.project.descendants(id).into_iter().filter(|d| !ids.contains(d)));
            }
        }
        if all.is_empty() {
            return;
        }
        // A plug-in a processing thread is left in keeps its editor, hidden, untold.
        self.mark_unresponsive();
        let first = self.project.tracks.iter().position(|t| all.contains(&t.id)).unwrap_or(0);
        for id in &all {
            let Some(i) = self.project.tracks.iter().position(|t| t.id == *id) else { continue };
            let t = self.project.tracks.remove(i);
            for slot in t.inserts.iter().flatten().chain(t.instrument.iter()) {
                self.controllers.remove(&slot.uid);
            }
            self.meters.remove(id);
            self.send(Cmd::RemoveTrack(*id));
        }
        if self.selected.map(|s| all.contains(&s)).unwrap_or(false) {
            self.selected = self.project.tracks.get(first.min(self.project.tracks.len().saturating_sub(1))).map(|t| t.id);
        }
        self.prune_groups();
        self.sync_tracks();
        self.dirty = true;
    }

    /// Tracks without regions (stacks that still hold tracks are kept), as Logic's Delete
    /// Unused Tracks. Returns how many went.
    pub fn delete_unused_tracks(&mut self) -> usize {
        let mut unused: Vec<Id> = self
            .project
            .tracks
            .iter()
            .filter(|t| {
                t.kind != TrackKind::Aux
                    && t.regions.is_empty()
                    && t.alternatives.iter().all(|a| a.regions.is_empty())
                    && !(t.kind.is_stack() && self.project.tracks.iter().any(|c| c.parent == Some(t.id)))
            })
            .map(|t| t.id)
            .collect();
        // An aux is used while a track that stays sends or routes to it. Auxes can feed each
        // other, so repeat until nothing changes.
        loop {
            let p = &self.project;
            let idle: Vec<Id> = p
                .tracks
                .iter()
                .filter(|a| a.kind == TrackKind::Aux && !unused.contains(&a.id))
                .filter(|a| !p.tracks.iter().any(|t| t.id != a.id && !unused.contains(&t.id) && p.feeds(t.id).contains(&a.id)))
                .map(|a| a.id)
                .collect();
            if idle.is_empty() {
                break;
            }
            unused.extend(idle);
        }
        self.delete_tracks(&unused);
        unused.len()
    }

    /// Reorders tracks, optionally moving some into or out of stacks (`parents`).
    pub fn reorder_tracks(&mut self, ids: Vec<Id>) {
        self.move_tracks(ids, &[]);
    }

    /// Reorders tracks to the order of `ids`, and sets new stack parents where that makes no loop. Auxes
    /// stay out of stacks.
    pub fn move_tracks(&mut self, ids: Vec<Id>, parents: &[(Id, Option<Id>)]) {
        self.project.tracks.sort_by_key(|t| ids.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
        for &(id, parent) in parents {
            // A track can't go into itself or one of its own subtracks.
            let ok = parent
                .map(|p| p != id && !self.project.descendants(id).contains(&p) && self.project.track(p).map(|t| t.kind.is_stack()).unwrap_or(false))
                .unwrap_or(true);
            // Auxes stay out of stacks, which keeps feedback loops out of the stack tree.
            let ok = ok && (parent.is_none() || !self.project.is_aux(id));
            if let (true, Some(t)) = (ok, self.project.track_mut(id)) {
                t.parent = parent;
            }
        }
        self.sync_tracks();
        self.dirty = true;
    }

    /// A name for a copy of `base`: a trailing number counts up ("Audio 1" -> "Audio 2"),
    /// otherwise one is added.
    fn copy_name(&self, base: &str) -> String {
        let trimmed = base.trim_end_matches(|c: char| c.is_ascii_digit());
        let (stem, n) = match base[trimmed.len()..].parse::<u32>() {
            Ok(n) if trimmed.ends_with(' ') || trimmed.is_empty() => (trimmed.to_string(), n + 1),
            _ => (format!("{base} "), 2),
        };
        (n..).map(|k| format!("{stem}{k}")).find(|name| !self.project.tracks.iter().any(|t| &t.name == name)).unwrap()
    }

    /// The slot with a fresh instance id and the running plug-in's current settings.
    fn copy_slot(&mut self, slot: &PluginSlot) -> PluginSlot {
        // A plug-in a processing thread is left in (`session/engine/left_out.rs`) gives its last saved state.
        let state = self.controllers.get(&slot.uid).and_then(|c| c.save_state()).map(|s| encode_state(&s)).or_else(|| slot.state.clone());
        PluginSlot { uid: self.project.alloc_id(), state, ..slot.clone() }
    }

    /// Duplicates tracks below the originals: settings, plug-ins (with their current settings)
    /// and, with `content`, the regions. A stack is copied with its subtracks. Returns the
    /// copies of the given tracks.
    pub fn duplicate_tracks(&mut self, ids: &[Id], content: bool) -> Vec<Id> {
        let mut roots: Vec<Id> = self.project.tracks.iter().map(|t| t.id).filter(|id| ids.contains(id)).collect();
        // Copying a stack already copies its subtracks.
        let anc: Vec<Vec<Id>> = roots.iter().map(|&id| self.project.ancestors(id)).collect();
        let keep: Vec<bool> = anc.iter().map(|a| !a.iter().any(|x| roots.contains(x))).collect();
        let mut k = keep.iter();
        roots.retain(|_| *k.next().unwrap());
        let mut made = Vec::new();
        for root in roots {
            let mut family = vec![root];
            family.extend(self.project.descendants(root));
            let mut map: HashMap<Id, Id> = HashMap::new();
            let mut copies = Vec::new();
            for id in &family {
                let src = self.project.track(*id).unwrap().clone();
                let new_id = self.project.alloc_id();
                map.insert(*id, new_id);
                let mut t = src.clone();
                t.id = new_id;
                t.name = self.copy_name(&src.name);
                t.solo = false;
                t.armed = false;
                // A copy of a frozen track is live: it gets its own plug-ins from their saved
                // settings rather than sharing the freeze file.
                t.frozen = None;
                if t.kind == TrackKind::Aux {
                    t.bus = self.free_bus();
                }
                t.instrument = src.instrument.as_ref().map(|s| self.copy_slot(s));
                t.inserts = src.inserts.iter().map(|s| s.as_ref().map(|s| self.copy_slot(s))).collect();
                // Automation goes with the content, onto the copied plug-ins.
                if content {
                    let old = src.inserts.iter().flatten().chain(src.instrument.iter()).map(|s| s.uid);
                    let new = t.inserts.iter().flatten().chain(t.instrument.iter()).map(|s| s.uid);
                    crate::edit::automation::remap_plugins(&mut t.automation, &old.zip(new).collect::<Vec<_>>());
                } else {
                    t.automation.clear();
                }
                t.regions = if content { src.regions.iter().map(|r| Region { id: self.project.alloc_id(), ..r.clone() }).collect() } else { Vec::new() };
                // Track alternatives are content too, and their regions need ids of their own:
                // a copy sharing them would edit the original's regions once both play them.
                if content {
                    for r in t.alternatives.iter_mut().flat_map(|a| a.regions.iter_mut()) {
                        r.id = self.project.alloc_id();
                    }
                } else {
                    t.alternatives.clear();
                    t.alternative.clear();
                }
                // Extra lanes of the original's plug-ins show the copies' parameters.
                let old = src.inserts.iter().flatten().chain(src.instrument.iter()).map(|s| s.uid);
                let new = t.inserts.iter().flatten().chain(t.instrument.iter()).map(|s| s.uid);
                let map: Vec<(Id, Id)> = old.zip(new).collect();
                for p in &mut t.auto_lanes {
                    if let AutoParam::Plugin { uid, .. } = p {
                        if let Some((_, n)) = map.iter().find(|(o, _)| o == uid) {
                            *uid = *n;
                        }
                    }
                }
                copies.push(t);
            }
            for t in &mut copies {
                if t.id != map[&root] {
                    t.parent = t.parent.map(|p| *map.get(&p).unwrap_or(&p));
                }
                // So the new track's name doesn't count as taken twice.
            }
            let end = self.project.tracks.iter().rposition(|t| family.contains(&t.id)).unwrap_or(0) + 1;
            for (k, t) in copies.into_iter().enumerate() {
                let dsp = self.build_track(&t);
                self.project.tracks.insert(end + k, t);
                self.send(Cmd::AddTrack(dsp, end + k));
            }
            made.push(map[&root]);
        }
        self.sync_tracks();
        self.dirty = true;
        made
    }

    /// Puts tracks into a new track stack, created above the first of them. They must share
    /// a parent; the ones that don't are left out.
    pub fn create_stack(&mut self, ids: &[Id], summing: bool) -> Option<Id> {
        let order: Vec<Id> = self.project.tracks.iter().map(|t| t.id).filter(|id| ids.contains(id)).collect();
        let first = *order.first()?;
        let parent = self.project.track(first)?.parent;
        let members: Vec<Id> =
            order.into_iter().filter(|id| self.project.track(*id).map(|t| t.parent == parent && t.kind != TrackKind::Aux).unwrap_or(false)).collect();
        let first = *members.first()?;
        let idx = self.project.tracks.iter().position(|t| t.id == first)?;
        let kind = if summing { TrackKind::Summing } else { TrackKind::Folder };
        let id = self.add_track(kind, InputSel::None, None, Some(idx)).ok()?;
        let color = self.project.track(first).map(|t| t.color.clone()).unwrap_or_else(|| STACK_COLOR.into());
        if let Some(t) = self.project.track_mut(id) {
            t.parent = parent;
            t.color = color;
        }
        for m in &members {
            self.project.track_mut(*m).unwrap().parent = Some(id);
        }
        self.sync_tracks();
        self.dirty = true;
        Some(id)
    }

    /// Removes a stack's main track; its subtracks stay, one level up.
    pub fn flatten_stack(&mut self, id: Id) {
        let Some(t) = self.project.track(id) else { return };
        if !t.kind.is_stack() {
            return;
        }
        let up = t.parent;
        for c in self.project.tracks.iter_mut().filter(|c| c.parent == Some(id)) {
            c.parent = up;
        }
        self.delete_tracks(&[id]);
    }

    /// Hides tracks (or shows them again). A stack takes its subtracks with it. Hidden tracks
    /// keep playing, as in Logic; only the views leave them out.
    pub fn set_tracks_hidden(&mut self, ids: &[Id], hidden: bool) {
        let mut all = ids.to_vec();
        for &id in ids {
            all.extend(self.project.descendants(id));
        }
        for t in self.project.tracks.iter_mut().filter(|t| all.contains(&t.id)) {
            t.hidden = hidden;
        }
        self.dirty = true;
    }
}
