//! A task's link to the pool while it runs: the [`Job`] the pool runs, and the [`Call`] through
//! which a task says when it calls out into a plug-in, which may never return, so the caller can
//! leave a worker there ([`Pool::abandon`](super::Pool::abandon)).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Work the pool runs: `task` indexes the graph, `thread` is 0 for the caller and otherwise a
/// worker's number, so the job can keep per-thread scratch space. `call` is the thread's, to say
/// when it calls into a plug-in.
pub trait Job: Sync {
    fn run(&self, task: usize, thread: usize, call: &Call);
}

/// Where a thread's task is: in the pool's own code, or out in a plug-in, which may never return.
/// Only there can the caller leave a worker it gave up on ([`Pool::abandon`]), since a plug-in call
/// touches what its own task owns and nothing else of the block's.
pub struct Call {
    /// The phase in the low byte, and above it the number of the call, so that the caller leaves
    /// the very call it looked at and not one that began since.
    pub(super) state: AtomicU64,
    /// When the call began, in nanoseconds from `epoch`.
    since: AtomicU64,
    epoch: Instant,
    /// What the job said it was calling (a plug-in's uid), for the message.
    pub(super) what: AtomicU64,
}

const IN_TASK: u8 = 0;
const IN_PLUGIN: u8 = 1;
/// Left in the plug-in by the caller. From here the thread touches nothing of the block, the job or
/// the graph, which may all be gone by the time the plug-in returns, if it ever does.
pub(super) const LEFT: u8 = 2;
/// Left, then back out of the plug-in, and done with everything it was given.
pub(super) const GONE: u8 = 3;

pub(super) fn phase(state: u64) -> u8 {
    state as u8
}

fn with_phase(state: u64, p: u8) -> u64 {
    (state & !0xff) | p as u64
}

impl Call {
    pub(super) fn new(epoch: Instant) -> Call {
        Call { state: AtomicU64::new(IN_TASK as u64), since: AtomicU64::new(0), epoch, what: AtomicU64::new(0) }
    }

    fn now(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    /// Before a call into a plug-in, `what` naming it. Reads the clock (a vDSO call on Linux).
    pub fn enter(&self, what: u64) {
        // Only this thread moves the state on from `IN_TASK`, the phase it's in now.
        let s = self.state.load(Ordering::Relaxed);
        self.what.store(what, Ordering::Relaxed);
        self.since.store(self.now(), Ordering::Relaxed);
        self.state.store(with_phase(s.wrapping_add(1 << 8), IN_PLUGIN), Ordering::SeqCst);
    }

    /// After it. False if the caller left the thread there meanwhile: the task is no longer its own,
    /// and the job must return at once, touching nothing more.
    #[must_use]
    pub fn leave(&self) -> bool {
        let s = self.state.load(Ordering::SeqCst);
        match phase(s) {
            IN_PLUGIN => self.state.compare_exchange(s, with_phase(s, IN_TASK), Ordering::SeqCst, Ordering::SeqCst).is_ok(),
            // Not in a call at all, which is no harm.
            p => p == IN_TASK,
        }
    }

    /// Whether the caller left the thread in a plug-in.
    pub fn left(&self) -> bool {
        phase(self.state.load(Ordering::SeqCst)) >= LEFT
    }

    /// The caller's side: leaves the thread in the call it's in, if it has been in it for `least`.
    pub(super) fn leave_behind(&self, least: Duration) -> bool {
        self.look(least).is_some_and(|s| self.leave_at(s))
    }

    /// The call the thread is in, if it has been in it for `least`.
    pub(super) fn look(&self, least: Duration) -> Option<u64> {
        let s = self.state.load(Ordering::SeqCst);
        if phase(s) != IN_PLUGIN {
            return None;
        }
        // Written before the state: this call's start, or a later call's, whose number then fails
        // the exchange in `leave_at`.
        let since = self.since.load(Ordering::Relaxed);
        (self.now().saturating_sub(since) >= least.as_nanos() as u64).then_some(s)
    }

    /// Leaves the thread in the call `look` saw, unless it has come out of it since: if it has gone
    /// into another, that one's number differs.
    pub(super) fn leave_at(&self, s: u64) -> bool {
        self.state.compare_exchange(s, with_phase(s, LEFT), Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

    pub(super) fn gone(&self) -> bool {
        phase(self.state.load(Ordering::SeqCst)) == GONE
    }
}
