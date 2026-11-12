//! What the activity log (the Diagnostics window's Record Everything,
//! `src-tauri/src/diagnostics/activity.rs`) looks at a few times a second: the device, the load
//! counters, the threads and the transport, and the lines that say what changed between two looks.
//! It reads the same things as the window's snapshot but leaves the tracks alone, as reading those
//! starts each strip's peak afresh under the window.

use super::*;
use crate::session::*;

/// One look.
#[derive(Debug, Clone)]
pub struct Watch {
    pub audio: AudioDiagnostics,
    pub load: LoadDiagnostics,
    pub threads: ThreadDiagnostics,
    pub pos: i64,
    pub playing: bool,
    pub recording: bool,
}

/// What a line in the log is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchKind {
    /// The device opening, stopping, changing or failing.
    Device,
    /// Audio that didn't play as it should: gaps, late buffers, overloads, silenced output.
    Gap,
    /// Something else that went wrong: dropped commands, recording overruns, stuck threads.
    Fault,
    Transport,
}

impl WatchKind {
    pub fn name(self) -> &'static str {
        match self {
            WatchKind::Device => "device",
            WatchKind::Gap => "gap",
            WatchKind::Fault => "fault",
            WatchKind::Transport => "transport",
        }
    }
}

impl Session {
    /// A look for the activity log. Like [`Session::diagnostics`], it never locks the engine.
    pub fn watch(&self) -> Watch {
        Watch {
            audio: self.audio_diagnostics(),
            load: self.load_diagnostics(),
            threads: self.thread_diagnostics(),
            pos: self.position(),
            playing: self.is_playing(),
            recording: self.is_recording(),
        }
    }
}

/// How much a counter grew. One that went down started afresh (a reopened device counts its own),
/// so all of it is new.
fn grew(before: u64, now: u64) -> u64 {
    if now >= before {
        now - before
    } else {
        now
    }
}

/// " ×3", or nothing for one.
fn times(n: u64) -> String {
    if n == 1 {
        String::new()
    } else {
        format!(" ×{n}")
    }
}

impl Watch {
    /// Where the playhead is, in seconds and frames: "0:12.345 (592560)".
    pub fn at(&self) -> String {
        let rate = self.audio.sample_rate.max(1) as i64;
        let ms = self.pos.max(0) * 1000 / rate;
        format!("{}:{:02}.{:03} ({})", ms / 60_000, ms / 1000 % 60, ms % 1000, self.pos)
    }

    /// Where it happened: playing, recording or stopped, and the playhead.
    fn when(&self) -> String {
        let state = if self.recording {
            "recording"
        } else if self.playing {
            "playing"
        } else {
            "stopped"
        };
        format!("{state} at {}", self.at())
    }

    /// What changed since `before`, a line each, oldest kind first: the device, then what went
    /// wrong, then the transport.
    pub fn changes(&self, before: &Watch) -> Vec<(WatchKind, String)> {
        let mut out = Vec::new();
        self.device_changes(before, &mut out);
        self.gaps(before, &mut out);
        self.faults(before, &mut out);
        self.transport_changes(before, &mut out);
        out
    }

    fn device_changes(&self, b: &Watch, out: &mut Vec<(WatchKind, String)>) {
        let (a, p) = (&self.audio, &b.audio);
        let device = |a: &AudioDiagnostics| {
            format!(
                "{}: {} → {}, {} Hz, {} frames",
                a.host,
                a.input.as_deref().unwrap_or("no input"),
                a.output.as_deref().unwrap_or("no output"),
                a.sample_rate,
                a.buffer_size
            )
        };
        if a.running && !p.running {
            out.push((WatchKind::Device, format!("opened {}", device(a))));
        } else if !a.running && p.running {
            out.push((WatchKind::Device, "stopped".into()));
        } else if a.running && (a.input != p.input || a.output != p.output || a.sample_rate != p.sample_rate || a.buffer_size != p.buffer_size) {
            out.push((WatchKind::Device, format!("now {}", device(a))));
        }
        if a.error != p.error {
            if let Some(e) = &a.error {
                out.push((WatchKind::Device, format!("didn't open: {e}")));
            }
        }
        let events = grew(p.device_events as u64, a.device_events as u64);
        if events > 0 {
            let last = a.last_stream_error.as_deref().map(|e| format!(": {e}")).unwrap_or_default();
            out.push((WatchKind::Device, format!("device event{}{last}", times(events))));
        }
        let errors = grew(p.stream_errors as u64, a.stream_errors as u64);
        if errors > 0 {
            let last = a.last_stream_error.as_deref().map(|e| format!(": {e}")).unwrap_or_default();
            out.push((WatchKind::Device, format!("stream error{}{last}", times(errors))));
        }
        let restarts = grew(p.restarts as u64, a.restarts as u64);
        if restarts > 0 {
            out.push((WatchKind::Device, format!("restarted by itself{}", times(restarts))));
        }
        if a.gave_up_restarting && !p.gave_up_restarting {
            out.push((WatchKind::Device, "gave up restarting the device by itself".into()));
        }
    }

