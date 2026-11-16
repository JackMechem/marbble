//! Keeping the audio thread out of the plug-in while the main thread restarts it.
//!
//! CLAP lets a plug-in be deactivated and activated again only on the main thread, and only once
//! it has stopped processing; a plug-in asks for that (`request_restart`) when its latency
//! changes, say. The audio thread can't wait for the main thread, so, as VST3's bus changes are
//! (`vst3-host`'s `multi_out.rs`), each block takes the [`Park`] with one compare-and-swap. To
//! restart, the controller asks the processor to park: it fades to its dry input over a few
//! milliseconds, stops processing and leaves the park to the controller. If no block comes, the
//! controller takes the park itself once no block is under way. While parked, `process` passes
//! its input through and the extra outputs are silent; the other strips play on untouched; and
//! afterwards the plug-in fades back in once its new latency has passed.
//!
//! [`Fade`] is the crossfade both this and the side-chain gate (`Processor::close_gate`) use.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

/// How long a fade takes each way.
pub(super) const FADE_SECS: f64 = 0.005;

const FREE: u8 = 0;
const BUSY: u8 = 1;
const PARKED: u8 = 2;
/// `resume` when no fade in waits.
const NO_RESUME: u32 = u32::MAX;

/// What the processor and the controller share to take turns with the plug-in.
pub(super) struct Park {
    state: AtomicU8,
    /// The controller wants the processor to park.
    want: AtomicBool,
    /// The frames the next block waits before fading in, once the controller is done.
    resume: AtomicU32,
}

impl Default for Park {
    fn default() -> Park {
        Park { state: AtomicU8::new(FREE), want: AtomicBool::new(false), resume: AtomicU32::new(NO_RESUME) }
    }
}

