//! The output callback's work, apart from cpal so tests and `rt_alloc_check` can drive it: the
//! input that arrived pulled off its ring, the engine run, and its output converted to the
//! device's format. A device buffer of any length goes through one `Engine::process_io`, a
//! sub-block at a time, so nothing here is sized by the device and nothing allocates.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use cpal::{FromSample, Sample};

use crate::dsp::{interleave, BlockIo, Engine, EngineStatus, MAX_BLOCK};

/// What the output callback keeps between calls.
pub struct Callback {
    /// The input device's samples, from its own callback; `None` when no input is open.
    in_rx: Option<rtrb::Consumer<f32>>,
    in_ch: usize,
    /// One sub-block of input.
    in_buf: Vec<f32>,
    status: Arc<EngineStatus>,
}

impl Callback {
    pub fn new(in_rx: Option<rtrb::Consumer<f32>>, in_ch: usize, status: Arc<EngineStatus>) -> Self {
        Callback { in_rx, in_ch, in_buf: vec![0.0; MAX_BLOCK * in_ch], status }
    }

    /// One device buffer of `out_ch` interleaved channels, any length. `engine` is `None` when
    /// it's busy (an offline render holds it): the input is still taken and the device gets silence.
    pub fn play<T>(&mut self, engine: Option<&mut Engine>, data: &mut [T], out_ch: usize)
    where
        T: Sample + FromSample<f32>,
    {
        let frames = data.len().checked_div(out_ch).unwrap_or(0);
        let in_ch = self.in_ch;
        if let Some(rx) = &mut self.in_rx {
            // Keep input latency bounded if the two device clocks drift apart.
            let excess = rx.slots().saturating_sub(frames * in_ch * 3);
            if excess > 0 {
                if let Ok(c) = rx.read_chunk(excess - excess % in_ch.max(1)) {
                    c.commit_all();
                }
            }
            self.status.ring_frames.store((rx.slots() / in_ch.max(1)) as u64, Ordering::Relaxed);
        }
        let mut io = DeviceIo { data, out_ch, at: 0, in_rx: self.in_rx.as_mut(), in_ch, in_buf: &mut self.in_buf, taken: 0 };
        match engine {
            Some(e) => {
                // A panic must never reach the audio driver: unwinding into its C callback
                // aborts the whole app. Silence the block instead; the session reports it.
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.process_io(frames, in_ch, &mut io))).is_ok();
                if !ok {
                    io.silence();
                    e.status.engine_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            None => io.silence(),
        }
        // The input of this buffer that the engine didn't take (busy, shedding after an overload,
        // or a block given up on), so the ring doesn't fill and monitoring stays as late as before.
        io.take_rest(frames);
    }
}

/// [`BlockIo`] for one device buffer: input from the ring, output straight into the device's format.
struct DeviceIo<'a, T> {
    data: &'a mut [T],
    out_ch: usize,
    /// Frames written so far.
    at: usize,
    in_rx: Option<&'a mut rtrb::Consumer<f32>>,
    in_ch: usize,
    in_buf: &'a mut [f32],
    /// Frames of input taken so far.
    taken: usize,
}

impl<T: Sample + FromSample<f32>> DeviceIo<'_, T> {
    /// Takes the input up to the buffer's `frames`, a sub-block at a time.
    fn take_rest(&mut self, frames: usize) {
        while self.taken < frames {
            self.input((frames - self.taken).min(MAX_BLOCK));
        }
    }
}

