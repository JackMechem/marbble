//! Restarting the audio by itself when its device changes or goes away (unplugged headphones, a
//! new system output, an interface switched to another rate).
//!
//! cpal reports these through the streams' error callbacks. On macOS they run on its listener
//! threads, but some (overloads, a bad timestamp; every error on ALSA) come from the audio thread,
//! so [`DeviceEvents`] only stores atomics there. `Session::idle` takes them and asks [`Watch`]
//! when to restart: once the events have settled ([`DEBOUNCE`]), with retries further and further
//! apart when no device opens, and never more than [`MAX_RESTARTS`] in [`WINDOW`], after which
//! it gives up until the user resets the audio engine or picks a device.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use cpal::ErrorKind;

/// Why the device needs reopening, worst last (several events keep the worst).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// The system default output moved to another device, and the stream followed it (macOS):
    /// its rate, buffer size and IO thread are the new device's now.
    DefaultChanged = 1,
    /// The device's configuration changed under the stream, e.g. its sample rate.
    Invalidated = 2,
    /// The input device went away.
    InputGone = 3,
    /// The output device went away.
    OutputGone = 4,
}

impl Reason {
    fn from_u8(n: u8) -> Option<Reason> {
        match n {
            1 => Some(Reason::DefaultChanged),
            2 => Some(Reason::Invalidated),
            3 => Some(Reason::InputGone),
            4 => Some(Reason::OutputGone),
            _ => None,
        }
    }

    /// What a stream's error means for the device, if it needs reopening.
    ///
    /// A changed default only counts on macOS: there the stream is the device's own AudioUnit,
    /// rerouted, and the engine's buffer size and IO workgroup are the old device's. PipeWire's
    /// graph takes a moved stream in its stride (and resamples), so reopening would only glitch.
    pub fn of(kind: ErrorKind, input: bool) -> Option<Reason> {
        match kind {
            ErrorKind::DeviceNotAvailable if input => Some(Reason::InputGone),
            ErrorKind::DeviceNotAvailable => Some(Reason::OutputGone),
            ErrorKind::StreamInvalidated => Some(Reason::Invalidated),
            ErrorKind::DeviceChanged if cfg!(target_os = "macos") => Some(Reason::DefaultChanged),
            _ => None,
        }
    }

    /// The end of "Audio restarted on X because …".
    pub fn because(self, old_output: Option<&str>) -> String {
        match (self, old_output) {
            (Reason::DefaultChanged, _) => "the system output changed".into(),
            (Reason::Invalidated, _) => "the device's settings changed".into(),
            (Reason::InputGone, _) => "the input device was disconnected".into(),
            (Reason::OutputGone, Some(name)) => format!("\u{201c}{name}\u{201d} was disconnected"),
            (Reason::OutputGone, None) => "the output device was disconnected".into(),
        }
    }
}

/// What the streams' error callbacks leave for the session: the worst reason to reopen the device
/// since it was last taken, and the other errors (not underruns) for the log. Only atomics, as
/// some callbacks run on the audio thread.
#[derive(Debug, Default)]
pub struct DeviceEvents {
    reason: AtomicU8,
    /// Events that call for a restart; each one restarts the debounce.
    events: AtomicU32,
    others: AtomicU32,
    /// The last of the others, as its index in [`OTHER_KINDS`].
    last_other: AtomicU8,
}

/// The errors besides restarts and underruns, by the index `DeviceEvents` keeps of them.
const OTHER_KINDS: [ErrorKind; 12] = [
    ErrorKind::Other,
    ErrorKind::DeviceBusy,
    ErrorKind::DeviceChanged,
    ErrorKind::HostUnavailable,
    ErrorKind::InvalidInput,
    ErrorKind::PermissionDenied,
    ErrorKind::RealtimeDenied,
    ErrorKind::ResourceExhausted,
    ErrorKind::UnsupportedConfig,
    ErrorKind::UnsupportedOperation,
    ErrorKind::BackendError,
    ErrorKind::StreamInvalidated,
];

