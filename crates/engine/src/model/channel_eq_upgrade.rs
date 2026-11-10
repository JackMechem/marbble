//! Projects from when every strip had a fixed Channel EQ ahead of its Audio FX. The Channel EQ is
//! now an insert (`fx/channel_eq/`); opening such a project turns each strip's EQ into one, at
//! the top of its Audio FX, where it plays exactly as before.
//!
//! - The old fields (`Track::legacy_eq`, `Master::legacy_eq` and their presets) are only read,
//!   never written, so older projects keep opening for good and this runs once per project: the
//!   next save has the inserts instead.
//! - A flat EQ that no controller assignment drives goes, so strips don't carry an EQ (and its
//!   CPU) they never used. One that shapes the sound, or that a controller drives (assignments
//!   are global, so a flat EQ may still be moved from hardware), becomes an insert with the same
//!   values and preset. The fixed EQ had no automation lanes and groups didn't link it, so there
//!   is nothing else to carry over. Controller assignments to it (`CtlTarget::Eq`) follow the
//!   strip's first Channel EQ insert (see `session/control/controllers/`).
//! - A strip whose eight Audio FX slots are all taken can't take one more: its EQ is dropped and
//!   the strip named, so the session can say so.
//! - Channel strip settings and presets saved then are converted where they're read
//!   (`strip_settings::StripSetting::upgrade`, `presets::read_user`).

use super::*;
use crate::fx::channel_eq;

/// A Channel EQ insert playing a fixed Channel EQ's settings.
pub fn channel_eq_slot(uid: Id, eq: &ChannelEq, preset: Option<PresetRef>) -> PluginSlot {
    PluginSlot {
        uid,
        plugin_id: channel_eq::ID.into(),
        name: "Channel EQ".into(),
        bypass: false,
        state: None,
        params: channel_eq::values_of(eq),
        sidechain: None,
        sidechain_pre: false,
        preset,
    }
}

/// Puts `slot` at the top of a strip's Audio FX, the plug-ins below moving down into the first
/// free slot, as dropping a plug-in on an occupied slot does. False, with nothing changed, when
/// all `INSERT_SLOTS` are taken.
pub fn insert_first(inserts: &mut Vec<Option<PluginSlot>>, slot: PluginSlot) -> bool {
    let n = inserts.len().min(INSERT_SLOTS);
    match (0..n).find(|&i| inserts[i].is_none()) {
        Some(free) => {
            inserts.remove(free);
        }
        // A strip saved with fewer slots than there are has room at the end.
        None if inserts.len() < INSERT_SLOTS => {}
        None => return false,
    }
    inserts.insert(0, Some(slot));
    true
}

/// Whether a fixed Channel EQ becomes an insert: it shapes the sound, or a controller drives it.
fn kept(eq: &ChannelEq, controlled: bool) -> bool {
    !eq.is_flat() || controlled
}