    fn gaps(&self, b: &Watch, out: &mut Vec<(WatchKind, String)>) {
        let (l, p) = (&self.load, &b.load);
        let when = self.when();
        let gaps = grew(b.audio.device_gaps as u64, self.audio.device_gaps as u64);
        if gaps > 0 {
            out.push((WatchKind::Gap, format!("device gap{} (a callback came after the last one's audio ran out), {when}", times(gaps))));
        }
        let late = grew(p.late_blocks as u64, l.late_blocks as u64);
        if late > 0 {
            out.push((
                WatchKind::Gap,
                format!(
                    "late buffer{} (load {:.0}%, recent peak {:.0}%, worst since launch {:.0}%), {when}",
                    times(late),
                    l.cpu * 100.0,
                    l.cpu_peak * 100.0,
                    l.max * 100.0
                ),
            ));
        }
        let overloads = grew(p.overloads as u64, l.overloads as u64);
        if overloads > 0 {
            let took = l.overload_ms.map(|ms| format!(": a buffer took {ms:.1} ms")).unwrap_or_default();
            let track = l.overload_track.as_deref().map(|t| format!(", \"{t}\" took longest")).unwrap_or_default();
            out.push((WatchKind::Gap, format!("overload{}{took}{track}, {when}", times(overloads))));
        }
        let errors = grew(p.engine_errors as u64, l.engine_errors as u64);
        if errors > 0 {
            out.push((WatchKind::Gap, format!("engine error{} (a block silenced after a panic), {when}", times(errors))));
        }
        let missed = grew(p.ahead_misses as u64, l.ahead_misses as u64);
        if missed > 0 {
            out.push((WatchKind::Gap, format!("block{} not rendered ahead in time, played live, {when}", times(missed))));
        }
        let bad = grew(p.bad_audio as u64, l.bad_audio as u64);
        if bad > 0 {
            out.push((WatchKind::Gap, format!("plug-in output silenced{} (NaN or a runaway level), {when}", times(bad))));
        }
    }

    fn faults(&self, b: &Watch, out: &mut Vec<(WatchKind, String)>) {
        let (l, p) = (&self.load, &b.load);
        let when = self.when();
        let dropped = grew(p.cmds_dropped, l.cmds_dropped);
        if dropped > 0 {
            out.push((WatchKind::Fault, format!("command{} to the engine dropped, {when}", times(dropped))));
        }
        let overruns = grew(p.overruns as u64, l.overruns as u64);
        if overruns > 0 {
            out.push((WatchKind::Fault, format!("recording overrun{} (the recorder couldn't keep up), {when}", times(overruns))));
        }
        let lost = grew(p.midi_rec_lost as u64, l.midi_rec_lost as u64);
        if lost > 0 {
            out.push((WatchKind::Fault, format!("recorded MIDI lost{}, {when}", times(lost))));
        }
        let pool = |t: &ThreadDiagnostics| t.pool.as_ref().map_or((0, 0), |p| (p.stuck as u64, p.refused as u64));
        let ((stuck, refused), (was_stuck, was_refused)) = (pool(&self.threads), pool(&b.threads));
        if stuck > was_stuck {
            out.push((WatchKind::Fault, format!("processing thread given up on in a plug-in ({stuck} now), {when}")));
        }
        if refused > was_refused {
            let why = self.threads.pool.as_ref().and_then(|p| p.refusal.as_deref()).map(|r| format!(": {r}")).unwrap_or_default();
            out.push((WatchKind::Fault, format!("processing thread refused real-time priority ({refused} now){why}")));
        }
        if self.threads.left_out_tracks != b.threads.left_out_tracks {
            out.push((WatchKind::Fault, format!("tracks sitting out: {}", self.threads.left_out_tracks)));
        }
    }

    fn transport_changes(&self, b: &Watch, out: &mut Vec<(WatchKind, String)>) {
        if self.recording && !b.recording {
            out.push((WatchKind::Transport, format!("recording from {}", self.at())));
        } else if self.playing && !b.playing {
            out.push((WatchKind::Transport, format!("playing from {}", self.at())));
        }
        if !self.playing && b.playing {
            out.push((WatchKind::Transport, format!("stopped at {}", self.at())));
        } else if !self.recording && b.recording {
            out.push((WatchKind::Transport, format!("recording stopped at {}", self.at())));
        }
    }
}
