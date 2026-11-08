//! Tracks sitting out after a plug-in stopped responding on a processing thread (see
//! `Parallel::leave_out`): the message when it happens, the track brought back when the plug-in
//! returns, and what mustn't call into a plug-in a thread is still left in, or play without the
//! tracks sitting out.

use super::*;
use crate::dsp::graph::TaskGraph;

impl Session {
    /// The tracks and plug-ins (uid, 0: unknown) that threads are still left in.
    pub(super) fn held(&self) -> Vec<(Id, Id)> {
        self.status.held()
    }

    /// Whether the plug-in `uid` on `track` may be in use by a thread left in it, and must not be
    /// called (its state saved, say). With the plug-in unknown, every one on the track might be.
    pub(super) fn left_in(held: &[(Id, Id)], track: Id, uid: Id) -> bool {
        held.iter().any(|&(t, p)| t == track && (p == uid || p == 0))
    }

    /// `left_in` for a plug-in found by its uid alone.
    pub(super) fn plugin_left_in(&self, held: &[(Id, Id)], uid: Id) -> bool {
        !held.is_empty()
            && self.project.tracks.iter().any(|t| t.inserts.iter().flatten().chain(t.instrument.iter()).any(|s| s.uid == uid) && Self::left_in(held, t.id, uid))
    }

    /// The plug-in's name, if it's known, and its track's, for a message.
    fn who(&self, track: Id, uid: Id) -> (Option<String>, String) {
        let t = self.project.track(track);
        let name = t.map_or_else(|| "a track".to_string(), |t| t.name.clone());
        let plugin = t.and_then(|t| t.inserts.iter().flatten().chain(t.instrument.iter()).find(|s| s.uid == uid)).map(|s| s.name.clone());
        (plugin, name)
    }

    /// The message when tracks were left out (in `tick`). A recording stops, as the take now has a
    /// gap; playback goes on.
    pub(super) fn check_left_out(&mut self) {
        let n = self.status.left_events.load(Ordering::Acquire);
        if n == self.left_events_seen {
            return;
        }
        self.left_events_seen = n;
        let recording = self.is_recording();
        if recording {
            self.stop();
        }
        let (plugin, track) = self.who(self.status.left_track.load(Ordering::Relaxed), self.status.left_plugin.load(Ordering::Relaxed));
        let plugin = plugin.unwrap_or_else(|| "A plug-in".into());
        log::warn!("{plugin} on \"{track}\" stopped responding: that track and what it feeds sit out");
        let stopped = if recording { " Recording stopped." } else { "" };
        // Above an overload's message from the same moment, which says less.
        self.warning = Some(format!(
            "{plugin} on \"{track}\" stopped responding, so \"{track}\" and anything it feeds sit out while the rest plays on.{stopped} They come back if it responds again, or with Reset Audio Engine in Settings › Audio. Saving keeps the settings it was last saved with."
        ));
    }

    /// Brings back the tracks of plug-ins that responded again (in `idle`): the track gets a new
    /// strip with new instances of its plug-ins, from their state now, and what it fed plays
    /// again, unless also fed by a plug-in still not responding.
    pub(super) fn bring_back(&mut self) {
        let back = self.status.take_back();
        if back.is_empty() {
            return;
        }
        let held = self.held();
        let snapshot = self.capture_plugin_states();
        let mut still_out: Vec<Id> = Vec::new();
        if !held.is_empty() {
            let ids: Vec<Id> = held.iter().map(|&(t, _)| t).collect();
            still_out = downstream(&self.project, &ids);
        }
        still_out.sort_unstable();
        for &(id, uid) in &back {
            let track = snapshot.track(id).map(|t| {
                for slot in t.inserts.iter().flatten().chain(t.instrument.iter()) {
                    if let Some(c) = self.controllers.get_mut(&slot.uid) {
                        c.close_editor();
                    }
                }
                self.build_track(t)
            });
            self.send(Cmd::Back { track, still_out: still_out.clone() });
            if self.project.track(id).is_some() {
                let (plugin, name) = self.who(id, uid);
                let plugin = plugin.unwrap_or_else(|| "A plug-in".into());
                log::info!("{plugin} on \"{name}\" responds again: the track is back");
                self.warning = Some(format!("{plugin} on \"{name}\" responds again, so \"{name}\" and what it feeds are back."));
            }
        }
        // The new strips start without delay compensation.
        self.update_pdc();
    }

    /// Why a render can't run now: tracks are sitting out, and would be missing from it.
    pub(super) fn left_out_refusal(&self) -> Option<String> {
        let n = self.status.left_out.load(Ordering::Relaxed);
        if n == 0 {
            return None;
        }
        let tracks = if n == 1 { "1 track is".to_string() } else { format!("{n} tracks are") };
        let after = match self.held().first() {
            Some(&(t, p)) => {
                let (plugin, track) = self.who(t, p);
                let plugin = plugin.unwrap_or_else(|| "a plug-in".into());
                format!(" after {plugin} on \"{track}\" stopped responding")
            }
            None => String::new(),
        };
        Some(format!("{tracks} sitting out{after}, so they'd be missing. Use Reset Audio Engine in Settings › Audio first."))
    }
}

/// `ids` and every track that waits for any of them in the project's task graph.
fn downstream(p: &Project, ids: &[Id]) -> Vec<Id> {
    let Ok(g) = TaskGraph::of_project(p) else { return ids.to_vec() };
    let mut out = vec![false; g.len()];
    for i in 0..g.len() {
        out[i] = ids.contains(&g.id(i)) || g.deps(i).iter().any(|&d| out[d as usize]);
    }
    let mut all: Vec<Id> = (0..g.len()).filter(|&i| out[i]).map(|i| g.id(i)).collect();
    let missing: Vec<Id> = ids.iter().copied().filter(|id| !all.contains(id)).collect();
    all.extend(missing);
    all
}
