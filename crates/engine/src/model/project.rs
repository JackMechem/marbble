//! [`Project`], the whole saved document, with the project-wide parts: markers, punch, master
//! strip, cycle and clip references, and the queries and edits that span tracks (ids, lookups,
//! the routing order the engine plays in, loop refusal).

use super::*;

/// A marker on the Marker track (Logic's global marker lane).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub id: Id,
    /// Timeline position in samples.
    pub pos: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// A marker can span a section; None is a point marker that reaches the next one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<i64>,
}

/// A section on the Arrangement track (Logic's arrangement marker: Intro, Verse, Chorus…).
/// Sections tile the timeline: each starts where the one before it ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrangementMarker {
    pub id: Id,
    pub name: String,
    /// Timeline position in samples, like a marker's.
    pub start: i64,
    pub length: i64,
    /// None colours the section by its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl ArrangementMarker {
    pub fn end(&self) -> i64 {
        self.start + self.length
    }
}

/// The Autopunch range: while it is on, recording only captures between `start` and `end`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Punch {
    pub enabled: bool,
    pub start: i64,
    pub end: i64,
}

impl Punch {
    /// The punch range, when Autopunch is on and the range isn't empty.
    pub fn range(&self) -> Option<(i64, i64)> {
        (self.enabled && self.end > self.start).then_some((self.start, self.end))
    }
}

/// The Stereo Out strip's document, the strip with [`MASTER_ID`](super::MASTER_ID).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Master {
    pub volume_db: f32,
    #[serde(default)]
    pub inserts: Vec<Option<PluginSlot>>,
    #[serde(default)]
    pub eq: ChannelEq,
    /// Stereo Out's automation: volume and its plug-ins' parameters, as on a track.
    #[serde(default)]
    pub automation: Vec<AutoLane>,
    #[serde(default)]
    pub auto_mode: AutoMode,
    /// Extra lanes shown below the Output track's main lane.
    #[serde(default)]
    pub auto_lanes: Vec<AutoParam>,
}

/// The cycle range between the locators, in frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cycle {
    pub enabled: bool,
    pub start: i64,
    pub end: i64,
}

/// An audio file the project uses, relative to the project folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipRef {
    pub id: Id,
    /// Path relative to the project folder.
    pub path: String,
}

/// The whole document: what `project.json` holds and what the undo history snapshots. New fields go at
/// the end with `#[serde(default)]` so older projects still open, and `src/api.ts` mirrors them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub sample_rate: u32,
    pub tempo: f64,
    pub sig_num: u32,
    pub sig_den: u32,
    pub key: String,
    pub tracks: Vec<Track>,
    pub master: Master,
    pub cycle: Cycle,
    pub metronome: bool,
    pub count_in: bool,
    #[serde(default)]
    pub clips: Vec<ClipRef>,
    #[serde(default)]
    pub groups: Vec<Group>,
    pub next_id: Id,
    /// Metronome settings (Project Settings › Metronome).
    #[serde(default)]
    pub click: ClickSettings,
    /// Overrides the app-wide recording format for this project.
    #[serde(default)]
    pub bit_depth: Option<BitDepth>,
    /// Free-form project notes.
    #[serde(default)]
    pub notes: String,
    /// The Marker track, sorted by position.
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// The Autopunch range.
    #[serde(default)]
    pub punch: Punch,
    /// The Tempo track (see `tempo.rs`). Empty: `tempo` throughout. Otherwise the first
    /// event is at 0 and `tempo` mirrors it.
    #[serde(default)]
    pub tempo_map: Vec<TempoEvent>,
    /// The Signature track. Empty: `sig_num/sig_den` throughout. Otherwise the first change
    /// is at bar 1 and `sig_num/sig_den` mirror it.
    #[serde(default)]
    pub signatures: Vec<SigEvent>,
    /// The Tracks area's drag mode (Logic's Drag pop-up): what edits do to the regions they meet.
    #[serde(default)]
    pub drag_mode: DragMode,
    /// The VCA faders, in the Mixer's order.
    #[serde(default)]
    pub vcas: Vec<Vca>,
    /// The name of this project alternative (File › Project Alternatives). Empty in projects
    /// from before alternatives, where it is the project's name. See `session/alternatives.rs`.
    #[serde(default)]
    pub alternative: String,
    /// The Arrangement track: named sections tiling the timeline, sorted by start (see
    /// `crate::arrangement`).
    #[serde(default)]
    pub arrangement: Vec<ArrangementMarker>,
    /// Edit › Snap Edits to Zero Crossings: splits and trims of audio regions move to the
    /// nearest zero crossing. See `session/zero_crossings.rs`.
    #[serde(default)]
    pub snap_zero_crossings: bool,
}