impl Park {
    /// The processor's way in, around every call it makes into the plug-in: false while the
    /// controller has it.
    pub fn enter(&self) -> bool {
        self.state.compare_exchange(FREE, BUSY, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }

    pub fn leave(&self) {
        self.state.store(FREE, Ordering::Release);
    }

    /// The processor, inside, hands the plug-in to the controller: it has faded out and stopped
    /// processing.
    pub fn park_here(&self) {
        self.state.store(PARKED, Ordering::Release);
    }

    /// The controller wants the plug-in.
    pub fn wanted(&self) -> bool {
        self.want.load(Ordering::Relaxed)
    }

    /// A fade in waits, after this many frames.
    pub fn take_resume(&self) -> Option<u32> {
        let settle = self.resume.swap(NO_RESUME, Ordering::Relaxed);
        (settle != NO_RESUME).then_some(settle)
    }

    pub fn resuming(&self) -> bool {
        self.resume.load(Ordering::Relaxed) != NO_RESUME
    }

    /// The controller asks for the plug-in, and has it once the processor has parked; with
    /// `take`, also when no block is under way. Never waits. Returns whether it has it.
    pub fn take(&self, take: bool) -> bool {
        self.want.store(true, Ordering::Relaxed);
        if self.state.load(Ordering::Acquire) == PARKED {
            return true;
        }
        take && self.state.compare_exchange(FREE, PARKED, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }

    /// The processor takes back a plug-in it parked itself, which the controller hasn't taken up
    /// (for a render, during which no controller call can come). Whether it did.
    pub fn unpark(&self) -> bool {
        self.want.store(false, Ordering::Relaxed);
        self.state.compare_exchange(PARKED, FREE, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }

    /// The controller won't take the plug-in after all (it won't start again): it stays parked,
    /// and no more is asked of the processor.
    pub fn give_up(&self) {
        self.want.store(false, Ordering::Relaxed);
    }

    /// The controller gives the plug-in back, to fade in after `settle` frames.
    pub fn give_back(&self, settle: u32) {
        self.want.store(false, Ordering::Relaxed);
        self.resume.store(settle.min(NO_RESUME - 1), Ordering::Relaxed);
        self.state.store(FREE, Ordering::Release);
    }
}

/// A crossfade between the plug-in's output (wet) and what it was given (dry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fade {
    Wet,
    /// Frames left of the fade to dry.
    Falling(u32),
    Dry,
    /// Frames to wait before fading back, so a latent plug-in's first output has arrived, and
    /// frames left of the fade.
    Rising {
        settle: u32,
        left: u32,
    },
}

impl Fade {
    /// The wet share of the next frame, moving on by one. `len` is the fade's length.
    pub fn step(&mut self, len: u32) -> f32 {
        match self {
            Fade::Wet => 1.0,
            Fade::Dry => 0.0,
            Fade::Falling(left) => {
                *left = left.saturating_sub(1);
                let g = *left as f32 / len as f32;
                if *left == 0 {
                    *self = Fade::Dry;
                }
                g
            }
            Fade::Rising { settle, .. } if *settle > 0 => {
                *settle -= 1;
                0.0
            }
            Fade::Rising { left, .. } => {
                *left = left.saturating_sub(1);
                let g = 1.0 - *left as f32 / len as f32;
                if *left == 0 {
                    *self = Fade::Wet;
                }
                g
            }
        }
    }

    pub fn moving(&self) -> bool {
        matches!(self, Fade::Falling(_) | Fade::Rising { .. })
    }

    /// Starts fading out from where it is: from wet, the whole way; partway up, back down from
    /// there; still waiting to rise (dry), it's out at once. Out or on its way already, it goes on.
    pub fn fall(&mut self, len: u32) {
        *self = match *self {
            Fade::Wet => Fade::Falling(len),
            Fade::Rising { settle, .. } if settle > 0 => Fade::Dry,
            Fade::Rising { left, .. } => Fade::Falling(len.saturating_sub(left)),
            other => other,
        };
    }

    /// Starts fading back in: from dry, after `settle` frames; partway down, back up from there
    /// at once (the plug-in played on, so its output is there already). In or on its way
    /// already, it goes on.
    pub fn rise(&mut self, settle: u32, len: u32) {
        *self = match *self {
            Fade::Dry => Fade::Rising { settle, left: len },
            Fade::Falling(left) => Fade::Rising { settle: 0, left: len.saturating_sub(left) },
            other => other,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_falls_waits_and_rises() {
        let mut f = Fade::Wet;
        f.fall(4);
        let down: Vec<f32> = (0..5).map(|_| f.step(4)).collect();
        assert_eq!(down, [0.75, 0.5, 0.25, 0.0, 0.0]);
        assert_eq!(f, Fade::Dry);
        f.rise(2, 4);
        let up: Vec<f32> = (0..7).map(|_| f.step(4)).collect();
        assert_eq!(up, [0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0]);
        assert_eq!(f, Fade::Wet);
        f.rise(2, 4);
        assert_eq!(f, Fade::Wet, "in already");
    }

    /// The largest step from one frame's gain to the next over `frames` of `f`.
    fn largest_step(f: &mut Fade, len: u32, frames: usize, mut at: impl FnMut(usize, &mut Fade)) -> f32 {
        let mut last = None::<f32>;
        let mut step = 0.0f32;
        for i in 0..frames {
            at(i, f);
            let g = f.step(len);
            if let Some(l) = last {
                step = step.max((g - l).abs());
            }
            last = Some(g);
        }
        step
    }

    #[test]
    fn a_fall_during_a_rise_and_a_rise_during_a_fall_go_on_from_where_they_are() {
        let len = 100;
        let bound = 1.5 / len as f32;
        for turn in [10, 50, 90] {
            let mut f = Fade::Dry;
            f.rise(0, len);
            let step = largest_step(&mut f, len, 300, |i, f| {
                if i == turn {
                    f.fall(len)
                }
            });
            assert!(step <= bound, "a fall {turn} frames into a rise: step {step}");
            assert_eq!(f, Fade::Dry);
            let mut f = Fade::Wet;
            f.fall(len);
            let step = largest_step(&mut f, len, 300, |i, f| {
                if i == turn {
                    f.rise(20, len)
                }
            });
            assert!(step <= bound, "a rise {turn} frames into a fall: step {step}");
            assert_eq!(f, Fade::Wet);
        }
        // Still waiting to rise, it is dry: falling then is out at once, with no step.
        let mut f = Fade::Dry;
        f.rise(50, len);
        let step = largest_step(&mut f, len, 300, |i, f| {
            if i == 20 {
                f.fall(len)
            }
        });
        assert_eq!((step, f), (0.0, Fade::Dry));
    }

    #[test]
    fn the_controller_has_the_plug_in_only_when_no_block_does() {
        let p = Park::default();
        assert!(p.enter(), "a block");
        assert!(!p.take(true), "not while the block is under way");
        assert!(p.wanted());
        p.park_here();
        assert!(!p.enter(), "parked");
        assert!(p.take(false), "the processor parked: the controller has it");
        p.give_back(10);
        assert!(!p.wanted() && p.resuming());
        assert!(p.enter());
        assert_eq!(p.take_resume(), Some(10));
        assert_eq!(p.take_resume(), None);
        p.leave();
        assert!(!p.take(false), "asked: it waits for the processor");
        assert!(p.take(true), "or takes it, with no block under way");
        assert!(!p.enter());
        p.give_back(0);
        p.take(false);
        assert!(p.enter());
        p.park_here();
        assert!(p.unpark() && !p.wanted() && p.enter(), "a render takes back a plug-in it parked");
    }
}
