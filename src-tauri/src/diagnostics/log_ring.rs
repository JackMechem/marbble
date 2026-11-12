//! The last few thousand log lines, kept in memory for the Diagnostics window as well as going to
//! stderr (`logger.rs` feeds it), with what the user did and what the engine's watch saw between
//! them (`activity.rs`). Bounded in lines and in each line's length, so a chatty plug-in can't grow
//! it. Its lock is its own and short; nothing on the audio thread logs, so nothing there ever takes
//! it.

use std::collections::VecDeque;

use parking_lot::Mutex;
use serde::Serialize;

/// How many lines it keeps, the oldest going first.
pub const LINES: usize = 2000;
/// Longer messages are cut here (in characters), with "…" after.
pub const LINE_CHARS: usize = 2000;

/// One record, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogLine {
    /// Counts up from 1 with every line, so the window asks only for those it hasn't got.
    pub seq: u64,
    /// Milliseconds since Marbble started.
    pub ms: u64,
    /// "error", "warn", "info", "debug" or "trace".
    pub level: &'static str,
    /// The module that logged it (`daw_engine::session::engine::device`), or for activity
    /// `activity::` and its kind (`activity::command`).
    pub target: String,
    pub message: String,
    /// A run of one command (a fader drag sends one per move) is one line, sent again with a new
    /// `seq` and the same `run` as it grows: the window replaces the one it has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<u64>,
}

/// The run of one command the newest line is.
struct Run {
    name: String,
    /// The `run` of its line.
    id: u64,
    first_ms: u64,
    last_ms: u64,
    count: u64,
}

#[derive(Default)]
struct Lines {
    lines: VecDeque<LogLine>,
    last_seq: u64,
    run: Option<Run>,
}

/// A run of one command goes on while the next comes within this of the one before, and until it
/// is this long, as in the activity log's file.
const RUN_QUIET_MS: u64 = 1000;
const RUN_LONGEST_MS: u64 = 5000;

/// See the module docs.
pub struct LogRing {
    inner: Mutex<Lines>,
    capacity: usize,
}

impl LogRing {
    pub fn new(capacity: usize) -> LogRing {
        LogRing { inner: Mutex::new(Lines::default()), capacity: capacity.max(1) }
    }

    /// Adds a line, dropping the oldest when full.
    pub fn push(&self, ms: u64, level: log::Level, target: &str, message: String) {
        let mut l = self.inner.lock();
        l.run = None;
        self.add(&mut l, ms, level_name(level), target.to_string(), message, None);
    }

    /// Adds what the user did or the engine's watch saw: `kind` as in the activity log ("menu",
    /// "gap"). Gaps and faults are warnings.
    pub fn push_activity(&self, ms: u64, kind: &str, text: String) {
        let level = if matches!(kind, "gap" | "fault") { "warn" } else { "info" };
        let mut l = self.inner.lock();
        l.run = None;
        self.add(&mut l, ms, level, format!("activity::{kind}"), text, None);
    }

    /// Adds a command the interface sent (`text`: its name and arguments). The same command again
    /// soon after, with nothing between, grows the line before into a run.
    pub fn push_command(&self, ms: u64, name: &str, text: String) {
        let mut l = self.inner.lock();
        let grows =
            l.run.as_ref().is_some_and(|r| r.name == name && ms.saturating_sub(r.last_ms) < RUN_QUIET_MS && ms.saturating_sub(r.first_ms) < RUN_LONGEST_MS);
        if !grows {
            // A run is known by its first line's number.
            l.run = Some(Run { name: name.to_string(), id: l.last_seq + 1, first_ms: ms, last_ms: ms, count: 0 });
        }
        let r = l.run.as_mut().expect("a run");
        r.count += 1;
        r.last_ms = ms;
        let (id, first_ms, count) = (r.id, r.first_ms, r.count);
        let message = if count == 1 { text } else { format!("{name} ×{count} over {:.1} s, the last: {text}", (ms - first_ms) as f64 / 1000.0) };
        if grows {
            l.lines.pop_back();
        }
        self.add(&mut l, first_ms, "info", "activity::command".into(), message, Some(id));
    }

