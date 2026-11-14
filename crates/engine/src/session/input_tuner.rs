//! The control bar's tuner: the pitch of the selected audio track's input, as the tuner in Logic's
//! LCD shows it. While it shows, the engine copies that input, summed to mono, into a wait-free ring
//! (`Cmd::TunerTap`), which costs the audio thread a copy of each sample. Each tick runs the Tuner
//! effect's detector (`fx/tuner/`) over what came in, off the audio thread, as the Channel EQ's
//! analyzer runs its FFT there.

use super::*;
use crate::fx::tuner::Tuner;
use crate::fx::{Effect, MAX_METERS};

/// The ring holds half a second; a tick takes what came in every 33 ms.
const RING_SECS: f64 = 0.5;
/// At most this much of the newest audio is analysed per tick, so a tick late after a stall
/// doesn't hold the session for long catching up.
const MOST_SECS: f64 = 0.1;
/// The detector is fed in pieces of this many samples, about an audio block each.
const PIECE: usize = 256;
/// Ticks in a row with nothing from the input (a device without inputs, or stopped) before the
/// reading goes blank.
const QUIET_TICKS: u32 = 6;

/// What the control bar's tuner shows, sent with each tick while it's on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TunerReading {
    /// The audio track whose input is tuned; None when the selected track isn't an audio track
    /// with an input.
    pub track: Option<Id>,
    /// The pitch in Hz, 0 when none is heard.
    pub freq: f32,
    /// The nearest MIDI note, -1 when none is heard.
    pub note: i32,
    /// How far off that note, -50 to 50.
    pub cents: f32,
    /// The input level in dBFS.
    pub level: f32,
}

pub(super) struct InputTuner {
    /// The track and input tapped now.
    track: Option<Id>,
    input: InputSel,
    rx: Option<rtrb::Consumer<f32>>,
    tuner: Tuner,
    rate: f64,
    piece: Vec<f32>,
    quiet: u32,
}

impl InputTuner {
    fn new(rate: f64) -> InputTuner {
        InputTuner { track: None, input: InputSel::None, rx: None, tuner: Tuner::new(rate as f32), rate, piece: vec![0.0; PIECE], quiet: 0 }
    }

    /// Takes what the input sent since the last tick into the detector and says what it hears.
    fn read(&mut self) -> TunerReading {
        let mut got = false;
        if let Some(rx) = &mut self.rx {
            let fresh = rx.slots();
            if let Ok(chunk) = rx.read_chunk(fresh) {
                let skip = fresh.saturating_sub((MOST_SECS * self.rate) as usize);
                let (a, b) = chunk.as_slices();
                let mut n = 0;
                for &x in a.iter().chain(b).skip(skip) {
                    self.piece[n] = x;
                    n += 1;
                    if n == PIECE {
                        self.tuner.listen(&self.piece);
                        n = 0;
                    }
                }
                if n > 0 {
                    self.tuner.listen(&self.piece[..n]);
                }
                chunk.commit_all();
                got = fresh > 0;
            }
        }
        self.quiet = if got { 0 } else { self.quiet.saturating_add(1) };
        if self.quiet == QUIET_TICKS {
            self.tuner.reset();
        }
        let mut m = [0.0; MAX_METERS];
        self.tuner.meters(&mut m);
        TunerReading { track: self.track, freq: m[0], note: m[1] as i32, cents: m[2], level: m[4] }
    }
}

impl Session {
    /// Shows or hides the control bar's tuner: while it shows, the selected audio track's input
    /// is tapped and its pitch sent with each tick.
    pub fn set_input_tuner(&mut self, on: bool) {
        match (on, self.input_tuner.is_some()) {
            (true, false) => {
                self.input_tuner = Some(Box::new(InputTuner::new(self.sample_rate())));
                self.input_tuner_idle();
            }
            (false, true) => {
                self.input_tuner = None;
                self.send(Cmd::TunerTap { input: InputSel::None, sink: None });
            }
            _ => {}
        }
    }

    /// The track and input the tuner should listen to: the selected track's, if it's an audio
    /// track with an input.
    fn tuner_wants(&self) -> (Option<Id>, InputSel) {
        let t = self.selected.and_then(|id| self.project.track(id)).filter(|t| t.kind == TrackKind::Audio && t.input.channels() > 0);
        t.map_or((None, InputSel::None), |t| (Some(t.id), t.input))
    }

    /// Follows the selection, its input and the sample rate (from `idle`): a change taps the
    /// new input into a new ring and starts the detector afresh.
    pub(super) fn input_tuner_idle(&mut self) {
        let Some(t) = &self.input_tuner else { return };
        let (track, input) = self.tuner_wants();
        let rate = self.sample_rate();
        if (track, input, rate) == (t.track, t.input, t.rate) {
            return;
        }
        let mut fresh = InputTuner::new(rate);
        fresh.track = track;
        fresh.input = input;
        let sink = (input.channels() > 0).then(|| {
            let (tx, rx) = rtrb::RingBuffer::new((RING_SECS * rate) as usize);
            fresh.rx = Some(rx);
            tx
        });
        self.send(Cmd::TunerTap { input, sink });
        self.input_tuner = Some(Box::new(fresh));
    }

    /// The tuner's reading for the tick, while it shows.
    pub(super) fn input_tuner_tick(&mut self) -> Option<TunerReading> {
        self.input_tuner.as_mut().map(|t| t.read())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::scratch_session;
    use std::f64::consts::TAU;

    /// Plays a sine into input 1 (and silence into input 2) for `secs`, a tick every 1600
    /// frames as the app's tick thread does, and returns the last reading.
    fn play(s: &mut Session, hz: f64, secs: f64) -> Option<TunerReading> {
        let sr = s.sample_rate();
        let mut last = None;
        let mut i = 0usize;
        while (i as f64) < secs * sr {
            let frames: Vec<f32> = (0..1600).flat_map(|k| [(0.3 * (TAU * hz * (i + k) as f64 / sr).sin()) as f32, 0.0]).collect();
            s.feed_input_for_checks(&frames, 2);
            i += 1600;
            last = s.input_tuner_tick();
        }
        last
    }

    #[test]
    fn the_tuner_hears_the_selected_audio_tracks_input() {
        let (mut s, scratch) = scratch_session("input-tuner");
        let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        s.select_track(Some(a));
        assert_eq!(s.input_tuner_tick(), None, "off until it shows");

        s.set_input_tuner(true);
        // A quarter tone sharp of A3 (220 Hz), played on input 1.
        let hz = 220.0 * 2f64.powf(0.25 / 12.0);
        let r = play(&mut s, hz, 0.6).unwrap();
        assert_eq!(r.track, Some(a));
        assert_eq!(r.note, 57);
        assert!((r.cents - 25.0).abs() < 0.5, "{r:?}");
        assert!((r.freq as f64 - hz).abs() < 0.05, "{r:?}");

        // An instrument track has no input to tune.
        s.select_track(Some(inst));
        s.input_tuner_idle();
        let r = play(&mut s, hz, 0.2).unwrap();
        assert_eq!((r.track, r.note), (None, -1));

        // A stereo input is heard summed: the sine on its left still reads.
        s.update_track(a, TrackPatch { input: Some(InputSel::Stereo(0)), ..Default::default() });
        s.select_track(Some(a));
        s.input_tuner_idle();
        let r = play(&mut s, hz, 0.6).unwrap();
        assert_eq!((r.track, r.note), (Some(a), 57));

        s.set_input_tuner(false);
        assert_eq!(s.input_tuner_tick(), None);
        let _ = std::fs::remove_dir_all(scratch);
    }
}
