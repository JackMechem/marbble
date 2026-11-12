//! A strip's rendered-ahead audio: blocks of its source's output, each marked with where on the
//! timeline it plays, from the render thread holding the strip (the one producer) to whichever
//! thread plays the strip's mix (the one consumer). Allocated up front, off the audio thread; the
//! audio path only copies and counts.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};

/// One rendered block: where it plays and how long it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark {
    pub pos: i64,
    pub n: usize,
}

/// See the module docs. Frames and blocks are counted up forever; a ring position is a count
/// modulo the capacity.
pub(crate) struct FrameRing {
    l: Box<[UnsafeCell<f32>]>,
    r: Box<[UnsafeCell<f32>]>,
    marks: Box<[UnsafeCell<Mark>]>,
    frames_in: AtomicU64,
    frames_out: AtomicU64,
    marks_in: AtomicU64,
    marks_out: AtomicU64,
}

// SAFETY: the producer writes only past `frames_in`/`marks_in` and publishes with a release store;
// the consumer reads only below them (acquired) and past `frames_out`/`marks_out`, which it alone
// moves. So no cell is written and read at once.
unsafe impl Sync for FrameRing {}
unsafe impl Send for FrameRing {}

impl FrameRing {
    /// Room for `frames` frames in up to `blocks` blocks. Allocates: off the audio thread.
    pub(crate) fn new(frames: usize, blocks: usize) -> FrameRing {
        let cells = |n: usize| (0..n).map(|_| UnsafeCell::new(0.0f32)).collect::<Box<[_]>>();
        FrameRing {
            l: cells(frames.max(1)),
            r: cells(frames.max(1)),
            marks: (0..blocks.max(1)).map(|_| UnsafeCell::new(Mark { pos: 0, n: 0 })).collect(),
            frames_in: AtomicU64::new(0),
            frames_out: AtomicU64::new(0),
            marks_in: AtomicU64::new(0),
            marks_out: AtomicU64::new(0),
        }
    }

    /// The frames it can hold.
    pub(crate) fn capacity(&self) -> usize {
        self.l.len()
    }

    /// Frames rendered and not yet played.
    pub(crate) fn buffered(&self) -> usize {
        (self.frames_in.load(Ordering::Acquire) - self.frames_out.load(Ordering::Acquire)) as usize
    }

    /// For the producer: adds a block played at `pos`. False, adding nothing, when it doesn't fit.
    pub(crate) fn push(&self, pos: i64, l: &[f32], r: &[f32]) -> bool {
        let n = l.len().min(r.len());
        let (fi, fo) = (self.frames_in.load(Ordering::Relaxed), self.frames_out.load(Ordering::Acquire));
        let (mi, mo) = (self.marks_in.load(Ordering::Relaxed), self.marks_out.load(Ordering::Acquire));
        if (fi - fo) as usize + n > self.capacity() || (mi - mo) as usize >= self.marks.len() {
            return false;
        }
        let cap = self.capacity();
        for i in 0..n {
            let at = (fi as usize + i) % cap;
            // SAFETY: past `frames_in`, so the consumer isn't reading it (see the Sync impl).
            unsafe {
                *self.l[at].get() = l[i];
                *self.r[at].get() = r[i];
            }
        }
        // SAFETY: as above, for the mark.
        unsafe { *self.marks[mi as usize % self.marks.len()].get() = Mark { pos, n } };
        self.frames_in.store(fi + n as u64, Ordering::Release);
        self.marks_in.store(mi + 1, Ordering::Release);
        true
    }

    /// For the consumer: the oldest block's mark, if there's one.
    pub(crate) fn head(&self) -> Option<Mark> {
        let (mi, mo) = (self.marks_in.load(Ordering::Acquire), self.marks_out.load(Ordering::Relaxed));
        // SAFETY: below `marks_in`, published by the producer's release store.
        (mi > mo).then(|| unsafe { *self.marks[mo as usize % self.marks.len()].get() })
    }

    /// For the consumer: copies the oldest block into `l` and `r` (as long as it is) and lets it
    /// go. False if there's none.
    pub(crate) fn pop(&self, l: &mut [f32], r: &mut [f32]) -> bool {
        let Some(m) = self.head() else { return false };
        let fo = self.frames_out.load(Ordering::Relaxed);
        let cap = self.capacity();
        for i in 0..m.n.min(l.len()).min(r.len()) {
            let at = (fo as usize + i) % cap;
            // SAFETY: below `frames_in` (the mark was published after its frames).
            unsafe {
                l[i] = *self.l[at].get();
                r[i] = *self.r[at].get();
            }
        }
        self.frames_out.store(fo + m.n as u64, Ordering::Release);
        self.marks_out.fetch_add(1, Ordering::Release);
        true
    }

    /// For the consumer, while nothing produces: lets everything go.
    pub(crate) fn clear(&self) {
        self.frames_out.store(self.frames_in.load(Ordering::Acquire), Ordering::Release);
        self.marks_out.store(self.marks_in.load(Ordering::Acquire), Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_come_out_in_order_with_their_marks_across_the_wrap() {
        let ring = FrameRing::new(10, 4);
        let block = |v: f32, n: usize| vec![v; n];
        assert!(ring.push(0, &block(1.0, 4), &block(-1.0, 4)));
        assert!(ring.push(4, &block(2.0, 4), &block(-2.0, 4)));
        assert!(!ring.push(8, &block(3.0, 4), &block(-3.0, 4)), "12 frames don't fit in 10");
        assert_eq!(ring.buffered(), 8);
        let (mut l, mut r) = (vec![0.0; 4], vec![0.0; 4]);
        assert_eq!(ring.head(), Some(Mark { pos: 0, n: 4 }));
        assert!(ring.pop(&mut l, &mut r));
        assert_eq!((l[3], r[0]), (1.0, -1.0));
        // Now it wraps round the end.
        assert!(ring.push(8, &block(3.0, 4), &block(-3.0, 4)));
        assert!(ring.pop(&mut l, &mut r));
        assert_eq!(l, [2.0; 4]);
        assert_eq!(ring.head(), Some(Mark { pos: 8, n: 4 }));
        assert!(ring.pop(&mut l, &mut r));
        assert_eq!(r, [-3.0; 4]);
        assert!(ring.head().is_none() && !ring.pop(&mut l, &mut r));
    }

    #[test]
    fn it_holds_as_many_blocks_as_it_has_marks_and_clears() {
        let ring = FrameRing::new(100, 2);
        assert!(ring.push(0, &[0.5; 2], &[0.5; 2]));
        assert!(ring.push(2, &[0.5; 2], &[0.5; 2]));
        assert!(!ring.push(4, &[0.5; 2], &[0.5; 2]));
        ring.clear();
        assert_eq!((ring.buffered(), ring.head()), (0, None));
        assert!(ring.push(9, &[0.5; 2], &[0.5; 2]));
        assert_eq!(ring.head(), Some(Mark { pos: 9, n: 2 }));
    }
}
