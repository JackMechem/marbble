//! What one block of processing shares: the per-thread [`Scratch`] buffers, the read-only
//! [`BlockCtx`], how a strip reaches other strips ([`Routes`]: directly in the serial loop, deferred
//! on worker threads, see `parallel/`), and the small mixing functions they use.
//!
//! `strip.rs` plays a strip with these, `process.rs` sets them up and `parallel/` runs them
//! on the pool.

use super::*;

/// Space a strip works in, one per thread: a send reading the delay line at its own tap, the
/// fader gains of the block, which post-fader sends at another tap apply again, and the side-chain
/// key a plug-in is reading.
pub(crate) struct Scratch {
    pub(super) tap_l: Vec<f32>,
    pub(super) tap_r: Vec<f32>,
    pub(super) gain_l: Vec<f32>,
    pub(super) gain_r: Vec<f32>,
    pub(super) key_l: Vec<f32>,
    pub(super) key_r: Vec<f32>,
}

impl Default for Scratch {
    fn default() -> Self {
        let buf = || vec![0.0; MAX_BLOCK];
        Scratch { tap_l: buf(), tap_r: buf(), gain_l: buf(), gain_r: buf(), key_l: buf(), key_r: buf() }
    }
}

/// What every strip knows about the block. Nothing in here changes while the strips run.
#[derive(Clone, Copy)]
pub(crate) struct BlockCtx<'a> {
    pub(super) n: usize,
    pub(super) pos: i64,
    pub(super) playing: bool,
    pub(super) recording: bool,
    pub(super) sr: f64,
    pub(super) sources: bool,
    pub(super) punch_range: Option<(i64, i64)>,
    pub(super) capture_from: i64,
    pub(super) capture_end: i64,
    pub(super) auto_monitor: bool,
    pub(super) low_latency: bool,
    pub(super) block_end: i64,
    pub(super) smooth: f32,
    pub(super) any_solo: bool,
    pub(super) snap_gains: bool,
    pub(super) iso: Option<&'a Isolate>,
    pub(super) transport: TransportInfo,
    pub(super) tempo: &'a TempoMap,
    pub(super) status: &'a EngineStatus,
    pub(super) inputs: &'a [Vec<f32>],
    pub(super) live: &'a [MidiEvent],
    pub(super) midi_target: Option<Id>,
    pub(super) midi_thru: bool,
    pub(super) chase: (bool, bool),
    pub(super) analyzer_target: Option<(Id, Id)>,
    /// Wait for every strip however long it takes, as an offline render can.
    pub(super) wait: bool,
    /// Render ahead's threads, while it's on (see `ahead/`).
    pub(super) ahead: Option<&'a ahead::AheadShared>,
    /// Until when lent strips wait for their blocks: one deadline for the whole callback.
    pub(super) ahead_until: Option<std::time::Instant>,
}

/// Where a strip's output goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Dest {
    /// Not heard: a key-only track in an isolated render, or a strip that didn't run.
    #[default]
    Nowhere,
    /// Stereo Out's bus.
    Bus,
    /// The input of the track at this index.
    Track(usize),
}

/// A send on its way: its slot, the index of the track it feeds, and which side of the fader it
/// taps (that decides its turn among what the strip writes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SendOut {
    pub(super) slot: usize,
    pub(super) to: usize,
    pub(super) pre: bool,
}

/// What a strip played this block on the pool, for the strips that wait for it and for the summing
/// of Stereo Out: its output stays in `TrackDsp::l` and `r`, and this holds the rest.
pub(crate) struct Outlets {
    pub(super) dest: Dest,
    pub(super) sends: [Option<SendOut>; SEND_SLOTS],
    /// Each send's signal with its gain applied, left and right.
    pub(super) buf: Vec<f32>,
    /// The spectrum analyzer's feed, when the analyzer looks at this strip.
    pub(super) analysis: Vec<f32>,
    pub(super) analyzed: bool,
}