    fn add(&self, l: &mut Lines, ms: u64, level: &'static str, target: String, message: String, run: Option<u64>) {
        l.last_seq += 1;
        let line = LogLine { seq: l.last_seq, ms, level, target, message: cut(message, LINE_CHARS), run };
        if l.lines.len() == self.capacity {
            l.lines.pop_front();
        }
        l.lines.push_back(line);
    }

    /// The lines after `seq` that it still holds, oldest first. 0 asks for all of them.
    pub fn since(&self, seq: u64) -> Vec<LogLine> {
        let l = self.inner.lock();
        // In order, but with gaps where a run's line was sent again.
        let skip = l.lines.partition_point(|line| line.seq <= seq);
        l.lines.iter().skip(skip).cloned().collect()
    }
}

fn level_name(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "error",
        log::Level::Warn => "warn",
        log::Level::Info => "info",
        log::Level::Debug => "debug",
        log::Level::Trace => "trace",
    }
}

/// `s` with at most `max` characters, "…" marking a cut.
fn cut(mut s: String, max: usize) -> String {
    if let Some((at, _)) = s.char_indices().nth(max) {
        s.truncate(at);
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Level;

    fn seqs(v: &[LogLine]) -> Vec<u64> {
        v.iter().map(|l| l.seq).collect()
    }

    #[test]
    fn keeps_the_newest_lines_and_hands_out_those_after_a_number() {
        let r = LogRing::new(3);
        assert!(r.since(0).is_empty());
        for i in 0..5 {
            r.push(i * 10, Level::Info, "daw", format!("line {i}"));
        }
        let all = r.since(0);
        assert_eq!(seqs(&all), [3, 4, 5], "the oldest went");
        assert_eq!((all[0].ms, all[0].message.as_str(), all[0].level), (20, "line 2", "info"));
        assert_eq!(seqs(&r.since(3)), [4, 5]);
        assert!(r.since(5).is_empty());
        assert!(r.since(99).is_empty());
        // Asking from before what it holds gives what it holds.
        assert_eq!(seqs(&r.since(1)), [3, 4, 5]);
    }

    #[test]
    fn a_run_of_one_command_is_one_line_sent_again_as_it_grows() {
        let r = LogRing::new(10);
        r.push_command(0, "update_track", "update_track {a}".into());
        r.push_command(300, "update_track", "update_track {b}".into());
        let first = r.since(0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].message, "update_track ×2 over 0.3 s, the last: update_track {b}");
        assert_eq!((first[0].ms, first[0].target.as_str(), first[0].run), (0, "activity::command", Some(1)));
        let seen = first[0].seq;
        r.push_command(600, "update_track", "update_track {c}".into());
        let fresh = r.since(seen);
        assert_eq!(fresh.len(), 1, "the grown line comes again");
        assert_eq!((fresh[0].run, fresh[0].message.contains("×3")), (Some(1), true));
        // Something else between ends the run; so does a pause.
        r.push(700, Level::Info, "daw", "between".into());
        r.push_command(800, "update_track", "update_track {d}".into());
        r.push_command(2000, "update_track", "update_track {e}".into());
        let all = r.since(0);
        let runs: Vec<_> = all.iter().filter_map(|l| l.run).collect();
        assert_eq!(runs.len(), 3);
        assert!(runs[0] < runs[1] && runs[1] < runs[2], "{runs:?}");
        assert!(all.windows(2).all(|w| w[0].seq < w[1].seq));
    }

    #[test]
    fn activity_is_kept_with_its_kind_and_gaps_are_warnings() {
        let r = LogRing::new(10);
        r.push_activity(5, "menu", "save".into());
        r.push_activity(6, "gap", "device gap at 0:01.000".into());
        let all = r.since(0);
        assert_eq!((all[0].target.as_str(), all[0].level), ("activity::menu", "info"));
        assert_eq!((all[1].target.as_str(), all[1].level), ("activity::gap", "warn"));
    }

    #[test]
    fn cuts_long_messages_at_a_character() {
        let r = LogRing::new(4);
        r.push(0, Level::Warn, "x", "é".repeat(LINE_CHARS + 5));
        let line = &r.since(0)[0];
        assert_eq!(line.message.chars().count(), LINE_CHARS + 1);
        assert!(line.message.ends_with('…'));
        assert_eq!(line.level, "warn");
        assert_eq!(cut("short".into(), 10), "short");
    }
}
