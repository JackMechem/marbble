//! Settings › Audio › Processing Threads on the session's side: starting and stopping the threads
//! that play tracks, off the audio thread, and keeping the engine's task graph up to date (see
//! `dsp/parallel/`).

use crate::audio_io::threads::{self as setting, LeftOut, ThreadChoices, Topology};
use crate::dsp::graph::{GraphError, TaskGraph};
use crate::dsp::pool::{Health, HealthReport, PoolOptions};
use crate::dsp::{InsertDsp, ParallelTest, Workers};
use crate::session::*;

/// Whether a command can change which strips read which.
pub(crate) fn changes_routes(cmd: &Cmd) -> bool {
    matches!(
        cmd,
        Cmd::AddTrack(..)
            | Cmd::RemoveTrack(_)
            | Cmd::Order(_)
            | Cmd::Params(..)
            | Cmd::Insert { .. }
            | Cmd::Instrument { .. }
            | Cmd::ReorderInserts { .. }
            | Cmd::Sidechain { .. }
    )
}

/// What the session knows about the workers it started.
#[derive(Default)]
pub(crate) struct ThreadState {
    pub topology: Topology,
    /// What the engine is running, as (threads, real time).
    pub running: Option<(usize, bool)>,
    /// The last graph it was sent.
    pub graph: Option<TaskGraph>,
    /// Checks route everything through the pool, so they want it whatever the project.
    pub test: ParallelTest,
    /// A measurement chose the workers itself.
    pub forced: bool,
    /// How the workers the engine has are doing.
    pub health: Option<Arc<Health>>,
    /// What of it has been logged.
    pub logged: HealthReport,
    /// Start the workers afresh even if the setting is the same (the audio restarted).
    pub renew: bool,
}

impl Session {
    /// The threads the setting means on this machine.
    pub fn processing_threads(&self) -> usize {
        setting::resolve(self.audio_config.processing_threads, &self.threads.topology, self.suggested_threads())
    }

    /// How many threads the running device's workgroup suggests (macOS), which Automatic stays
    /// within.
    fn suggested_threads(&self) -> Option<u32> {
        self.io_thread().and_then(|io| io.suggested_threads())
    }

    /// What Settings › Audio offers, and which threads the running pool left out.
    pub fn thread_choices(&self) -> ThreadChoices {
        let mut c = setting::choices(&self.threads.topology, self.suggested_threads());
        c.left_out = self.threads_left_out();
        c
    }

    /// Threads the engine's pool asked for but plays without, once they've all started: refused
    /// real-time priority, or given up on in a plug-in that kept one too long.
    pub fn threads_left_out(&self) -> Option<LeftOut> {
        let r = self.threads.health.as_ref()?.report();
        (r.starting == 0 && r.refused + r.stuck > 0).then_some(LeftOut { playing: r.playing, threads: r.threads, refused: r.refused, stuck: r.stuck })
    }

    /// Logs, once, what the pool left out since the last look.
    fn log_threads_left_out(&mut self) {
        let Some(h) = &self.threads.health else { return };
        let r = h.report();
        if r.starting > 0 {
            return;
        }
        let told = &mut self.threads.logged;
        if r.refused > told.refused {
            let why = h.refusal().unwrap_or("the thread couldn't start");
            log::warn!(
                "processing threads refused real-time priority: {} of {} ({why}); they play nothing, and tracks play on {} threads",
                r.refused,
                r.threads - 1,
                r.playing
            );
        }
        if r.realtime > told.realtime || r.joined > told.joined {
            log::info!("processing threads: {} of {} real time, {} in the audio device's workgroup", r.realtime, r.threads - 1, r.joined);
        }
        if r.stuck > told.stuck {
            log::warn!("processing threads given up on in a plug-in, demoted and left out: {}; tracks play on {} threads", r.stuck, r.playing);
        }
        *told = r;
    }

    fn started_workers(&mut self, opts: PoolOptions) {
        self.threads.renew = false;
        let w = Workers::new(opts);
        self.threads.health = Some(w.health());
        self.threads.logged = HealthReport::default();
        self.send(Cmd::Workers(Some(w)));
    }

