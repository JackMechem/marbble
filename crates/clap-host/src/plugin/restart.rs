//! What the controller does on the main thread that needs the plug-in stopped or told about a
//! render: restarting it when it asks (`request_restart`, for a new latency say), and switching
//! its render mode (`clap.render`) around offline renders.
//!
//! A restart deactivates and activates the plug-in again, which CLAP keeps to the main thread and
//! to a plug-in that isn't processing. The controller asks for the plug-in through the `Park`
//! (`park.rs`) and never waits for it in `idle`: each `idle` looks again, and once the processor
//! has faded out and parked (or no block has come for `PARK_WAIT`), restarts it, reads its
//! latency and gives it back, to fade in once that latency has passed. The session's next `idle`
//! then moves the delay compensation.

use std::time::Duration;

use clap_sys::ext::render::{CLAP_RENDER_OFFLINE, CLAP_RENDER_REALTIME};

use super::*;
use crate::host::AudioRole;

/// How long the controller waits for the processor to park itself before taking the plug-in
/// whenever no block is under way: long enough for a fade at the largest block size.
const PARK_WAIT: Duration = Duration::from_millis(250);
/// The longest a fade in waits for the plug-in's new latency to pass.
const MAX_SETTLE_SECS: f64 = 1.0;

impl ClapController {
    /// `latency.get`, which CLAP allows only on the main thread of an active plug-in.
    pub(super) fn read_latency(&self) -> u32 {
        if self.dead {
            return self.latency;
        }
        unsafe { self.shared.latency.as_ref() }.and_then(|l| l.get).map_or(0, |g| unsafe { g(self.shared.plugin) })
    }

    /// Notes a latency just read: for delay compensation and the processor's `ring_out`.
    pub(super) fn note_latency(&mut self, latency: u32) {
        self.latency_changed |= latency != self.latency;
        self.latency = latency;
        self.shared.latency_now.store(latency, Ordering::Relaxed);
    }

    /// Restarts the plug-in if it asked to be and the processor has let go of it; otherwise
    /// asks again. Returns whether no restart is waiting any more. Main thread.
    pub(super) fn restart_idle(&mut self) -> bool {
        if self.shared.take(|r| &r.restart) && !self.dead {
            self.restart.get_or_insert_with(Instant::now);
        }
        let Some(since) = self.restart else { return true };
        if !self.callable() {
            return false;
        }
        let s = &self.shared;
        // A plug-in that isn't processing makes no sound to fade, so it's taken at once.
        let take = since.elapsed() >= PARK_WAIT || !s.processing.load(Ordering::Relaxed);
        if !s.park.take(take) {
            return false;
        }
        self.restart = None;
        self.reactivate();
        true
    }

    /// Deactivates and activates the plug-in, which the controller has (`Park::take`), and gives
    /// it back to fade in after its new latency. One that won't activate again stays parked, its
    /// input passing through.
    fn reactivate(&mut self) {
        let s = self.shared.clone();
        let p = self.plugin();
        unsafe {
            if s.processing.swap(false, Ordering::Relaxed) {
                // The processor's call, made for it while it's kept out.
                let _role = AudioRole::enter();
                if let Some(f) = p.stop_processing {
                    f(s.plugin);
                }
            }
            if let Some(f) = p.deactivate {
                f(s.plugin);
            }
            s.inst.active.store(false, Ordering::Release);
            if !p.activate.is_some_and(|f| f(s.plugin, self.sample_rate, 1, self.max_block as u32)) {
                log::warn!("{} wouldn't start again after asking to be restarted: it stays silent until it's loaded again", self.desc.name);
                self.dead = true;
                return;
            }
        }
        s.inst.active.store(true, Ordering::Release);
        let latency = self.read_latency();
        self.note_latency(latency);
        s.requests().tail.store(true, Ordering::Release);
        log::debug!("{} restarted, latency {latency}", self.desc.name);
        s.park.give_back(latency.min((self.sample_rate * MAX_SETTLE_SECS) as u32));
    }

    /// `settle`: answers what the plug-in asked, as `idle` does, until no restart waits, waiting
    /// for the processor as long as `idle` would. A plug-in may ask for the restart from
    /// `on_main_thread`, which it asked for in the block that took the edit (LSP's do).
    pub(super) fn settle_restart(&mut self) {
        let deadline = Instant::now() + PARK_WAIT + Duration::from_millis(50);
        loop {
            plugin_api::Controller::idle(self);
            let waiting = self.restart.is_some() || self.shared.requests().callback.load(Ordering::Acquire);
            if !waiting || !self.callable() || Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// `clap.render`'s mode: offline for a render, unless the plug-in must play in real time
    /// (hardware behind it, say).
    pub(super) fn render_mode(&mut self, offline: bool) {
        if !self.callable() || self.offline == offline {
            return;
        }
        let Some(render) = (unsafe { self.shared.render.as_ref() }) else { return };
        let hard = || render.has_hard_realtime_requirement.is_some_and(|f| unsafe { f(self.shared.plugin) });
        if offline && hard() {
            return;
        }
        let mode = if offline { CLAP_RENDER_OFFLINE } else { CLAP_RENDER_REALTIME };
        if render.set.is_some_and(|f| unsafe { f(self.shared.plugin, mode) }) {
            self.offline = offline;
        }
    }
}
