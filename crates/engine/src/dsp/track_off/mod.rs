//! Track On/Off on the audio thread: a track that is off runs nothing, so it costs close to
//! nothing, unlike a muted one, whose regions, instrument and plug-ins all keep running.
//!
//! Switched off, a track first fades through its fader like a muted one (no click), then goes
//! dormant: its plug-ins are told to stop and cleared, its Channel EQ, delay line and side-chain
//! keys are emptied, and from then on each block only empties what feeders wrote into it. Its
//! plug-ins stay loaded. Switched back on, it starts clean: notes are released and chased as
//! after a locate, automation is sent afresh, and the source fades in, ahead of the plug-ins and
//! the delay line, so audio emerging later from a latent path doesn't start with a jump either.
//!
//! The session leaves the track's latency out of delay compensation while it is off, and the
//! other tracks glide to their new delays as for any other latency change (see `session/engine/pdc.rs`).

use super::*;

/// Below about -80 dB a fading track is quiet enough to stop.
const QUIET: f32 = 1e-4;
/// How long the source fades in when a track comes back on.
const RESUME_SECS: f64 = 0.005;

/// The length of the source's fade-in, in frames.
pub(super) fn resume_frames(sr: f64) -> u32 {
    (RESUME_SECS * sr).max(1.0) as u32
}

/// Applies the rest of a fade-in of `len` frames to a block.
pub(super) fn fade_in(left: &mut u32, len: u32, l: &mut [f32], r: &mut [f32]) {
    for (a, b) in l.iter_mut().zip(r.iter_mut()) {
        if *left == 0 {
            return;
        }
        let g = 1.0 - *left as f32 / len as f32;
        *a *= g;
        *b *= g;
        *left -= 1;
    }
}

impl Plugin {
    /// Stops the plug-in and clears its state, for a track going off. A VST3 stays active
    /// (`setActive` belongs to the main thread): see `PluginProcessor::suspend`.
    fn sleep(&mut self) {
        match self {
            Plugin::Vst(p) => p.suspend(),
            Plugin::Instrument(i) => i.sleep(),
            Plugin::Builtin(b) => b.sleep(),
        }
    }

    /// A VST3's parameter edits only reach its processor in `process`. Its editor still works
    /// on an Off track, so while the user moves something there the edits go through, in a block
    /// of silence nobody hears: its saved state and what it plays when back on must have them.
    fn pass_edits(&mut self, l: &mut [f32], r: &mut [f32], t: &TransportInfo) {
        if let Plugin::Vst(p) = self {
            if p.has_edits() {
                l.fill(0.0);
                r.fill(0.0);
                p.process(l, r, &[], t);
                p.suspend();
            }
        }
    }
}

impl Plugin {
    /// `pass_edits` for a plug-in the engine skips on a track that's playing (bypassed, or left
    /// out by Low Latency Mode): the track's buffers are in use, so it brings its own.
    pub(super) fn pass_edits_aside(&mut self, t: &TransportInfo) {
        if let Plugin::Vst(p) = self {
            p.pass_edits(t);
        }
    }
}

impl TrackDsp {
    /// Whether the fader and the pre-fader sends have finished fading (post-fader sends follow
    /// the fader).
    pub(super) fn faded_out(&self) -> bool {
        let sends = self.params.sends.iter().zip(&self.send_gains).all(|(s, g)| !s.is_some_and(|s| s.pre_fader) || g.abs() < QUIET);
        self.gains.0.abs() < QUIET && self.gains.1.abs() < QUIET && sends
    }

    /// One block of an Off track: stops it if it was running, then does nothing but empty its
    /// input, since tracks that send to an Off aux keep writing into it.
    pub(super) fn sleep_block(&mut self, n: usize, transport: &TransportInfo) {
        if !self.dormant {
            self.sleep();
        }
        if matches!(self.kind, TrackKind::Summing | TrackKind::Aux) {
            self.sum_l[..n].fill(0.0);
            self.sum_r[..n].fill(0.0);
        }
        let (l, r) = (&mut self.l[..n], &mut self.r[..n]);
        for ins in self.inserts.iter_mut().flatten().chain(self.instrument.iter_mut()) {
            ins.plugin.pass_edits(l, r, transport);
        }
    }

    fn sleep(&mut self) {
        self.dormant = true;
        self.resume_left = 0;
        self.gains = (0.0, 0.0);
        self.send_gains = [0.0; SEND_SLOTS];
        for ins in self.inserts.iter_mut().flatten().chain(self.instrument.iter_mut()) {
            ins.plugin.sleep();
            ins.wake_up();
        }
        if let Some(d) = &mut self.delay {
            d.clear();
        }
        self.feed.clear();
        // What an effect keyed from this track reads: silence, until it's back on.
        for key in [&mut self.key_l, &mut self.key_r, &mut self.pre_key_l, &mut self.pre_key_r] {
            key.fill(0.0);
        }
        self.flush_notes = true;
        self.chase_cc = true;
        self.auto_sent = [f32::NAN; MAX_AUTO_SENT];
    }

    /// Back on: `fade` frames of fade-in. The delay line holds only silence, so its taps go
    /// straight to their delays.
    pub(super) fn wake(&mut self, fade: u32) {
        self.dormant = false;
        self.resume_left = fade;
        self.pdc_taps = pdc::PdcTaps::at(&self.pdc);
        // The flush set when it went off releases these with the notes it played itself.
        self.adopt_live_notes();
    }

    /// The next flush releases the keys held on this track with the notes it played itself.
    pub(super) fn adopt_live_notes(&mut self) {
        for (sounding, held) in self.sounding.iter_mut().zip(&mut self.live_held) {
            *sounding |= std::mem::take(held);
        }
        // Their keys' note-offs, when they come, have nothing left to end.
        self.live_keys.clear();
    }
}

/// Follows the notes played live on a track, so they can be released after it was off.
pub(super) fn note_held(held: &mut [bool; 128], m: [u8; 3]) {
    match m[0] & 0xF0 {
        0x90 if m[2] > 0 => held[m[1] as usize & 127] = true,
        0x80 | 0x90 => held[m[1] as usize & 127] = false,
        _ => {}
    }
}

#[cfg(test)]
mod tests;
