//! The tempo map: steps and ramps, signatures, normalizing, MIDI file conversion, remapping.

use super::*;

const SR: f64 = 48000.0;

fn ev(pos: i64, bpm: f64, ramp: bool) -> TempoEvent {
    TempoEvent { pos, bpm, ramp }
}

fn sig(bar: u32, num: u32, den: u32) -> SigEvent {
    SigEvent { bar, num, den }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn empty_map_is_the_old_constant_timing() {
    for (bpm, num, den) in [(120.0, 4, 4), (97.0, 7, 8), (133.3, 3, 4), (60.0, 6, 8)] {
        let m = TempoMap::constant(44100.0, bpm, num, den);
        let spb = 44100.0 * 60.0 / bpm * (4.0 / den as f64);
        let spbar = spb * num as f64;
        assert!(m.is_constant());
        for k in [0i64, 1, 7, 100, 12345] {
            assert!(close(m.bar_start(k + 1), k as f64 * spbar, 1e-6), "{bpm} bar {k}");
        }
        for pos in [0.0, 1.0, 22050.0, 123456.0, 9_999_999.0] {
            // The old toBbt.
            let beats = pos / spb;
            let bar = (beats / num as f64).floor();
            let in_bar = beats - bar * num as f64;
            let beat = in_bar.floor();
            let frac = in_bar - beat;
            let div = (frac * 4.0).floor();
            let tick = ((frac * 4.0 - div) * 240.0).floor();
            let want = Bbt { bar: bar as i64 + 1, beat: beat as u32 + 1, div: div as u32 + 1, tick: tick as u32 + 1 };
            assert_eq!(m.bar_beat_at(pos), want, "{bpm} at {pos}");
            assert!(close(m.beats_at(pos), pos / (44100.0 * 60.0 / bpm), 1e-9));
            assert_eq!(m.bpm_at(pos), bpm);
            assert_eq!(m.sig_at(pos), (num, den));
        }
        // The old click grid: k * spb / div, rounded.
        let div = 2;
        let mut g = m.grid_at_or_after(0.0, div);
        for k in 0..64i64 {
            assert_eq!(g.pos.round() as i64, (k as f64 * spb / div as f64).round() as i64);
            assert_eq!(g.downbeat(), k % (num as i64 * div as i64) == 0);
            g = m.grid_after(&g);
        }
    }
}

#[test]
fn steps_round_trip() {
    // 120 BPM for 2 s (4 quarters), then 60 BPM.
    let m = TempoMap::new(SR, &[ev(0, 120.0, false), ev(96000, 60.0, false)], &[], 120.0, (4, 4));
    assert!(close(m.beats_at(96000.0), 4.0, 1e-12));
    assert!(close(m.beats_at(144000.0), 5.0, 1e-12));
    assert!(close(m.sample_at(5.0), 144000.0, 1e-9));
    assert_eq!(m.bpm_at(95999.0), 120.0);
    assert_eq!(m.bpm_at(96000.0), 60.0);
    for s in [0.0, 1.0, 50000.0, 95999.5, 96000.0, 96001.0, 1e7] {
        assert!(close(m.sample_at(m.beats_at(s)), s, 1e-6), "{s}");
    }
    // Bar 2 starts at the tempo change; bar 3 four seconds later.
    assert!(close(m.bar_start(2), 96000.0, 1e-9));
    assert!(close(m.bar_start(3), 96000.0 + 4.0 * SR, 1e-9));
    assert_eq!(m.bar_beat_at(96000.0 + 2.0 * SR), Bbt { bar: 2, beat: 3, div: 1, tick: 1 });
}

#[test]
fn ramps_integrate_exactly() {
    // 60 → 180 BPM over 4 s: the mean is 120 BPM, so 8 quarters.
    let m = TempoMap::new(SR, &[ev(0, 60.0, true), ev(4 * 48000, 180.0, false)], &[], 60.0, (4, 4));
    assert!(close(m.beats_at(4.0 * SR), 8.0, 1e-9));
    assert!(close(m.bpm_at(2.0 * SR), 120.0, 1e-9));
    // First second: from 60 to 90 BPM, mean 75: 1.25 quarters.
    assert!(close(m.beats_at(SR), 1.25, 1e-9));
    // After the ramp, 180 BPM holds: 3 quarters a second.
    assert!(close(m.beats_at(5.0 * SR), 11.0, 1e-9));
    for q in [0.0, 0.001, 1.25, 3.3, 7.999, 8.0, 8.5, 40.0] {
        assert!(close(m.beats_at(m.sample_at(q)), q, 1e-9), "{q}");
    }
    assert!(close(m.sample_at(1.25), SR, 1e-6));
    // A slowing ramp too.
    let d = TempoMap::new(SR, &[ev(0, 200.0, true), ev(96000, 50.0, false)], &[], 200.0, (4, 4));
    assert!(close(d.beats_at(96000.0), 2.0 * 125.0 / 60.0, 1e-9));
    for s in [0.0, 1000.0, 48000.0, 95999.0, 96000.0, 200000.0] {
        assert!(close(d.sample_at(d.beats_at(s)), s, 1e-6), "{s}");
    }
    // Positions keep increasing through the ramp.
    let mut last = -1.0;
    for k in 0..200 {
        let s = d.sample_at(k as f64 * 0.05);
        assert!(s > last);
        last = s;
    }
}

#[test]
fn nearly_flat_ramp_is_stable() {
    let m = TempoMap::new(SR, &[ev(0, 120.0, true), ev(10_000_000, 120.000001, false)], &[], 120.0, (4, 4));
    for q in [0.5, 10.0, 400.0] {
        // Within a tenth of a sample of 120 BPM (the drift is a few hundredths), and the
        // inverse holds.
        assert!(close(m.sample_at(q), q * 24000.0, 0.1), "{q}");
        assert!(close(m.beats_at(m.sample_at(q)), q, 1e-9), "{q}");
    }
}

#[test]
fn signature_changes_number_bars() {
    // 4/4 for bars 1–2, 7/8 for bars 3–4, 3/4 from bar 5. 120 BPM: 24000 samples a quarter.
    let m = TempoMap::new(SR, &[], &[sig(1, 4, 4), sig(3, 7, 8), sig(5, 3, 4)], 120.0, (4, 4));
    let q = 24000.0;
    let starts = [0.0, 4.0, 8.0, 11.5, 15.0, 18.0, 21.0];
    for (i, s) in starts.iter().enumerate() {
        assert!(close(m.bar_start(i as i64 + 1), s * q, 1e-6), "bar {}", i + 1);
        assert_eq!(m.bar_at(s * q), i as i64 + 1);
        assert_eq!(m.bar_beat_at(s * q), Bbt { bar: i as i64 + 1, beat: 1, div: 1, tick: 1 });
    }
    assert_eq!(m.sig_at_bar(2), (4, 4));
    assert_eq!(m.sig_at_bar(3), (7, 8));
    assert_eq!(m.sig_at_bar(4), (7, 8));
    assert_eq!(m.sig_at_bar(5), (3, 4));
    assert_eq!(m.sig_at_bar(99), (3, 4));
    assert_eq!(m.sig_at(9.0 * q), (7, 8));
    // In 7/8, beats are eighths: half a quarter past bar 3 is beat 2.
    assert_eq!(m.bar_beat_at(8.5 * q), Bbt { bar: 3, beat: 2, div: 1, tick: 1 });
    assert_eq!(m.bar_beat_at(11.0 * q), Bbt { bar: 3, beat: 7, div: 1, tick: 1 });
    // The last sixteenth of bar 4 is in beat 7 of 7/8.
    assert_eq!(m.bar_beat_at(14.875 * q).beat, 7);
    // Bar lines and beats in a range: 7 beats in each 7/8 bar.
    let bars: Vec<_> = m.bars(8.0 * q, 15.0 * q).map(|b| (b.bar, b.num, b.den)).collect();
    assert_eq!(bars, vec![(3, 7, 8), (4, 7, 8)]);
    let beats: Vec<_> = m.grid(8.0 * q, 15.0 * q, 1).collect();
    assert_eq!(beats.len(), 14);
    assert!(beats[0].downbeat() && beats[7].downbeat() && !beats[6].downbeat());
    assert!(close(beats[7].pos, 11.5 * q, 1e-6));
    // Stepping
    assert!(close(m.next_bar(8.0 * q), 11.5 * q, 1e-6));
    assert!(close(m.prev_bar(11.5 * q), 8.0 * q, 1e-6));
    assert!(close(m.prev_bar(11.6 * q), 11.5 * q, 1e-6));
    assert!(close(m.next_beat(11.0 * q), 11.5 * q, 1e-6));
    assert!(close(m.prev_beat(11.5 * q), 11.0 * q, 1e-6));
    assert!(close(m.prev_beat(8.0 * q), 7.0 * q, 1e-6));
    assert!(close(m.bar_position(13.25 * q), 3.0 + 1.75 / 3.5, 1e-9));
    // Snapping counts from the bar line: in 7/8 the quarter grid of bar 3 is 8, 9, 10
    // and 11, then bar 4 starts at 11.5.
    assert!(close(m.snap_q(11.3, 1.0), 11.5, 1e-9));
    assert!(close(m.snap_q(11.2, 1.0), 11.0, 1e-9));
    assert!(close(m.snap_q(12.4, 1.0), 12.5, 1e-9));
    assert!(close(m.snap_q(8.4, 0.25), 8.5, 1e-9));
}

#[test]
fn signatures_and_tempo_changes_together() {
    // 7/8 from bar 2, and the tempo halves at bar 2's line (4 quarters = 2 s at 120).
    let m = TempoMap::new(SR, &[ev(0, 120.0, false), ev(96000, 60.0, false)], &[sig(2, 7, 8)], 120.0, (4, 4));
    // Bar 2 is 3.5 quarters at 60 BPM: 3.5 s.
    assert!(close(m.bar_start(3), 96000.0 + 3.5 * SR, 1e-6));
    assert_eq!(m.bar_beat_at(96000.0 + 0.5 * SR), Bbt { bar: 2, beat: 2, div: 1, tick: 1 });
    assert!(close(m.samples_per_beat_at(100000.0), 24000.0, 1e-9));
    assert!(close(m.samples_per_bar_at(100000.0), 7.0 * 24000.0, 1e-9));
    // The click grid across both changes: 4 quarters in bar 1, then eighths every 0.5 s.
    let pts: Vec<_> = m.grid(0.0, 96000.0 + 3.5 * SR, 1).map(|g| g.pos.round() as i64).collect();
    let mut want: Vec<i64> = (0..4).map(|k| k * 24000).collect();
    want.extend((0..7).map(|k| 96000 + k * 24000));
    assert_eq!(pts, want);
}

#[test]
fn normalizing_sorts_dedupes_and_anchors() {
    let n = normalize_tempo(&[ev(500, 90.0, true), ev(100, 1000.0, false), ev(500, 80.0, true)], 110.0);
    assert_eq!(n, vec![ev(0, 110.0, false), ev(100, 990.0, false), ev(500, 80.0, false)]);
    let s = normalize_sigs(&[sig(4, 5, 3), sig(0, 3, 4)], (4, 4));
    assert_eq!(s, vec![sig(1, 3, 4), sig(4, 5, 4)]);
    assert_eq!(normalize_sigs(&[], (6, 8)), vec![sig(1, 6, 8)]);
}

#[test]
fn negative_positions_extend_the_first_tempo() {
    let m = TempoMap::new(SR, &[ev(0, 60.0, true), ev(48000, 120.0, false)], &[], 60.0, (3, 4));
    assert!(close(m.beats_at(-48000.0), -1.0, 1e-12));
    assert!(close(m.sample_at(-3.0), -3.0 * SR, 1e-9));
    assert_eq!(m.bar_at(-1.0), 0);
    assert!(close(m.bar_start(0), -3.0 * SR, 1e-9));
}

#[test]
fn midi_file_changes_convert() {
    // 120 BPM for 4 quarters, then 60.
    let ev = events_from_quarters(SR, &[(0.0, 120.0), (4.0, 60.0)]);
    assert_eq!(ev, vec![ev_(0, 120.0), ev_(96000, 60.0)]);
    // 4/4 for two bars, then 7/8 for one bar, then 3/4.
    let s = sigs_from_quarters(&[(0.0, 4, 4), (8.0, 7, 8), (11.5, 3, 4)], (4, 4));
    assert_eq!(s, vec![sig(1, 4, 4), sig(3, 7, 8), sig(4, 3, 4)]);
}

fn ev_(pos: i64, bpm: f64) -> TempoEvent {
    ev(pos, bpm, false)
}

#[test]
fn remap_keeps_bars_and_beats() {
    let a = TempoMap::constant(SR, 120.0, 4, 4);
    let b = TempoMap::new(SR, &[ev(0, 120.0, false), ev(96000, 60.0, false)], &[], 120.0, (4, 4));
    // Bar 3 at 120 throughout is 4 s; with the tempo halved after bar 2 it is 6 s.
    assert!(close(a.remap(&b, 4.0 * SR), 6.0 * SR, 1e-6));
    assert!(close(a.remap(&b, 1.0 * SR), 1.0 * SR, 1e-6));
}
