//! Tracks sitting out after a plug-in stopped responding on a processing thread (see
//! `Parallel::leave_out`): the message when it happens, the track brought back when the plug-in
//! returns, and what mustn't call into a plug-in a thread is still left in, or play without the
//! tracks sitting out.

use super::*;
use crate::dsp::graph::TaskGraph;
use crate::dsp::HeldStrip;

impl Session {
    /// The strips threads are still left in plug-ins with.
    pub(super) fn held(&self) -> Vec<HeldStrip> {
        self.status.held()
    }

    /// Whether the instance `instance` of the plug-in `uid` on `track` may be in use by a thread
    /// left in it, and must not be called (its state saved, say). With the plug-in unknown, every
    /// one on the track might be. An instance made after the thread was left (the track deleted
    /// and brought back by undo, say) is another, and can be.
    pub(super) fn left_in(held: &[HeldStrip], track: Id, uid: Id, instance: u64) -> bool {
        held.iter().any(|h| h.track == track && (h.plugin == uid || h.plugin == 0) && instance <= h.before)
    }

    /// Marks the controller of each plug-in a thread is left in as not to be called from here, and
    /// those of plug-ins that came back as callable again (`PluginController::set_unresponsive`):
    /// its editor, parameters, latency and state go untouched meanwhile, and an editor closed
    /// then is only hidden until the instance is gone. Before anything else in `idle`, and before
    /// dropping controllers wholesale.
    pub(super) fn mark_unresponsive(&mut self) {
        let held = self.held();
        if held.is_empty() && !self.marked_unresponsive {
            return;
        }
        self.marked_unresponsive = !held.is_empty();
        for t in &self.project.tracks {
            for s in t.inserts.iter().flatten().chain(t.instrument.iter()) {
                if let Some(c) = self.controllers.get_mut(&s.uid) {
                    let left = Self::left_in(&held, t.id, s.uid, c.instance());
                    c.set_unresponsive(left);
                }
            }
        }
    }

    /// For checks: marks a plug-in's controller as `mark_unresponsive` would one a processing
    /// thread is left in, which holds while no thread is.
    #[doc(hidden)]
    pub fn set_unresponsive_for_checks(&mut self, uid: Id, on: bool) {
        if let Some(c) = self.controllers.get_mut(&uid) {
            c.set_unresponsive(on);
        }
    }

