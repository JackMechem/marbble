//! Linear Phase EQ, after Logic's: the Channel EQ's eight bands, with linear phase.
//!
//! - The bands are the Channel EQ's (`crate::fx::channel_eq::filters`): their combined magnitude response is
//!   realized as a linear-phase FIR (see `fir.rs`), so every frequency is delayed equally and
//!   nothing smears in time, at the cost of latency, which delay compensation makes up.
//! - **Quality** picks the kernel length: Low, Medium and High are 2047, 4095 and 8191 taps at
//!   44.1 or 48 kHz (twice that at 88.2 or 96 kHz, four times at 176.4 or 192). Longer kernels
//!   resolve low frequencies better. The latency is half the kernel: 1023, 2047 or 4095 frames
//!   at 48 kHz (21, 43 or 85 ms).
//! - Designing a kernel takes a few milliseconds, too long for the audio thread, so each
//!   instance has a worker thread. Parameter changes (from the panel or automation) reach it
//!   through atomics; it designs at most every [`DESIGN_INTERVAL`] and hands the kernel over
//!   through a lock-free queue, and the convolver crossfades to it over about
//!   [`FADE_SECS`]. A fast automation sweep is therefore followed in steps of about 25 ms,
//!   each faded into the next.
//! - Offline renders (bounces, freezes) may block, so a render designs each kernel it needs
//!   itself (unless the worker has it ready) and uses it at once: changes land at the same point
//!   of every render, and the result doesn't depend on the speed of the machine or on when the
//!   worker gets to run.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::fir::{linear_phase_taps, plan_forward, Convolver, Kernel};
use super::*;
use crate::model::ChannelEq;

pub use super::channel_eq::{channel_eq, freq, gain, is_cut, on, q, slope, BANDS, OUTPUT};
/// The kernel length: 0 Low, 1 Medium, 2 High.
pub const QUALITY: u32 = 65;
/// One past the highest parameter id.
const ID_SPACE: usize = 66;

/// The fastest the worker designs new kernels while parameters keep changing.
pub const DESIGN_INTERVAL: Duration = Duration::from_millis(25);
/// How long the old and new kernels crossfade.
pub const FADE_SECS: f32 = 0.02;
/// How often an idle worker looks for changes.
const POLL: Duration = Duration::from_millis(10);

const QUALITIES: &[&str] = &["Low", "Medium", "High"];

/// The Channel EQ's parameters (`channel_eq::BAND_PARAMS`), then the Quality.
static PARAMS: [ParamSpec; 34] = {
    let quality =
        ParamSpec { id: QUALITY, name: "Quality", min: 0.0, max: 2.0, default: 1.0, unit: Unit::None, kind: ParamKind::Choice(QUALITIES), log: false };
    let mut out = [quality; 34];
    let mut i = 0;
    while i < 33 {
        out[i] = super::channel_eq::BAND_PARAMS[i];
        i += 1;
    }
    out
};