impl DeviceEvents {
    /// The stream error callback's whole job.
    pub fn note(&self, kind: ErrorKind, input: bool) {
        match Reason::of(kind, input) {
            Some(r) => {
                self.reason.fetch_max(r as u8, Ordering::Relaxed);
                self.events.fetch_add(1, Ordering::Relaxed);
            }
            // Underruns show in the engine's own late-block count already.
            None if kind == ErrorKind::Xrun => {}
            None => {
                let i = OTHER_KINDS.iter().position(|k| *k == kind).unwrap_or(0);
                self.last_other.store(i as u8, Ordering::Relaxed);
                self.others.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// How many restart events and other errors there have been so far.
    pub fn counts(&self) -> (u32, u32) {
        (self.events.load(Ordering::Relaxed), self.others.load(Ordering::Relaxed))
    }

    /// The kind of the last error that was neither a restart nor an underrun.
    pub fn last_other(&self) -> ErrorKind {
        OTHER_KINDS[self.last_other.load(Ordering::Relaxed) as usize % OTHER_KINDS.len()]
    }

    /// The worst reason since the last call.
    pub fn take_reason(&self) -> Option<Reason> {
        Reason::from_u8(self.reason.swap(0, Ordering::Relaxed))
    }
}

/// How long the events must have settled before the device reopens: unplugging or switching
/// sends a few in a row, and the system needs a moment to settle on its new default.
pub const DEBOUNCE: Duration = Duration::from_millis(400);
/// The most restarts (failed or not) in [`WINDOW`] before giving up: a device that keeps dropping
/// out mustn't keep the app reopening it.
pub const MAX_RESTARTS: usize = 3;
pub const WINDOW: Duration = Duration::from_secs(30);
/// When a restart opened no device, the next try waits this long, doubling each time.
pub const RETRY: Duration = Duration::from_secs(1);

/// What [`Watch::poll`] says to do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Wait,
    Restart,
    /// Too many restarts: stop trying (reported once) until [`Watch::reset`].
    GiveUp,
}

/// When to restart, as a pure function of the events and the time; see the module docs.
#[derive(Debug, Default)]
pub struct Watch {
    last_event: Option<Instant>,
    retry_at: Option<Instant>,
    failures: u32,
    recent: Vec<Instant>,
    gave_up: bool,
}

impl Watch {
    /// A device event came in at `now`.
    pub fn event(&mut self, now: Instant) {
        if !self.gave_up {
            self.last_event = Some(now);
        }
    }

    /// Whether a restart (or a retry) is due at `now`.
    pub fn poll(&mut self, now: Instant) -> Step {
        if self.gave_up {
            return Step::Wait;
        }
        let settled = self.last_event.is_some_and(|t| now.duration_since(t) >= DEBOUNCE);
        let retry = self.retry_at.is_some_and(|t| now >= t);
        if !settled && !retry {
            return Step::Wait;
        }
        self.recent.retain(|t| now.duration_since(*t) < WINDOW);
        if self.recent.len() >= MAX_RESTARTS {
            self.gave_up = true;
            self.last_event = None;
            self.retry_at = None;
            return Step::GiveUp;
        }
        Step::Restart
    }

    /// The restart [`Watch::poll`] asked for happened at `now`, and opened a device or not.
    pub fn restarted(&mut self, now: Instant, ok: bool) {
        self.last_event = None;
        self.recent.push(now);
        if ok {
            self.failures = 0;
            self.retry_at = None;
        } else {
            self.retry_at = Some(now + RETRY * (1 << self.failures.min(6)));
            self.failures += 1;
        }
    }

    /// Whether it gave up (and waits for [`Watch::reset`]).
    pub fn gave_up(&self) -> bool {
        self.gave_up
    }

    /// A fresh start, when the user reopens the device themselves.
    pub fn reset(&mut self) {
        *self = Watch::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn waits_for_the_events_to_settle() {
        let t0 = Instant::now();
        let mut w = Watch::default();
        assert_eq!(w.poll(t0), Step::Wait, "nothing happened");
        w.event(t0);
        assert_eq!(w.poll(t0 + ms(100)), Step::Wait);
        // Another event starts the wait again.
        w.event(t0 + ms(300));
        assert_eq!(w.poll(t0 + ms(500)), Step::Wait);
        assert_eq!(w.poll(t0 + ms(700)), Step::Restart);
        w.restarted(t0 + ms(700), true);
        assert_eq!(w.poll(t0 + ms(5000)), Step::Wait, "one restart per burst");
    }

    #[test]
    fn retries_further_apart_when_no_device_opens() {
        let t0 = Instant::now();
        let mut w = Watch::default();
        w.event(t0);
        assert_eq!(w.poll(t0 + DEBOUNCE), Step::Restart);
        let t1 = t0 + DEBOUNCE;
        w.restarted(t1, false);
        assert_eq!(w.poll(t1 + ms(900)), Step::Wait);
        assert_eq!(w.poll(t1 + ms(1000)), Step::Restart);
        let t2 = t1 + ms(1000);
        w.restarted(t2, false);
        assert_eq!(w.poll(t2 + ms(1900)), Step::Wait);
        assert_eq!(w.poll(t2 + ms(2000)), Step::Restart);
        let t3 = t2 + ms(2000);
        w.restarted(t3, false);
        // A fourth try within the window is one too many.
        assert_eq!(w.poll(t3 + ms(4000)), Step::GiveUp);
        assert!(w.gave_up());
        w.event(t3 + ms(5000));
        assert_eq!(w.poll(t3 + ms(9000)), Step::Wait, "given up for good");
        assert_eq!(w.poll(t3 + ms(60_000)), Step::Wait);
    }

    #[test]
    fn a_device_that_keeps_dropping_out_is_given_up_on() {
        let t0 = Instant::now();
        let mut w = Watch::default();
        let mut now = t0;
        for _ in 0..MAX_RESTARTS {
            w.event(now);
            now += DEBOUNCE;
            assert_eq!(w.poll(now), Step::Restart);
            w.restarted(now, true);
            now += ms(2000);
        }
        w.event(now);
        assert_eq!(w.poll(now + DEBOUNCE), Step::GiveUp);
        assert_eq!(w.poll(now + DEBOUNCE + ms(10)), Step::Wait, "reported once");
        // The user resets the engine: a fresh budget.
        w.reset();
        w.event(now + ms(1000));
        assert_eq!(w.poll(now + ms(1000) + DEBOUNCE), Step::Restart);
    }

    #[test]
    fn restarts_spread_over_time_never_give_up() {
        let t0 = Instant::now();
        let mut w = Watch::default();
        for i in 0..10u32 {
            let now = t0 + WINDOW / 2 * i;
            w.event(now);
            assert_eq!(w.poll(now + DEBOUNCE), Step::Restart, "restart {i}");
            w.restarted(now + DEBOUNCE, true);
        }
    }

    #[test]
    fn a_success_ends_the_retries() {
        let t0 = Instant::now();
        let mut w = Watch::default();
        w.event(t0);
        assert_eq!(w.poll(t0 + DEBOUNCE), Step::Restart);
        w.restarted(t0 + DEBOUNCE, false);
        assert_eq!(w.poll(t0 + DEBOUNCE + RETRY), Step::Restart);
        w.restarted(t0 + DEBOUNCE + RETRY, true);
        assert_eq!(w.poll(t0 + ms(20_000)), Step::Wait);
    }

    #[test]
    fn events_keep_the_worst_reason_and_skip_underruns() {
        let e = DeviceEvents::default();
        e.note(ErrorKind::Xrun, false);
        assert_eq!(e.counts(), (0, 0));
        assert_eq!(e.take_reason(), None);
        e.note(ErrorKind::StreamInvalidated, false);
        e.note(ErrorKind::DeviceNotAvailable, false);
        e.note(ErrorKind::StreamInvalidated, false);
        e.note(ErrorKind::BackendError, false);
        assert_eq!(e.counts(), (3, 1));
        assert_eq!(e.last_other(), ErrorKind::BackendError);
        assert_eq!(e.take_reason(), Some(Reason::OutputGone));
        assert_eq!(e.take_reason(), None, "taken");
        e.note(ErrorKind::DeviceNotAvailable, true);
        assert_eq!(e.take_reason(), Some(Reason::InputGone));
    }

    #[test]
    fn a_new_default_restarts_only_on_macos() {
        assert_eq!(Reason::of(ErrorKind::DeviceChanged, false).is_some(), cfg!(target_os = "macos"));
        assert_eq!(Reason::of(ErrorKind::Xrun, false), None);
        assert_eq!(Reason::of(ErrorKind::RealtimeDenied, false), None);
    }
}