impl<T: Sample + FromSample<f32>> BlockIo for DeviceIo<'_, T> {
    fn input(&mut self, n: usize) -> &[f32] {
        self.taken += n;
        let Some(rx) = &mut self.in_rx else { return &[] };
        let inp = &mut self.in_buf[..n.min(MAX_BLOCK) * self.in_ch];
        let got = rx.slots().min(inp.len());
        let got = got - got % self.in_ch.max(1);
        if let Ok(chunk) = rx.read_chunk(got) {
            let (a, b) = chunk.as_slices();
            inp[..a.len()].copy_from_slice(a);
            inp[a.len()..a.len() + b.len()].copy_from_slice(b);
            chunk.commit_all();
        }
        // An underflow is padded.
        inp[got..].fill(0.0);
        inp
    }

    fn output(&mut self, l: &[f32], r: &[f32]) {
        let at = self.at * self.out_ch;
        interleave(l, r, &mut self.data[at..at + l.len() * self.out_ch], self.out_ch, |s| T::from_sample(s.clamp(-4.0, 4.0)));
        self.at += l.len();
    }

    fn silence(&mut self) {
        self.data.fill(T::EQUILIBRIUM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::tests::{audio_track, rig};
    use crate::dsp::Cmd;
    use crate::model::InputSel;

    /// A device callback much longer than any buffer the callback keeps plays exactly what the
    /// same audio does in callbacks of the usual length, the input included.
    #[test]
    fn a_huge_device_buffer_plays_as_short_ones_do() {
        // Frames: more than three times what the callback once kept, and not a whole number of
        // sub-blocks.
        const FRAMES: usize = 3 * 8192 + 100;
        let input: Vec<f32> = (0..FRAMES * 2).map(|i| ((i * 7919) % 2000) as f32 / 1000.0 - 1.0).collect();
        let play = |sizes: &[usize]| -> Vec<f32> {
            let mut r = rig();
            r.tx.push(Cmd::Metronome { on: true, count_in: false }).ok().unwrap();
            // A track that monitors the input, so it's heard.
            let mut t = audio_track(1, vec![vec![0.0; 16]; 2]);
            (t.params.monitor, t.params.input) = (true, InputSel::Stereo(0));
            r.tx.push(Cmd::AddTrack(Box::new(t), 0)).ok().unwrap();
            r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
            let (mut tx, rx) = rtrb::RingBuffer::new(FRAMES * 2);
            let mut cb = Callback::new(Some(rx), 2, r.status.clone());
            let mut out = vec![0.0f32; FRAMES * 2];
            let mut at = 0;
            for &n in sizes {
                // The input device delivers as the output asks.
                for &s in &input[at * 2..(at + n) * 2] {
                    tx.push(s).unwrap();
                }
                cb.play(Some(&mut r.engine), &mut out[at * 2..(at + n) * 2], 2);
                at += n;
            }
            assert_eq!(at, FRAMES);
            assert_eq!(tx.slots(), FRAMES * 2, "every input sample was taken");
            out
        };
        let short: Vec<usize> = std::iter::repeat_n(MAX_BLOCK, FRAMES / MAX_BLOCK).chain([FRAMES % MAX_BLOCK]).collect();
        let (one, many) = (play(&[FRAMES]), play(&short));
        assert!(one.iter().any(|s| s.abs() > 0.1), "the click plays");
        // The input is heard, past the click's first 5 ms.
        let quiet = |out: &[f32]| (240..FRAMES).filter(|&f| out[f * 2] == 0.0).count();
        assert!(quiet(&one) < FRAMES / 100, "{} silent frames", quiet(&one));
        assert_eq!(one, many);
    }

    /// A busy engine: the device hears silence and the input is still taken.
    #[test]
    fn a_busy_engine_plays_silence_and_takes_the_input() {
        let r = rig();
        let (mut tx, rx) = rtrb::RingBuffer::new(4096);
        let mut cb = Callback::new(Some(rx), 1, r.status.clone());
        for _ in 0..3000 {
            tx.push(0.5).unwrap();
        }
        let mut out = vec![7i16; 3000 * 2];
        cb.play(None, &mut out, 2);
        assert!(out.iter().all(|&s| s == 0));
        assert_eq!(tx.slots(), 4096);
    }

    /// While the engine sheds after an overload it plays nothing, and the input is still taken.
    #[test]
    fn a_shedding_engine_still_takes_the_input() {
        let mut r = rig();
        r.engine.shed_for(3);
        let (mut tx, rx) = rtrb::RingBuffer::new(4096);
        let mut cb = Callback::new(Some(rx), 2, r.status.clone());
        let mut out = vec![1.0f32; 512 * 2];
        for _ in 0..3 {
            for _ in 0..512 * 2 {
                tx.push(0.5).unwrap();
            }
            cb.play(Some(&mut r.engine), &mut out, 2);
            assert!(out.iter().all(|&s| s == 0.0), "silence while shedding");
            assert_eq!(tx.slots(), 4096, "the buffer's input was taken");
        }
    }
}