    /// Brings the workers and the graph in line with the setting and the project, after an edit.
    /// Allocates and may start or stop threads, as the audio thread never does; the engine takes
    /// the results between two blocks and sends back what it replaced, to be dropped in `idle`. Until
    /// a new graph reaches it, the engine notices that the old one doesn't fit the routes and plays
    /// the affected blocks serially, so this only needs to come soon after the edit.
    pub fn sync_parallel(&mut self) {
        self.log_threads_left_out();
        if !std::mem::take(&mut self.graph_dirty) {
            return;
        }
        let threads = self.processing_threads();
        if threads <= 1 {
            self.stop_workers();
            return;
        }
        let graph = match TaskGraph::of_project(&self.project) {
            Ok(g) => Some(g),
            Err(e @ (GraphError::TooBig(_) | GraphError::Unknown(_) | GraphError::Backwards { .. })) => {
                log::warn!("playing tracks on one thread: {e}");
                None
            }
        };
        // Not for a project with nothing to gain, so a light session never has the threads at all.
        let worth = self.threads.test != ParallelTest::default() || graph.as_ref().is_some_and(|g| g.heavy_width() >= 2);
        if graph != self.threads.graph {
            self.threads.graph.clone_from(&graph);
            self.send(Cmd::Graph(graph.map(Box::new)));
        }
        let realtime = self.audio_status.running && self.audio_config.output.as_deref() != Some(audio_io::NULL_OUTPUT);
        let wanted = (threads, realtime);
        if worth && !self.threads.forced && (self.threads.running != Some(wanted) || self.threads.renew) {
            let opts = PoolOptions {
                threads,
                realtime: realtime.then_some((self.audio_status.buffer_size, self.audio_status.sample_rate)),
                cpus: self.threads.topology.fast.clone(),
                io_thread: self.audio.as_ref().filter(|_| realtime).map(|a| a.io_thread().clone()).unwrap_or_default(),
                ..Default::default()
            };
            self.threads.running = Some(wanted);
            self.started_workers(opts);
        }
    }

    fn stop_workers(&mut self) {
        if self.threads.running.take().is_some() {
            self.threads.health = None;
            self.send(Cmd::Workers(None));
        }
        if self.threads.graph.take().is_some() {
            self.send(Cmd::Graph(None));
        }
    }

    /// Render ahead (#448): the source of tracks that aren't live rendered `lead_secs` ahead of the
    /// playhead on `threads` render threads; None plays every track live. Off by default while it's
    /// being built; checks and measurements switch it on.
    pub fn set_render_ahead(&mut self, ahead: Option<(usize, f64)>) {
        match ahead {
            Some((threads, lead_secs)) => {
                let fresh = self.ahead.is_none();
                let t = crate::dsp::ahead::AheadThreads::start(threads, self.status.clone());
                self.send(Cmd::Ahead(Some(t.shared()), lead_secs));
                // The old threads end once the engine has let go of them: a strip they hold is
                // taken back as the engine gets the new ones.
                self.ahead = Some(t);
                if fresh {
                    let ctls: Vec<_> = self.project.tracks.iter().filter_map(|t| crate::dsp::ahead::ctl_for(t.kind).map(|c| (t.id, c))).collect();
                    self.send(Cmd::AheadCtls(ctls));
                }
            }
            None => {
                self.send(Cmd::Ahead(None, 0.0));
                self.ahead = None;
            }
        }
    }

    /// For measurements: starts workers exactly as given (real time on the null device, say).
    #[doc(hidden)]
    pub fn start_workers_for_checks(&mut self, opts: PoolOptions) {
        self.threads.forced = true;
        self.threads.running = Some((opts.threads, opts.realtime.is_some()));
        self.started_workers(opts);
    }

    /// For checks: how the workers the engine was last sent are doing.
    #[doc(hidden)]
    pub fn threads_health_for_checks(&self) -> Option<HealthReport> {
        self.threads.health.as_ref().map(|h| h.report())
    }

    /// For checks: whether the engine would trust its task graph right now.
    #[doc(hidden)]
    pub fn pool_usable_for_checks(&mut self) -> bool {
        self.sync_parallel();
        self.engine.lock().task_graph_fits()
    }

    /// For checks: puts `insert` straight into a track's slot on the engine, out of the project's
    /// sight (`dsp::parallel::tar_for_checks`, say).
    #[doc(hidden)]
    pub fn insert_for_checks(&mut self, track: Id, slot: usize, insert: InsertDsp) {
        self.send(Cmd::Insert { track, slot, insert: Some(insert) });
    }

    /// For checks: sets the setting and routes blocks through the pool as `test` says (see
    /// [`ParallelTest`]), and takes effect by the next block. A patient `test` is in effect when
    /// this returns, and an overload the engine reported before it (a check's first blocks on a
    /// loaded machine) is forgotten, so the next [`Session::tick`] doesn't stop a transport for it.
    pub fn set_threads_for_checks(&mut self, threads: u32, test: ParallelTest) {
        self.audio_config.processing_threads = threads;
        self.threads.test = test;
        self.send(Cmd::ParallelTest(test));
        self.graph_dirty = true;
        self.sync_parallel();
        if test.patient {
            self.wait_for_engine();
            self.overloads_seen = self.status.overloads.load(Ordering::Relaxed);
        }
    }
}