#[rustfmt::skip] // One row per preset's values: split per value, these tables run to thousands of lines.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Flat", values: &[] },
    FactoryPreset { name: "Gentle Mastering Lift", values: &[(gain(1), 1.0), (freq(4), 3000.0), (gain(4), 1.0), (q(4), 0.8), (freq(6), 12000.0), (gain(6), 1.5)] },
    FactoryPreset { name: "Low End Cleanup", values: &[(on(0), 1.0), (freq(0), 35.0), (slope(0), 2.0), (freq(2), 250.0), (gain(2), -2.0), (q(2), 1.2)] },
    FactoryPreset { name: "Mid Scoop", values: &[(gain(3), -3.0), (q(3), 0.9)] },
    FactoryPreset { name: "Vocal Presence", values: &[(on(0), 1.0), (freq(0), 90.0), (freq(3), 300.0), (gain(3), -1.5), (freq(4), 3500.0), (gain(4), 2.5), (q(4), 1.0)] },
    FactoryPreset { name: "Air", values: &[(freq(6), 12000.0), (gain(6), 3.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:linear-phase-eq",
        name: "Linear Phase EQ",
        category: "EQ",
        params: &PARAMS,
        meters: &[],
        sidechain: false,
        new: |sr| Box::new(LinearPhaseEq::new(sr)),
    }
}

/// The kernel length for a quality (0..=2) at a sample rate: odd, so the centre is a sample.
pub fn taps(quality: usize, sample_rate: f32) -> usize {
    let base = [2048, 4096, 8192][quality.min(2)];
    let scale = ((sample_rate / 48_000.0).round() as usize).clamp(1, 4).next_power_of_two().min(4);
    base * scale - 1
}

fn quality_of(values: &[f32; ID_SPACE]) -> usize {
    values[QUALITY as usize].round().clamp(0.0, 2.0) as usize
}

/// Designs the kernel for a set of parameter values. Allocates and takes a few
/// milliseconds: never on the audio thread.
fn design(values: &[f32; ID_SPACE], sample_rate: f32, gen: u64, fft: &dyn realfft::RealToComplex<f32>) -> Box<Kernel> {
    let eq = channel_eq(|id| values[id as usize]);
    let len = taps(quality_of(values), sample_rate);
    let bands_flat = ChannelEq { output_db: 0.0, ..eq.clone() }.is_flat();
    let taps = if bands_flat {
        // Only the output level: an exact delay, which also costs nearly nothing to run.
        let mut t = vec![0.0f32; len];
        t[(len - 1) / 2] = util::db_to_gain(eq.output_db);
        t
    } else {
        let c = crate::fx::channel_eq::filters::coefficients(&eq, sample_rate as f64);
        linear_phase_taps(len, |w| c.magnitude(w))
    };
    Kernel::new(&taps, gen, fft)
}

/// What the effect shares with its worker.
struct Shared {
    values: [AtomicU32; ID_SPACE],
    /// Bumped after every parameter change.
    gen: AtomicU64,
    /// The effect is rendering offline, and designs the kernels it needs itself.
    offline: AtomicBool,
    stop: AtomicBool,
}

/// The effect's end of the worker: kernels come in, used ones go back to be freed.
struct Link {
    shared: Arc<Shared>,
    kernels: rtrb::Consumer<Box<Kernel>>,
    spent: rtrb::Producer<Box<Kernel>>,
    worker: std::thread::Thread,
}

/// The worker: designs a kernel whenever the parameters changed, no more often than
/// `DESIGN_INTERVAL` and not during an offline render, and frees kernels the effect is done with.
fn run_worker(shared: Arc<Shared>, mut out: rtrb::Producer<Box<Kernel>>, mut spent: rtrb::Consumer<Box<Kernel>>, sample_rate: f32, mut done: u64) {
    let fft = plan_forward();
    let mut last: Option<Instant> = None;
    while !shared.stop.load(Ordering::Acquire) {
        while spent.pop().is_ok() {}
        let gen = shared.gen.load(Ordering::Acquire);
        let offline = shared.offline.load(Ordering::Acquire);
        let since = last.map(|t| t.elapsed()).unwrap_or(DESIGN_INTERVAL);
        if gen != done && !offline && since >= DESIGN_INTERVAL && out.slots() > 0 {
            // The values are at least as new as `gen` (they're stored before it).
            let values: [f32; ID_SPACE] = std::array::from_fn(|i| f32::from_bits(shared.values[i].load(Ordering::Relaxed)));
            let k = design(&values, sample_rate, gen, fft.as_ref());
            if out.push(k).is_ok() {
                done = gen;
            }
            last = Some(Instant::now());
            continue;
        }
        let wait = if gen != done { DESIGN_INTERVAL.saturating_sub(since).max(Duration::from_millis(1)) } else { POLL };
        std::thread::park_timeout(wait);
    }
}

/// Hands a kernel back to the worker to free. Never frees on the audio thread: if the queue
/// is full (the worker has stopped), the kernel is leaked instead.
fn dispose(link: &mut Option<Link>, k: Box<Kernel>) {
    match link {
        Some(l) => {
            if let Err(rtrb::PushError::Full(k)) = l.spent.push(k) {
                std::mem::forget(k);
            }
        }
        None => std::mem::forget(k),
    }
}

pub struct LinearPhaseEq {
    sample_rate: f32,
    values: [f32; ID_SPACE],
    /// Counts parameter changes; a kernel carries the count it was designed for.
    gen: u64,
    conv: Convolver,
    link: Option<Link>,
    /// Rendering offline: wait for kernels instead of fading to them when they're ready.
    offline: bool,
    /// Reset (or new) and not yet run: a new kernel replaces the old one without a fade.
    fresh: bool,
    /// Frames since an offline render last waited for a kernel (as good as `design_frames` after
    /// a reset), and the least it waits between two (`DESIGN_INTERVAL` in frames, so renders
    /// follow automation as playback does).
    since_design: usize,
    design_frames: usize,
    /// The FFT plan renders design kernels with, made by the first that needs one.
    offline_fft: Option<Arc<dyn realfft::RealToComplex<f32>>>,
}

impl LinearPhaseEq {
    pub fn new(sample_rate: f32) -> LinearPhaseEq {
        let mut values = [0.0f32; ID_SPACE];
        for p in PARAMS.iter() {
            values[p.id as usize] = p.default;
        }
        let fft = plan_forward();
        let kernel = design(&values, sample_rate, 0, fft.as_ref());
        let conv = Convolver::new(taps(2, sample_rate), kernel, (FADE_SECS * sample_rate) as usize);
        let shared = Arc::new(Shared {
            values: std::array::from_fn(|i| AtomicU32::new(values[i].to_bits())),
            gen: AtomicU64::new(0),
            offline: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });
        let (k_tx, k_rx) = rtrb::RingBuffer::new(2);
        let (s_tx, s_rx) = rtrb::RingBuffer::new(8);
        let worker_shared = shared.clone();
        let link = std::thread::Builder::new()
            .name("linear-phase-eq".into())
            .spawn(move || run_worker(worker_shared, k_tx, s_rx, sample_rate, 0))
            .map(|h| Link { shared, kernels: k_rx, spent: s_tx, worker: h.thread().clone() })
            .map_err(|e| log::error!("Linear Phase EQ: no worker thread, so its settings stay as they are: {e}"))
            .ok();
        LinearPhaseEq {
            sample_rate,
            values,
            gen: 0,
            conv,
            link,
            offline: false,
            fresh: true,
            since_design: 0,
            design_frames: (DESIGN_INTERVAL.as_secs_f32() * sample_rate) as usize,
            offline_fft: None,
        }
    }

    /// Puts a kernel to use: at once after a reset, else with a crossfade.
    fn adopt(&mut self, k: Box<Kernel>) {
        let old = if self.fresh { Some(self.conv.replace(k)) } else { self.conv.queue(k) };
        if let Some(old) = old {
            dispose(&mut self.link, old);
        }
    }

    /// Takes the newest kernel the worker has finished (playing live).
    fn collect(&mut self) {
        let Some(link) = &mut self.link else { return };
        let mut newest: Option<Box<Kernel>> = None;
        while let Ok(k) = link.kernels.pop() {
            if let Some(old) = newest.replace(k) {
                if let Err(rtrb::PushError::Full(k)) = link.spent.push(old) {
                    std::mem::forget(k);
                }
            }
        }
        if let Some(k) = newest {
            self.adopt(k);
        }
    }

    /// Puts the kernel of the current settings to use (offline only: this blocks and allocates).
    /// The worker's serves if it has one ready; otherwise the render designs it here rather than
    /// wait for the worker, which a loaded machine may not run for a long while: what a render
    /// plays mustn't depend on that.
    fn wait_for_kernel(&mut self) {
        let want = self.gen;
        let mut ready: Option<Box<Kernel>> = None;
        if let Some(link) = &mut self.link {
            while let Ok(k) = link.kernels.pop() {
                ready = Some(k);
            }
        }
        let k = match ready {
            Some(k) if k.gen == want => k,
            _ => {
                let fft = self.offline_fft.get_or_insert_with(plan_forward);
                design(&self.values, self.sample_rate, want, fft.as_ref())
            }
        };
        self.adopt(k);
        self.since_design = 0;
    }

    fn dispose_retired(&mut self) {
        let link = &mut self.link;
        for k in self.conv.retired() {
            dispose(link, k);
        }
    }

    /// The settings changed since the newest kernel was designed.
    fn stale(&self) -> bool {
        self.conv.newest_gen() != self.gen
    }
}

impl Drop for LinearPhaseEq {
    fn drop(&mut self) {
        if let Some(l) = &self.link {
            l.shared.stop.store(true, Ordering::Release);
            l.worker.unpark();
        }
    }
}

impl Effect for LinearPhaseEq {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(v) = self.values.get_mut(id as usize) else { return };
        if *v == value {
            return;
        }
        *v = value;
        self.gen += 1;
        if let Some(l) = &self.link {
            l.shared.values[id as usize].store(value.to_bits(), Ordering::Relaxed);
            l.shared.gen.store(self.gen, Ordering::Release);
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, _ctx: &FxContext) {
        if self.offline {
            if self.stale() && (self.fresh || self.since_design >= self.design_frames) {
                self.wait_for_kernel();
            }
        } else {
            self.collect();
        }
        self.fresh = false;
        self.conv.process(l, r);
        self.since_design += l.len();
        self.dispose_retired();
    }

    fn reset(&mut self) {
        self.conv.reset();
        self.fresh = true;
        if self.offline && self.stale() {
            self.wait_for_kernel();
        }
        // Whatever played before (live, or an earlier render), the next change may be waited
        // for at once: a render that starts here follows its automation the same every time.
        self.since_design = self.design_frames;
        self.dispose_retired();
    }

    fn latency(&self) -> usize {
        (taps(quality_of(&self.values), self.sample_rate) - 1) / 2
    }

    fn tail_secs(&self) -> f32 {
        (taps(quality_of(&self.values), self.sample_rate) / 2) as f32 / self.sample_rate
    }

    fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
        if let Some(l) = &self.link {
            l.shared.offline.store(offline, Ordering::Release);
            l.worker.unpark();
        }
    }
}

#[cfg(test)]
mod tests;
