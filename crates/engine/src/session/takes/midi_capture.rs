//! The MIDI a recording captures, on its way from the engine to `place_recording`. The engine
//! pushes every message it records, and the end of each cycle pass, into one ring; `idle` moves
//! what is there into a list as the take goes on, so however long the take, the ring only holds
//! what arrives between two idles. A full ring loses messages: the engine counts them in
//! `EngineStatus::midi_rec_lost` and the take's placement reports it.

use std::sync::atomic::Ordering;

use crate::dsp::EngineStatus;

/// A recorded message: its position on the timeline and its bytes (or `PASS_MARK`).
pub(crate) type RecordedEvent = (i64, [u8; 3]);

/// Room for what the engine records between two `idle`s, which run every 30 ms. A dense stream
/// (pitch bend and aftertouch on a few controllers at once) is a few thousand messages a second,
/// so this holds over six seconds of 10,000 a second, for a main thread held up by a plug-in
/// editor opening or a dialog. 16 bytes each: 1 MiB.
pub(crate) const RECORDED_MIDI_QUEUE: usize = 1 << 16;

/// The session's end of the ring, and what it has taken from it during this recording.
pub(crate) struct RecordedMidi {
    rx: rtrb::Consumer<RecordedEvent>,
    events: Vec<RecordedEvent>,
}

impl RecordedMidi {
    pub fn new(rx: rtrb::Consumer<RecordedEvent>) -> RecordedMidi {
        RecordedMidi { rx, events: Vec::new() }
    }

    /// A recording starts: whatever the last one left behind goes, and so does its count of
    /// lost messages. The engine isn't recording yet, so nothing is pushed meanwhile.
    pub fn start(&mut self, status: &EngineStatus) {
        while self.rx.pop().is_ok() {}
        self.events.clear();
        status.midi_rec_lost.store(0, Ordering::Relaxed);
    }

    /// Moves what the engine recorded since the last call onto the end of the list, in the order
    /// it was pushed, so the pass ends stay between the right messages.
    pub fn drain(&mut self) {
        let n = self.rx.slots();
        if let Ok(chunk) = self.rx.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            self.events.extend_from_slice(a);
            self.events.extend_from_slice(b);
            chunk.commit_all();
        }
    }

    /// Everything the recording captured, in order, and how many messages it lost; the list and
    /// the count start again empty.
    pub fn take(&mut self, status: &EngineStatus) -> (Vec<RecordedEvent>, u32) {
        self.drain();
        (std::mem::take(&mut self.events), status.midi_rec_lost.swap(0, Ordering::Relaxed))
    }
}

/// What the user is told when a take lost `lost` messages.
pub(crate) fn lost_warning(lost: u32) -> String {
    let what = if lost == 1 { "1 MIDI message".to_string() } else { format!("{lost} MIDI messages") };
    format!("The recording missed {what}: Marbble couldn't take them from the audio engine in time. The rest of the take is kept.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_across_drains() {
        let (mut tx, rx) = rtrb::RingBuffer::new(4);
        let status = EngineStatus::default();
        let mut rec = RecordedMidi::new(rx);
        rec.start(&status);
        let mut sent = Vec::new();
        // Ten messages through a ring of four: drained in between, as idle does, none is lost,
        // and the wrap-around (two slices) keeps its order.
        for i in 0..10i64 {
            let ev = if i == 5 { (i, crate::edit::takes::recording::PASS_MARK) } else { (i, [0xB0, 1, i as u8]) };
            tx.push(ev).unwrap();
            sent.push(ev);
            if i % 3 == 2 {
                rec.drain();
            }
        }
        assert_eq!(rec.take(&status), (sent, 0));
        assert!(rec.take(&status).0.is_empty());
    }

    #[test]
    fn start_forgets_the_last_recording() {
        let (mut tx, rx) = rtrb::RingBuffer::new(4);
        let status = EngineStatus::default();
        let mut rec = RecordedMidi::new(rx);
        tx.push((1, [0x90, 60, 100])).unwrap();
        rec.drain();
        tx.push((2, [0x80, 60, 0])).unwrap();
        status.midi_rec_lost.store(3, Ordering::Relaxed);
        rec.start(&status);
        assert_eq!(rec.take(&status), (Vec::new(), 0));
    }
}
