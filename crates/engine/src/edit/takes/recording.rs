//! Cutting a recording into takes: the plan a recording is set up with ([`RecPlan`]), the cycle
//! passes of recorded MIDI, and the colours takes are given.

/// Marks the end of a cycle pass among the MIDI events recorded (the engine sends it when the
/// cycle jumps back while recording). An undefined system real-time byte, so it can never be
/// mistaken for something played.
pub const PASS_MARK: [u8; 3] = [0xF9, 0, 0];

/// Most takes one recording is cut into (a guard against a degenerate cycle).
const MAX_TAKES: i64 = 999;

/// The colours Settings › Recording › Color takes gives a folder's takes in turn: every other
/// one of the track colours (`TRACK_COLORS` in src/tracks/viewport.ts), so neighbours differ.
pub const TAKE_COLORS: [&str; 10] = ["#e5483a", "#f2a63b", "#c9cf3a", "#4fae4c", "#35a8a0", "#3b8fd9", "#6a5be0", "#b14fc8", "#d9546e", "#8e8e93"];

/// Splits recorded MIDI events at the [`PASS_MARK`]s into one list per cycle pass, dropping
/// passes in which nothing was played.
pub fn midi_passes(events: &[(i64, [u8; 3])]) -> Vec<Vec<(i64, [u8; 3])>> {
    let mut out = vec![Vec::new()];
    for &(pos, d) in events {
        if d == PASS_MARK {
            out.push(Vec::new());
        } else {
            out.last_mut().unwrap().push((pos, d));
        }
    }
    out.retain(|p| !p.is_empty());
    out
}

/// How a recording was set up, so it can be cut into takes once it stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecPlan {
    /// Where the transport started.
    pub from: i64,
    /// The cycle, when recording in cycle mode.
    pub cycle: Option<(i64, i64)>,
    /// The Autopunch range.
    pub punch: Option<(i64, i64)>,
    /// Recording latency compensation (frames recordings move earlier).
    pub latency: i64,
    /// The same for recorded MIDI, which only travels one way.
    pub midi_latency: i64,
    /// A pass stopped part-way through is kept as a take if it got at least this far.
    pub min_take: i64,
}

impl RecPlan {
    /// For cycle recording: the take-time window `w0..w1` (the cycle, narrowed by the punch
    /// range), and where each pass stops capturing (`g1`, the punch-out plus the latency, so
    /// the compensated take still reaches the punch-out). None when the window is empty.
    pub fn cycle_window(&self) -> Option<(i64, i64, i64)> {
        let (cs, ce) = self.cycle?;
        let (w0, w1) = match self.punch {
            Some((a, b)) => (a.max(cs), b.min(ce)),
            None => (cs, ce),
        };
        if w1 <= w0 {
            return None;
        }
        let g1 = if self.punch.is_some() { (w1 + self.latency).min(ce) } else { ce };
        Some((w0, w1, g1))
    }

    /// Engine positions between which input is captured: from the record start (`playhead`
    /// outside cycle mode) to the end, per pass in cycle mode. None captures nothing.
    pub fn capture(&self, playhead: i64) -> Option<(i64, i64)> {
        if self.cycle.is_some() {
            let (w0, _, g1) = self.cycle_window()?;
            return Some((w0, if self.punch.is_some() { g1 } else { i64::MAX }));
        }
        match self.punch {
            Some((a, b)) => {
                let start = playhead.max(a);
                (start < b).then_some((start, b + self.latency))
            }
            None => Some((playhead, i64::MAX)),
        }
    }

    /// The takes of a cycle recording `frames` long: the folder's timeline start and length,
    /// then per take its clip offset and the take-time range it has audio for.
    ///
    /// Without a punch range the file is one continuous stream, the passes back to back. With
    /// one, each pass only captured `w0..g1`, so the passes are `g1 - w0` frames apart. Either
    /// way the latency puts the audio heard at `w0` `latency` frames into its pass.
    #[allow(clippy::type_complexity)] // The doc comment names the parts; there is one caller.
    pub fn cycle_takes(&self, frames: i64) -> Option<(i64, i64, Vec<(i64, i64, i64)>)> {
        let (cs, ce) = self.cycle?;
        let (w0, w1, g1) = self.cycle_window()?;
        let stride = g1 - w0;
        let continuous = w0 == cs && g1 == ce;
        let len = if continuous { w1 - w0 } else { (w1 - w0).min(stride - self.latency) };
        if len <= 0 {
            return None;
        }
        // Recording that starts inside the window begins part-way into the first pass.
        let first = if self.from > w0 && self.from < g1 { self.from } else { w0 };
        let base = self.latency + (w0 - first);
        let mut takes = Vec::new();
        for k in 0..MAX_TAKES {
            let offset = base + k * stride;
            if offset >= frames {
                break;
            }
            let (start, end) = ((-offset).max(0), len.min(frames - offset));
            if end - start >= self.min_take.min(len).max(1) {
                takes.push((offset, start, end));
            }
        }
        Some((w0, len, takes))
    }

    /// Outside cycle mode, with Autopunch on: where the take lands, trimmed to the punch range:
    /// timeline start, length and clip offset. `start` is where capture began.
    pub fn punch_span(&self, start: i64, frames: i64) -> Option<(i64, i64, i64)> {
        let (pin, pout) = self.punch?;
        let heard = start - self.latency;
        let a = heard.max(pin).max(0);
        let b = (heard + frames).min(pout);
        (a < b).then_some((a, b - a, a - heard))
    }
}
