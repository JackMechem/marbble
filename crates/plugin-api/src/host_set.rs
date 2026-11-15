//! What the host itself did to a plug-in's parameters, so that undo can tell it from edits in the
//! plug-in's editor.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long after the host set a parameter the same value from the plug-in counts as its answer.
const HOST_ECHO: Duration = Duration::from_millis(200);

/// Counts the host's changes (parameters set, states loaded) and keeps the values it set a moment
/// ago. Main thread.
#[derive(Default)]
pub struct HostSet {
    changes: AtomicU64,
    set: parking_lot::Mutex<HashMap<u32, (f64, Instant)>>,
}

impl HostSet {
    /// The host set a parameter to `value` (normalized).
    pub fn param(&self, id: u32, value: f64) {
        self.changes.fetch_add(1, Ordering::Relaxed);
        self.set.lock().insert(id, (value, Instant::now()));
    }

    /// The host changed the plug-in some other way (loaded a state).
    pub fn other(&self) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }

    /// How many changes so far.
    pub fn changes(&self) -> u64 {
        self.changes.load(Ordering::Relaxed)
    }

    /// Whether an edit from the plug-in only repeats what the host set a moment ago: an editor
    /// answering the host's value with an edit of its own, as some do.
    pub fn echo(&self, id: u32, value: f64) -> bool {
        let mut set = self.set.lock();
        let echo = set.get(&id).is_some_and(|(v, at)| (v - value).abs() < 1e-6 && at.elapsed() < HOST_ECHO);
        if !echo {
            set.remove(&id);
        }
        echo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_value_soon_after_is_an_echo_once() {
        let h = HostSet::default();
        h.param(3, 0.5);
        h.other();
        assert_eq!(h.changes(), 2);
        assert!(h.echo(3, 0.5));
        assert!(!h.echo(3, 0.6));
        assert!(!h.echo(3, 0.5), "a different value ends the window");
        assert!(!h.echo(4, 0.5));
    }
}
