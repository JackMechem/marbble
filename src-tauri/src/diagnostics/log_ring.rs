//! The last few thousand log lines, kept in memory for the Diagnostics window as well as going to
//! stderr (`logger.rs` feeds it). Bounded in lines and in each line's length, so a chatty plug-in
//! can't grow it. Its lock is its own and short; nothing on the audio thread logs, so nothing there
//! ever takes it.

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
    /// The module that logged it (`daw_engine::session::engine::device`).
    pub target: String,
    pub message: String,
}

#[derive(Default)]
struct Lines {
    lines: VecDeque<LogLine>,
    last_seq: u64,
}

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
        let message = cut(message, LINE_CHARS);
        let mut l = self.inner.lock();
        l.last_seq += 1;
        let line = LogLine { seq: l.last_seq, ms, level: level_name(level), target: target.to_string(), message };
        if l.lines.len() == self.capacity {
            l.lines.pop_front();
        }
        l.lines.push_back(line);
    }

    /// The lines after `seq` that it still holds, oldest first. 0 asks for all of them.
    pub fn since(&self, seq: u64) -> Vec<LogLine> {
        let l = self.inner.lock();
        // Sequence numbers are consecutive, so the first one wanted is found by arithmetic.
        let first = l.lines.front().map_or(0, |f| f.seq);
        let skip = (seq + 1).saturating_sub(first) as usize;
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