impl Project {
    /// Turns each strip's fixed Channel EQ into a Channel EQ insert, or drops it (see the module
    /// notes). `controlled(strip)` says whether a controller assignment drives that strip's EQ.
    /// Returns the names of the strips whose EQ was dropped for want of a free Audio FX slot.
    pub fn upgrade_channel_eq(&mut self, controlled: impl Fn(Id) -> bool) -> Vec<String> {
        let mut full = Vec::new();
        if let Some(eq) = self.master.legacy_eq.take() {
            let preset = self.master.legacy_eq_preset.take();
            if kept(&eq, controlled(MASTER_ID)) {
                let slot = channel_eq_slot(self.alloc_id(), &eq, preset);
                if !insert_first(&mut self.master.inserts, slot) {
                    full.push("Stereo Out".to_string());
                }
            }
        }
        for i in 0..self.tracks.len() {
            let t = &mut self.tracks[i];
            let (Some(eq), preset) = (t.legacy_eq.take(), t.legacy_eq_preset.take()) else { continue };
            // A folder stack passes no audio of its own, so its EQ never played.
            if t.kind == TrackKind::Folder || !kept(&eq, controlled(t.id)) {
                continue;
            }
            let slot = channel_eq_slot(self.alloc_id(), &eq, preset);
            let t = &mut self.tracks[i];
            if !insert_first(&mut t.inserts, slot) {
                full.push(t.name.clone());
            }
        }
        full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shaped() -> ChannelEq {
        let mut eq = ChannelEq::default();
        eq.bands[3].gain = -4.0;
        eq
    }

    fn slot(uid: Id) -> Option<PluginSlot> {
        let mut s = channel_eq_slot(uid, &ChannelEq::default(), None);
        s.plugin_id = "builtin:compressor".into();
        Some(s)
    }

    #[test]
    fn an_eq_goes_to_the_top_and_the_rest_moves_down() {
        let mut v = vec![slot(1), slot(2), None, slot(3), None, None, None, None];
        assert!(insert_first(&mut v, channel_eq_slot(9, &shaped(), None)));
        let uids: Vec<Option<Id>> = v.iter().map(|s| s.as_ref().map(|s| s.uid)).collect();
        assert_eq!(uids, vec![Some(9), Some(1), Some(2), Some(3), None, None, None, None]);
        let mut short = vec![slot(1)];
        assert!(insert_first(&mut short, channel_eq_slot(9, &shaped(), None)));
        assert_eq!(short.len(), 2);
        let mut full: Vec<Option<PluginSlot>> = (1..=8).map(slot).collect();
        assert!(!insert_first(&mut full, channel_eq_slot(9, &shaped(), None)));
    }

    #[test]
    fn old_projects_keep_what_they_used() {
        // A project as saved before: every strip with its "eq", one of them shaped and one with
        // a preset loaded, one flat but driven by a controller, Stereo Out's shaped, and a strip
        // with every Audio FX slot taken.
        let mut p = Project::new("Old", 48000);
        for (name, kind) in
            [("Flat", TrackKind::Audio), ("Shaped", TrackKind::Audio), ("Learnt", TrackKind::Aux), ("Full", TrackKind::Audio), ("Folder", TrackKind::Folder)]
        {
            let id = p.alloc_id();
            let mut t = Track {
                id,
                kind,
                name: name.into(),
                color: String::new(),
                volume_db: 0.0,
                pan: 0.0,
                mute: false,
                solo: false,
                armed: false,
                monitor: false,
                input: InputSel::None,
                instrument: None,
                inserts: vec![None; INSERT_SLOTS],
                regions: Vec::new(),
                legacy_eq: None,
                parent: None,
                collapsed: false,
                group: None,
                hidden: false,
                sends: Vec::new(),
                output: None,
                bus: 0,
                automation: Vec::new(),
                auto_mode: AutoMode::Read,
                frozen: None,
                vca: None,
                alternatives: Vec::new(),
                alternative: String::new(),
                auto_lanes: Vec::new(),
                off: false,
                instrument_output: None,
                legacy_eq_preset: None,
                icon: None,
            };
            if name == "Full" {
                t.inserts = (100..108).map(slot).collect();
            }
            p.tracks.push(t);
        }
        let mut json = serde_json::to_value(&p).unwrap();
        let eq_of = |name: &str| if matches!(name, "Shaped" | "Full" | "Folder") { shaped() } else { ChannelEq::default() };
        for t in json["tracks"].as_array_mut().unwrap() {
            let name = t["name"].as_str().unwrap().to_string();
            t["eq"] = serde_json::to_value(eq_of(&name)).unwrap();
            if name == "Shaped" {
                t["eq_preset"] = serde_json::json!({ "name": "Mine", "factory": false });
            }
        }
        json["master"]["eq"] = serde_json::to_value(shaped()).unwrap();
        let mut old: Project = serde_json::from_value(json).unwrap();
        let learnt = old.tracks[2].id;
        let full = old.upgrade_channel_eq(|strip| strip == learnt);
        assert_eq!(full, vec!["Full".to_string()]);

        let first = |t: &Track| t.inserts[0].clone();
        assert!(old.tracks[0].inserts.iter().all(|s| s.is_none()), "a flat, unused EQ goes");
        let s = first(&old.tracks[1]).unwrap();
        assert_eq!(s.plugin_id, channel_eq::ID);
        assert!(s.params.contains(&(channel_eq::gain(3), -4.0)));
        assert_eq!(s.preset, Some(PresetRef { name: "Mine".into(), factory: false }));
        assert_eq!(first(&old.tracks[2]).unwrap().plugin_id, channel_eq::ID, "a controller drives it");
        assert_eq!(first(&old.tracks[3]).unwrap().uid, 100, "a full strip keeps its plug-ins");
        assert!(old.tracks[4].inserts.iter().all(|s| s.is_none()), "a folder's EQ never played");
        assert_eq!(old.master.inserts[0].as_ref().unwrap().plugin_id, channel_eq::ID);
        // Instance ids stay unique, and the old fields are gone for good.
        let mut uids: Vec<Id> = old.tracks.iter().flat_map(|t| t.inserts.iter().flatten()).chain(old.master.inserts.iter().flatten()).map(|s| s.uid).collect();
        uids.extend(old.tracks.iter().map(|t| t.id));
        let n = uids.len();
        uids.sort_unstable();
        uids.dedup();
        assert_eq!(uids.len(), n);
        assert!(old.tracks.iter().all(|t| t.legacy_eq.is_none() && t.legacy_eq_preset.is_none()) && old.master.legacy_eq.is_none());
        let saved = serde_json::to_string(&old).unwrap();
        assert!(!saved.contains("\"eq\"") && !saved.contains("eq_preset"));
        assert!(old.upgrade_channel_eq(|_| true).is_empty(), "nothing left to do the second time");
    }
}