impl Outlets {
    pub(super) fn new() -> Outlets {
        // Only pages a send really uses are ever touched.
        Outlets { dest: Dest::Nowhere, sends: [None; SEND_SLOTS], buf: vec![0.0; SEND_SLOTS * 2 * MAX_BLOCK], analysis: vec![0.0; MAX_BLOCK], analyzed: false }
    }

    pub(super) fn reset(&mut self) {
        self.dest = Dest::Nowhere;
        self.sends = [None; SEND_SLOTS];
        self.analyzed = false;
    }

    pub(super) fn dest(&self) -> Dest {
        self.dest
    }

    pub(super) fn analysis(&self, n: usize) -> Option<&[f32]> {
        self.analyzed.then(|| &self.analysis[..n])
    }

    pub(super) fn send_buf(&self, slot: usize, n: usize) -> (&[f32], &[f32]) {
        let at = slot * 2 * MAX_BLOCK;
        (&self.buf[at..at + n], &self.buf[at + MAX_BLOCK..at + MAX_BLOCK + n])
    }

    pub(super) fn send_buf_mut(&mut self, slot: usize, n: usize) -> (&mut [f32], &mut [f32]) {
        let at = slot * 2 * MAX_BLOCK;
        let (a, b) = self.buf[at..at + 2 * MAX_BLOCK].split_at_mut(MAX_BLOCK);
        (&mut a[..n], &mut b[..n])
    }
}

/// How a strip reaches the other strips, which is all that differs between the serial loop and the
/// pool: the loop's [`Direct`] writes straight into them, and the pool's `parallel::Deferred` leaves
/// everything in the strip's [`Outlets`] for them to read. The arithmetic is the same.
pub(crate) trait Routes {
    /// The track at `idx`, played earlier in the block.
    fn earlier(&self, idx: usize) -> Option<&TrackDsp>;
    /// The id of a track later in the list.
    fn id_of(&self, j: usize) -> Id;
    /// What the folder at `j` does to this strip's fader at timeline position `at`.
    fn fold(&self, read_ahead: auto::FoldState, j: usize, at: i64, playing: bool) -> auto::FoldState;
    /// A bus's input for the block into `l` and `r`, which hold silence.
    fn inputs(&mut self, sum: (&mut [f32], &mut [f32]), l: &mut [f32], r: &mut [f32]);
    /// A send of `from`, with its gain ramping to `target`.
    fn send(&mut self, outs: &mut Outlets, send: SendOut, from: (&[f32], &[f32]), gain: &mut f32, target: f32, smooth: f32);
    /// The strip's output.
    fn output(&mut self, outs: &mut Outlets, dest: Dest, l: &[f32], r: &[f32]);
    /// The analyzer's feed, when it looks at this strip (None: silence, from an Off track).
    fn analyze(&mut self, outs: &mut Outlets, signal: Option<(&[f32], &[f32])>, n: usize);
    /// Around a call into the strip's plug-in `uid`, which may never return. False after it if the
    /// strip was left in it (`pool::Call`): it's no longer this thread's, which must stop at once.
    /// The serial loop has nobody to leave a strip to.
    fn enter(&self, _uid: Id) {}
    fn leave(&self) -> bool {
        true
    }
}

/// The serial loop's routes.
pub(super) struct Direct<'a> {
    pub(super) before: &'a [Box<TrackDsp>],
    pub(super) tail: &'a mut [Box<TrackDsp>],
    pub(super) ti: usize,
    pub(super) bus: (&'a mut [f32], &'a mut [f32]),
    pub(super) analyzer: &'a mut rtrb::Producer<f32>,
}