    /// Rebuilds every track as Reset Audio Engine does while tracks sit out, for checks.
    #[doc(hidden)]
    pub fn rebuild_graph_for_checks(&mut self) {
        self.rebuild_graph();
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
        let unlisted = self.status.held_unlisted();
        if unlisted != self.held_unlisted_seen {
            self.held_unlisted_seen = unlisted;
            log::error!(
                "{unlisted} strips left in plug-ins couldn't be listed: their tracks won't come back by themselves, and their plug-ins may still be called"
            );
        }
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
            "{plugin} on \"{track}\" stopped responding, so \"{track}\" and anything it feeds sit out while the rest plays on.{stopped} They come back if it responds again, or with Reset Audio Engine in Settings › Audio. Saving keeps the settings it had before it stopped."
        ));
    }

    /// Brings back the tracks of plug-ins that responded again (in `idle`): the track gets a new
    /// strip with new instances of its plug-ins, from their state now, and what it fed plays
    /// again, unless also fed by a plug-in still not responding.
    pub(super) fn bring_back(&mut self) {
        // Not before the engine has applied every command sent: one deleting the track (whose
        // stand-in then goes, `HeldStrip::stand_in_gone`) may land in the same callback as the
        // thread lets go, after it's listed as back.
        if self.status.cmds_done.load(Ordering::Acquire) < self.cmds_sent {
            return;
        }
        let back = self.status.take_back();
        if back.is_empty() {
            return;
        }
        let held = self.held();
        let snapshot = self.capture_plugin_states();
        self.adopt_states(&snapshot, true);
        let mut still_out: Vec<Id> = Vec::new();
        if !held.is_empty() {
            let ids: Vec<Id> = held.iter().map(|h| h.track).collect();
            still_out = downstream(&self.project, &ids);
        }
        still_out.sort_unstable();
        for b in &back {
            let (id, uid, token) = (b.track, b.plugin, b.token);
            // The track was deleted meanwhile, and whatever it has now (an undo brought it back)
            // is new and plays; or a thread still holds another strip of it (left again after
            // that), whose stand-in stays. Either way its plug-ins aren't touched.
            if b.stand_in_gone || held.iter().any(|h| h.track == id) {
                self.send(Cmd::Back { token, track: None, still_out: still_out.clone() });
                continue;
            }
            let track = snapshot.track(id).map(|t| {
                for slot in t.inserts.iter().flatten().chain(t.instrument.iter()) {
                    if let Some(c) = self.controllers.get_mut(&slot.uid) {
                        c.close_editor();
                    }
                }
                self.build_track(t)
            });
            self.send(Cmd::Back { token, track, still_out: still_out.clone() });
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
            Some(h) => {
                let (plugin, track) = self.who(h.track, h.plugin);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::NULL_OUTPUT;
    use crate::dsp::parallel::{tar_for_checks, test_hooks};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    /// Only an instance made by the time the thread was left can be the one it's in: one made
    /// since (an undo bringing the track back) is another, and may be called.
    #[test]
    fn only_an_instance_made_before_a_strip_was_held_counts_as_held() {
        let h = HeldStrip { token: 1, track: 5, plugin: 9, before: 10, stand_in_gone: false };
        assert!(Session::left_in(&[h], 5, 9, 10));
        assert!(!Session::left_in(&[h], 5, 9, 11), "made after");
        assert!(!Session::left_in(&[h], 5, 8, 3), "another plug-in of the track");
        assert!(!Session::left_in(&[h], 6, 9, 3), "another track");
        let any = HeldStrip { plugin: 0, ..h };
        assert!(Session::left_in(&[any], 5, 8, 3), "with the plug-in unknown, any of the track's");
        assert!(!Session::left_in(&[any], 5, 8, 11));
    }

    fn session() -> Session {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    /// A render that starts while a block given up on is still open, with a worker left in a
    /// plug-in there but its track not yet sitting out, is refused once the block is over, rather
    /// than rendered without that track.
    #[test]
    fn a_render_started_while_a_block_stays_open_is_refused_once_it_ends() {
        let mut s = session();
        // Ids apart from the engine's tests, as `test_hooks` goes by track id.
        while s.project.alloc_id() < 100_000 {}
        let mut add = || s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        let (first, tar, held) = (add(), add(), add());
        // The caller waits in the first track's strip until workers have the other two: one is held
        // up in Tar, the other in its own task, which holds the block open.
        test_hooks::gate(first, &[tar, held]);
        test_hooks::hold_in_task(held, true);
        // Tar first, so that it's there for the first block on the workers.
        let hold = Arc::new(AtomicBool::new(true));
        s.insert_for_checks(tar, 7, tar_for_checks(777_777, s.sample_rate() as f32, hold.clone()));
        s.set_threads_for_checks(3, ParallelTest { always: true, renders: false, patient: false });
        s.play();
        let engine = s.engine_for_checks();
        {
            let mut e = engine.lock();
            let mut out = vec![0.0f32; 512];
            let started = Instant::now();
            while !e.stalled_for_tests() {
                assert!(started.elapsed().as_secs() < 20, "no block was given up on");
                e.process(&[], 0, &mut out, 2);
            }
        }
        assert_eq!(s.status.left_out.load(Ordering::Relaxed), 0, "nothing sits out yet");
        // The held worker gets through while the render waits for the block to end.
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            test_hooks::hold_in_task(held, false);
        });
        let r = s.render(0, 4800, 0, None, &mut |_| {});
        release.join().unwrap();
        let why = r.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(why.contains("sitting out"), "refused: {why:?}");
        assert!(s.status.left_out.load(Ordering::Relaxed) > 0);
        test_hooks::gate(first, &[]);
        hold.store(false, Ordering::SeqCst);
    }
}