impl Project {
    pub fn new(name: &str, sample_rate: u32) -> Project {
        Project {
            name: name.to_string(),
            sample_rate,
            tempo: 120.0,
            sig_num: 4,
            sig_den: 4,
            key: "C major".into(),
            tracks: Vec::new(),
            master: Master {
                volume_db: 0.0,
                inserts: vec![None; INSERT_SLOTS],
                eq: ChannelEq::default(),
                automation: Vec::new(),
                auto_mode: AutoMode::Read,
                auto_lanes: Vec::new(),
            },
            cycle: Cycle { enabled: false, start: 0, end: 0 },
            metronome: false,
            count_in: false,
            clips: Vec::new(),
            groups: Vec::new(),
            next_id: 1,
            click: ClickSettings::default(),
            bit_depth: None,
            notes: String::new(),
            markers: Vec::new(),
            punch: Punch::default(),
            tempo_map: Vec::new(),
            signatures: Vec::new(),
            drag_mode: DragMode::Overlap,
            vcas: Vec::new(),
            alternative: String::new(),
            arrangement: Vec::new(),
            snap_zero_crossings: false,
        }
    }

    /// A new id. Ids are never reused within a project.
    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn track(&self, id: Id) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: Id) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn group(&self, id: Id) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Ids of `id`'s subtracks, nested ones included, in track order.
    pub fn descendants(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut frontier = vec![id];
        while let Some(p) = frontier.pop() {
            for t in self.tracks.iter().filter(|t| t.parent == Some(p)) {
                out.push(t.id);
                frontier.push(t.id);
            }
        }
        out.sort_by_key(|id| self.tracks.iter().position(|t| t.id == *id));
        out
    }

    /// The stacks `id` sits in, innermost first.
    /// Whether a track is off, itself or through a stack it is in.
    pub fn is_off(&self, id: Id) -> bool {
        self.track(id).is_some_and(|t| t.off) || self.ancestors(id).iter().any(|a| self.track(*a).is_some_and(|t| t.off))
    }

    /// The stacks `id` sits in, innermost first.
    pub fn ancestors(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut cur = self.track(id).and_then(|t| t.parent);
        while let Some(p) = cur {
            if out.contains(&p) {
                break;
            }
            out.push(p);
            cur = self.track(p).and_then(|t| t.parent);
        }
        out
    }

    /// Puts every stack's subtracks directly below it (keeping their relative order) and drops
    /// parent links to tracks that aren't stacks.
    pub fn normalize_tree(&mut self) {
        let stacks: Vec<Id> = self.tracks.iter().filter(|t| t.kind.is_stack()).map(|t| t.id).collect();
        let ids: Vec<Id> = self.tracks.iter().map(|t| t.id).collect();
        for t in &mut self.tracks {
            if t.parent.map(|p| !stacks.contains(&p) || p == t.id).unwrap_or(false) {
                t.parent = None;
            }
        }
        // Break cycles.
        for id in &ids {
            let anc = self.ancestors(*id);
            if anc.contains(id) {
                self.track_mut(*id).unwrap().parent = None;
            }
        }
        let mut order = Vec::with_capacity(ids.len());
        fn visit(p: &Project, id: Id, order: &mut Vec<Id>) {
            order.push(id);
            for t in p.tracks.iter().filter(|t| t.parent == Some(id)) {
                visit(p, t.id, order);
            }
        }
        for t in self.tracks.iter().filter(|t| t.parent.is_none()) {
            visit(self, t.id, &mut order);
        }
        self.tracks.sort_by_key(|t| order.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
    }

    /// Track ids in processing order: every track before whatever it plays into (its summing
    /// stack, its output aux and the auxes it sends to). Ties keep the track-tree order, with
    /// subtracks before their stack. Cycles are refused when routing is edited; if one is
    /// loaded anyway, the edge that closes it is skipped here and ignored by the engine.
    pub fn process_order(&self) -> Vec<Id> {
        let mut base = Vec::with_capacity(self.tracks.len());
        fn tree(p: &Project, id: Id, out: &mut Vec<Id>) {
            for t in p.tracks.iter().filter(|t| t.parent == Some(id)) {
                tree(p, t.id, out);
            }
            out.push(id);
        }
        for t in self.tracks.iter().filter(|t| t.parent.is_none()) {
            tree(self, t.id, &mut base);
        }
        // Who plays into each track, in tree order.
        let mut inputs: HashMap<Id, Vec<Id>> = HashMap::new();
        for &id in &base {
            let parent = self.track(id).and_then(|t| t.parent);
            for dst in parent.into_iter().chain(self.feeds(id)).chain(self.keyed_by(id)) {
                inputs.entry(dst).or_default().push(id);
            }
        }
        let mut out = Vec::with_capacity(base.len());
        let mut seen: HashSet<Id> = HashSet::new();
        fn visit(id: Id, inputs: &HashMap<Id, Vec<Id>>, seen: &mut HashSet<Id>, out: &mut Vec<Id>) {
            if !seen.insert(id) {
                return;
            }
            for &src in inputs.get(&id).into_iter().flatten() {
                visit(src, inputs, seen, out);
            }
            out.push(id);
        }
        for &id in &base {
            visit(id, &inputs, &mut seen, &mut out);
        }
        out
    }

    pub fn is_aux(&self, id: Id) -> bool {
        self.track(id).map(|t| t.kind == TrackKind::Aux).unwrap_or(false)
    }

    /// Where a track's own signal goes (None = Stereo Out): its explicit output, else the
    /// nearest summing stack it sits in. A folder passes its subtracks on to wherever it plays.
    pub fn output_of(&self, id: Id) -> Option<Id> {
        match self.track(id)?.output {
            Some(MASTER_ID) => None,
            Some(o) if o != id && self.is_aux(o) => Some(o),
            _ => self.ancestors(id).into_iter().find(|a| self.track(*a).map(|t| t.kind == TrackKind::Summing).unwrap_or(false)),
        }
    }

    /// Every track `id` plays into directly: its output, the auxes its sends feed and, for an
    /// instrument, the auxes on its extra outputs.
    pub fn feeds(&self, id: Id) -> Vec<Id> {
        let mut out = self.feeds_past_fader(id);
        if self.track(id).is_some_and(|t| t.kind == TrackKind::Instrument) {
            for a in self.tracks.iter().filter(|a| a.instrument_output.is_some_and(|o| o.track == id)) {
                if !out.contains(&a.id) {
                    out.push(a.id);
                }
            }
        }
        out
    }

    /// Like [`Project::feeds`], for what leaves through the track's fader or sends: an
    /// instrument's extra outputs don't.
    fn feeds_past_fader(&self, id: Id) -> Vec<Id> {
        let mut out: Vec<Id> = self.output_of(id).into_iter().collect();
        if let Some(t) = self.track(id) {
            for s in t.sends.iter().flatten() {
                if s.bus != id && self.is_aux(s.bus) && !out.contains(&s.bus) {
                    out.push(s.bus);
                }
            }
        }
        out
    }

    /// The tracks with an insert keyed (side-chained) by `id`.
    pub fn keyed_by(&self, id: Id) -> Vec<Id> {
        self.tracks.iter().filter(|t| t.id != id && t.inserts.iter().flatten().any(|s| s.sidechain == Some(id))).map(|t| t.id).collect()
    }

    /// Whether `to` must play after `from`: `from`'s audio reaches it, or keys a side-chain on
    /// it or on anything it reaches. Edits that would make this true both ways are refused.
    pub fn depends(&self, from: Id, to: Id) -> bool {
        let mut stack = vec![from];
        let mut seen: HashSet<Id> = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let parent = self.track(id).and_then(|t| t.parent);
            for d in parent.into_iter().chain(self.feeds(id)).chain(self.keyed_by(id)) {
                if d == to {
                    return true;
                }
                stack.push(d);
            }
        }
        false
    }

    /// Whether `from`'s audio reaches `to`, through outputs, stacks and sends.
    pub fn reaches(&self, from: Id, to: Id) -> bool {
        self.reaches_by(from, to, |p, id| p.feeds(id))
    }

    /// Whether `to` hears `from` only if `from` is audible: `reaches`, without an instrument's
    /// extra outputs, which leave ahead of its fader, mute and solo.
    pub fn reaches_past_fader(&self, from: Id, to: Id) -> bool {
        self.reaches_by(from, to, |p, id| if id == from { p.feeds_past_fader(id) } else { p.feeds(id) })
    }

    fn reaches_by(&self, from: Id, to: Id, feeds: impl Fn(&Project, Id) -> Vec<Id>) -> bool {
        let mut stack = vec![from];
        let mut seen: HashSet<Id> = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            for d in feeds(self, id) {
                if d == to {
                    return true;
                }
                stack.push(d);
            }
        }
        false
    }

    /// Drops routing that no longer makes sense: sends and outputs to tracks that are gone or
    /// aren't auxes, routing on folders (which carry no audio), auxes inside stacks, and any
    /// edge that closes a feedback loop. Pads every track to `SEND_SLOTS` send slots. An aux
    /// whose instrument is gone keeps its settings and its bus, without that input.
    pub fn prune_routes(&mut self) {
        let auxes: HashSet<Id> = self.tracks.iter().filter(|t| t.kind == TrackKind::Aux).map(|t| t.id).collect();
        let instruments: HashSet<Id> = self.tracks.iter().filter(|t| t.kind == TrackKind::Instrument).map(|t| t.id).collect();
        for t in &mut self.tracks {
            if t.kind != TrackKind::Aux || t.instrument_output.is_some_and(|o| !instruments.contains(&o.track) || o.bus == 0) {
                t.instrument_output = None;
            }
        }
        for t in &mut self.tracks {
            if t.sends.len() < SEND_SLOTS {
                t.sends.resize(SEND_SLOTS, None);
            }
            let id = t.id;
            let folder = t.kind == TrackKind::Folder;
            for s in &mut t.sends {
                if s.map(|s| folder || s.bus == id || !auxes.contains(&s.bus)).unwrap_or(false) {
                    *s = None;
                }
            }
            if t.output.map(|o| folder || (o != MASTER_ID && (o == id || !auxes.contains(&o)))).unwrap_or(false) {
                t.output = None;
            }
            if t.kind == TrackKind::Aux {
                t.parent = None;
            }
        }
        // Break loops, cutting the edge that closes each one.
        for i in 0..self.tracks.len() {
            let id = self.tracks[i].id;
            if let Some(o) = self.tracks[i].output.filter(|&o| o != MASTER_ID) {
                if self.reaches(o, id) {
                    self.tracks[i].output = None;
                }
            }
            for k in 0..self.tracks[i].sends.len() {
                if let Some(s) = self.tracks[i].sends[k] {
                    if self.reaches(s.bus, id) {
                        self.tracks[i].sends[k] = None;
                    }
                }
            }
        }
        // Side-chains from tracks that are gone or carry no audio, and ones closing a loop. A
        // frozen track's plug-ins can't change (and aren't loaded), so they are left alone.
        let sources: HashSet<Id> = self.tracks.iter().filter(|t| t.kind != TrackKind::Folder).map(|t| t.id).collect();
        for i in 0..self.tracks.len() {
            if self.tracks[i].frozen.is_some() {
                continue;
            }
            let id = self.tracks[i].id;
            for k in 0..self.tracks[i].inserts.len() {
                let Some(src) = self.tracks[i].inserts[k].as_ref().and_then(|s| s.sidechain) else { continue };
                if src == id || !sources.contains(&src) || self.depends(id, src) {
                    if let Some(s) = &mut self.tracks[i].inserts[k] {
                        s.sidechain = None;
                    }
                }
            }
        }
        for s in self.master.inserts.iter_mut().flatten() {
            if s.sidechain.map(|src| !sources.contains(&src)).unwrap_or(false) {
                s.sidechain = None;
            }
        }
    }

    /// Samples per beat at the tempo and signature at position 0 (see `tempo::TempoMap` for
    /// positions elsewhere).
    pub fn samples_per_beat(&self) -> f64 {
        self.sample_rate as f64 * 60.0 / self.tempo * (4.0 / self.sig_den as f64)
    }

    /// Samples per bar at position 0.
    pub fn samples_per_bar(&self) -> f64 {
        self.samples_per_beat() * self.sig_num as f64
    }

    /// A copy safe to hand to the UI (plugin state blobs stripped). A sampler's state goes as a
    /// short fingerprint instead, which tells its panel when to read the zones again.
    pub fn for_ui(&self) -> Project {
        let mut p = self.clone();
        let strip = |s: &mut Option<PluginSlot>| {
            if let Some(s) = s {
                s.state = s.state.take().filter(|_| s.plugin_id == crate::instrument::sampler::ID).map(|text| {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    text.hash(&mut h);
                    format!("{:x}", h.finish())
                });
            }
        };
        for t in &mut p.tracks {
            strip(&mut t.instrument);
            t.inserts.iter_mut().for_each(strip);
            // The UI shows only the names of inactive track alternatives.
            t.alternatives.iter_mut().for_each(|a| a.regions.clear());
        }
        p.master.inserts.iter_mut().for_each(strip);
        p
    }
}