impl Routes for Direct<'_> {
    fn earlier(&self, idx: usize) -> Option<&TrackDsp> {
        self.before.get(idx).map(|t| &**t)
    }

    fn id_of(&self, j: usize) -> Id {
        self.tail[j - self.ti - 1].id
    }

    fn fold(&self, _read_ahead: auto::FoldState, j: usize, at: i64, playing: bool) -> auto::FoldState {
        auto::fold_at(self.tail, self.ti + 1, j, at, playing)
    }

    fn inputs(&mut self, sum: (&mut [f32], &mut [f32]), l: &mut [f32], r: &mut [f32]) {
        l.copy_from_slice(sum.0);
        r.copy_from_slice(sum.1);
        sum.0.fill(0.0);
        sum.1.fill(0.0);
    }

    fn send(&mut self, _outs: &mut Outlets, send: SendOut, from: (&[f32], &[f32]), gain: &mut f32, target: f32, smooth: f32) {
        mix_send(&mut self.tail[send.to - self.ti - 1], from.0, from.1, gain, target, smooth);
    }

    fn output(&mut self, _outs: &mut Outlets, dest: Dest, l: &[f32], r: &[f32]) {
        let (dl, dr) = match dest {
            Dest::Nowhere => return,
            Dest::Bus => (&mut *self.bus.0, &mut *self.bus.1),
            Dest::Track(j) => {
                let o = &mut *self.tail[j - self.ti - 1];
                (&mut o.sum_l[..l.len()], &mut o.sum_r[..r.len()])
            }
        };
        for i in 0..l.len() {
            dl[i] += l[i];
            dr[i] += r[i];
        }
    }

    fn analyze(&mut self, _outs: &mut Outlets, signal: Option<(&[f32], &[f32])>, n: usize) {
        for i in 0..n {
            let _ = self.analyzer.push(signal.map_or(0.0, |(l, r)| 0.5 * (l[i] + r[i])));
        }
    }
}

pub(super) fn db_to_gain(db: f32) -> f32 {
    if db <= -96.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

/// The gain of a fader at `db`, with -96 dB and below silent.
pub fn volume_gain(db: f32) -> f32 {
    db_to_gain(db)
}

/// `t` moved to timeline position `pos` (also for plug-ins on a strip whose audio lags the
/// playhead): the tempo, signature and musical position there.
pub(super) fn transport_at(map: &TempoMap, t: &TransportInfo, pos: i64) -> TransportInfo {
    let at = pos as f64;
    let bar = map.bar_at(at);
    let (num, den) = map.sig_at_bar(bar);
    TransportInfo {
        tempo: map.bpm_at(at),
        sig_num: num as i32,
        sig_den: den as i32,
        position_samples: pos,
        position_quarters: map.beats_at(at),
        bar_start_quarters: map.bar_start_q(bar),
        ..*t
    }
}

/// Mixes the audio regions playing in this block into `l`/`r`, except inside `quiet` (timeline
/// samples, start inclusive), where an Autopunch recording is replacing them.
pub(super) fn play_audio(regions: &[RegionDsp], pos: i64, l: &mut [f32], r: &mut [f32], quiet: (i64, i64)) {
    let block_end = pos + l.len() as i64;
    for reg in regions {
        let RegionDsp::Audio { start, length, offset, gain, clip, fade_in, fade_out, laws } = reg else { continue };
        let (rs, re) = (*start, *start + *length);
        if re <= pos || rs >= block_end {
            continue;
        }
        let from = rs.max(pos);
        let to = re.min(block_end);
        let ch = &clip.channels;
        let frames = clip.frames() as i64;
        for tl in from..to {
            if tl >= quiet.0 && tl < quiet.1 {
                continue;
            }
            let src = tl - rs + offset;
            // Fades: `x` runs from 0 (silent) to 1 (full level). A speed fade also moves where
            // the audio is read from, by `lag` frames.
            let mut g = *gain;
            let mut lag = 0.0f64;
            if tl < fade_in.0 + fade_in.1 && fade_in.1 > 0 {
                let x = (tl - fade_in.0) as f32 / fade_in.1 as f32;
                g *= laws[0].gain(x);
                lag += laws[0].lag(x) as f64 * fade_in.1 as f64;
            }
            if tl >= fade_out.0 && fade_out.1 > 0 {
                let x = (fade_out.0 + fade_out.1 - tl) as f32 / fade_out.1 as f32;
                g *= laws[1].gain(x);
                lag -= laws[1].lag(x) as f64 * fade_out.1 as f64;
            }
            let i = (tl - pos) as usize;
            if lag == 0.0 {
                if src < 0 || src >= frames {
                    continue;
                }
                let a = ch[0][src as usize] * g;
                let b = if ch.len() > 1 { ch[1][src as usize] * g } else { a };
                l[i] += a;
                r[i] += b;
            } else {
                // Between two frames: interpolate.
                let at = src as f64 + lag;
                let k = at.floor();
                let f = (at - k) as f32;
                let k = k as i64;
                let read = |c: &[f32], j: i64| if j >= 0 && j < frames { c[j as usize] } else { 0.0 };
                let tap = |c: &[f32]| read(c, k) + (read(c, k + 1) - read(c, k)) * f;
                let a = tap(&ch[0]) * g;
                let b = if ch.len() > 1 { tap(&ch[1]) * g } else { a };
                l[i] += a;
                r[i] += b;
            }
        }
    }
}

/// Mixes audio regions into `l`/`r` for the block starting at `pos`, exactly as playback does
/// (for renders that mix regions outside the engine, such as Join).
pub fn mix_regions(regions: &[RegionDsp], pos: i64, l: &mut [f32], r: &mut [f32]) {
    play_audio(regions, pos, l, r, (0, 0));
}

/// The side-chain signal for an insert keyed by the track at `idx` (played earlier in
/// the block): its post-fader output (or with `pre`, its signal before the fader), or None when
/// there is none (no source, or one an isolated render leaves out).
pub(super) fn key_signal<'a>(src: Option<&'a TrackDsp>, pre: bool, iso: Option<&Isolate>, n: usize) -> crate::fx::Key<'a> {
    let src = src?;
    if !(if pre { src.keyed_pre } else { src.keyed }) || src.kind == TrackKind::Folder || iso.map(|i| i.role(src.id) == Role::Skip).unwrap_or(false) {
        return None;
    }
    if pre {
        return Some((&src.pre_key_l[..n], &src.pre_key_r[..n]));
    }
    Some((&src.key_l[..n], &src.key_r[..n]))
}

/// `mix_send` for the pool: `gain * (l, r)` into `out`, for the receiver to add. False if the send
/// is silent and nothing was written.
pub(super) fn render_send(out: (&mut [f32], &mut [f32]), from: (&[f32], &[f32]), gain: &mut f32, target: f32, smooth: f32) -> bool {
    let mut g = *gain;
    if g == 0.0 && target == 0.0 {
        return false;
    }
    let (dl, dr) = out;
    let (l, r) = from;
    for i in 0..l.len() {
        g += (target - g) * smooth;
        dl[i] = l[i] * g;
        dr[i] = r[i] * g;
    }
    *gain = if target == 0.0 && g.abs() < 1e-6 { 0.0 } else { g };
    true
}

/// Adds `gain * (l, r)` into an aux's (or summing stack's) input, ramping the gain to `target`.
fn mix_send(dst: &mut TrackDsp, l: &[f32], r: &[f32], gain: &mut f32, target: f32, smooth: f32) {
    let mut g = *gain;
    if g == 0.0 && target == 0.0 {
        return;
    }
    let (dl, dr) = (&mut dst.sum_l[..l.len()], &mut dst.sum_r[..r.len()]);
    for i in 0..l.len() {
        g += (target - g) * smooth;
        dl[i] += l[i] * g;
        dr[i] += r[i] * g;
    }
    // Settle exactly on silence so idle sends cost nothing.
    *gain = if target == 0.0 && g.abs() < 1e-6 { 0.0 } else { g };
}
