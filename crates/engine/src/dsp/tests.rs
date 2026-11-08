//! The engine's unit tests that span modules: the click, block load, routes and the block loop,
//! on a small rig that feeds an `Engine` commands and plays blocks. Pure parts test themselves in
//! their own modules.

use super::*;

#[test]
fn a_block_peak_holds_then_falls_to_the_current_load() {
    const BLOCK: f64 = 0.01;
    let mut b = BlockLoad::default();
    b.block(1.5, BLOCK);
    for _ in 0..190 {
        b.block(0.2, BLOCK);
    }
    assert_eq!(b.peak, 1.5, "held for the hold time");
    for _ in 0..20 {
        b.block(0.2, BLOCK);
    }
    assert!(b.peak < 1.5, "falling after it");
    for _ in 0..200 {
        b.block(0.2, BLOCK);
    }
    assert_eq!(b.peak, 0.2, "and stops at the current load");
}

#[test]
fn only_a_higher_block_restarts_the_peak_hold() {
    const BLOCK: f64 = 0.01;
    let mut b = BlockLoad::default();
    b.block(0.5, BLOCK);
    for _ in 0..150 {
        b.block(0.4, BLOCK);
    }
    b.block(0.6, BLOCK);
    for _ in 0..150 {
        b.block(0.4, BLOCK);
    }
    assert_eq!(b.peak, 0.6, "the higher block restarted it");
    for _ in 0..60 {
        b.block(0.4, BLOCK);
    }
    assert!(b.peak < 0.6, "the lower ones didn't");
}

#[test]
fn blocks_over_their_own_length_are_counted() {
    let mut b = BlockLoad::default();
    for load in [0.5, 1.0, 1.2, 0.9, 3.0, 1.01] {
        b.block(load, 0.01);
    }
    assert_eq!(b.late, 3, "exactly 100% made it in time");
}

/// At this sample rate every block is late, but a render runs flat out on purpose.
#[test]
fn a_render_does_not_count_as_late_blocks() {
    let (_tx, rx) = rtrb::RingBuffer::new(64);
    let (gtx, _grx) = rtrb::RingBuffer::new(64);
    let (_live_tx, live_rx) = rtrb::RingBuffer::new(16);
    let (rec_tx, _rec_rx) = rtrb::RingBuffer::new(16);
    let (an_tx, _an_rx) = rtrb::RingBuffer::new(16);
    let status = Arc::new(EngineStatus::default());
    let mut e = Engine::new(1e12, rx, gtx, status.clone(), live_rx, rec_tx, an_tx);
    let mut out = vec![0.0f32; 512 * 2];
    e.offline = true;
    e.process(&[], 0, &mut out, 2);
    assert_eq!(status.late_blocks.load(Ordering::Relaxed), 0);
    assert_eq!(f32::from_bits(status.cpu_peak.load(Ordering::Relaxed)), 0.0);
    e.offline = false;
    e.process(&[], 0, &mut out, 2);
    assert_eq!(status.late_blocks.load(Ordering::Relaxed), 1);
    assert!(f32::from_bits(status.cpu_peak.load(Ordering::Relaxed)) > 1.0);
}

/// At an absurd sample rate every callback is "late", like a machine that can't keep up:
/// the engine must notice, report it and go quiet instead of grinding on.
#[test]
fn a_sustained_overload_sheds_load() {
    let (_tx, rx) = rtrb::RingBuffer::new(64);
    let (gtx, _grx) = rtrb::RingBuffer::new(64);
    let (_live_tx, live_rx) = rtrb::RingBuffer::new(16);
    let (rec_tx, _rec_rx) = rtrb::RingBuffer::new(16);
    let (an_tx, _an_rx) = rtrb::RingBuffer::new(16);
    let status = Arc::new(EngineStatus::default());
    let mut e = Engine::new(1e12, rx, gtx, status.clone(), live_rx, rec_tx, an_tx);
    let mut out = vec![1.0f32; 512 * 2];
    let started = Instant::now();
    while status.overloads.load(Ordering::Relaxed) == 0 && started.elapsed().as_secs() < 5 {
        e.process(&[], 0, &mut out, 2);
    }
    assert_eq!(status.overloads.load(Ordering::Relaxed), 1);
    out.fill(1.0);
    e.process(&[], 0, &mut out, 2);
    assert!(out.iter().all(|&s| s == 0.0), "the next callback is silence");
    assert!(e.shedding > 0);
}

#[test]
fn contain_silences_nans_and_clamps_runaways() {
    let mut l = vec![0.5, f32::NAN, 1e30, -f32::INFINITY];
    let mut r = vec![-0.25; 4];
    assert!(contain(&mut l, &mut r));
    assert_eq!(l, [0.5, 0.0, RUNAWAY, -RUNAWAY]);
    assert!(!contain(&mut l, &mut r), "contained audio passes untouched");
    assert_eq!(r, [-0.25; 4]);
}

struct Rig {
    engine: Engine,
    tx: rtrb::Producer<Cmd>,
    // Holds the rings' far ends open for the rig's life; nothing reads this tuple.
    #[allow(clippy::type_complexity)]
    _keep: (rtrb::Consumer<Garbage>, rtrb::Producer<[u8; 3]>, rtrb::Consumer<(i64, [u8; 3])>, rtrb::Consumer<f32>),
}

fn rig() -> Rig {
    let (tx, rx) = rtrb::RingBuffer::new(64);
    let (gtx, grx) = rtrb::RingBuffer::new(64);
    let (live_tx, live_rx) = rtrb::RingBuffer::new(16);
    let (rec_tx, rec_rx) = rtrb::RingBuffer::new(16);
    let (an_tx, an_rx) = rtrb::RingBuffer::new(16);
    let engine = Engine::new(48000.0, rx, gtx, Arc::new(EngineStatus::default()), live_rx, rec_tx, an_tx);
    Rig { engine, tx, _keep: (grx, live_tx, rec_rx, an_rx) }
}

/// Plays one second (two bars at 480 BPM) with the metronome on and returns the peak level
/// and the number of clicks heard.
fn render(c: ClickSettings, recording: bool) -> (f32, usize) {
    let mut r = rig();
    r.tx.push(Cmd::Tempo(Arc::new(TempoMap::constant(48000.0, 480.0, 4, 4)))).ok().unwrap();
    r.tx.push(Cmd::Metronome { on: true, count_in: false }).ok().unwrap();
    r.tx.push(Cmd::Click(ClickParams::from_settings(&c))).ok().unwrap();
    r.tx.push(if recording { Cmd::Record { from: 0, punch: 0 } } else { Cmd::Play { from: 0 } }).ok().unwrap();
    let mut out = vec![0.0f32; 48000 * 2];
    r.engine.process(&[], 0, &mut out, 2);
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    // A click starts wherever the output wakes up after at least 5 ms of silence.
    let mut clicks = 0;
    let mut quiet = usize::MAX;
    for f in out.chunks(2) {
        if f[0].abs() > 1e-4 {
            if quiet > 240 {
                clicks += 1;
            }
            quiet = 0;
        } else {
            quiet = quiet.saturating_add(1);
        }
    }
    (peak, clicks)
}

#[test]
fn default_click_matches_the_original_level() {
    let (peak, clicks) = render(ClickSettings::default(), false);
    assert!((peak - 0.5).abs() < 0.03, "peak {peak}");
    assert_eq!(clicks, 8);
}

#[test]
fn click_volume_scales_the_level() {
    let (loud, _) = render(ClickSettings::default(), false);
    let (quiet, _) = render(ClickSettings { volume_db: -12.0, ..Default::default() }, false);
    let db = 20.0 * (quiet / loud).log10();
    assert!((db + 12.0).abs() < 0.5, "{db} dB");
}

#[test]
fn accent_off_levels_the_downbeat() {
    let (peak, _) = render(ClickSettings { accent: false, ..Default::default() }, false);
    assert!((peak - 0.32).abs() < 0.03, "peak {peak}");
}

#[test]
fn click_follows_play_and_record_switches() {
    let (p, _) = render(ClickSettings { while_playing: false, ..Default::default() }, false);
    assert_eq!(p, 0.0);
    let (p, _) = render(ClickSettings { while_playing: false, ..Default::default() }, true);
    assert!(p > 0.1);
    let (p, _) = render(ClickSettings { while_recording: false, ..Default::default() }, true);
    assert_eq!(p, 0.0);
    let (p, _) = render(ClickSettings { only_count_in: true, ..Default::default() }, true);
    assert_eq!(p, 0.0);
}

#[test]
fn click_waits_for_compensated_music() {
    let first = |total: u32| {
        let mut r = rig();
        r.tx.push(Cmd::Metronome { on: true, count_in: false }).ok().unwrap();
        r.tx.push(Cmd::PdcGlobal(PdcGlobal { total, low_latency: false })).ok().unwrap();
        r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
        let mut out = vec![0.0f32; 4800 * 2];
        r.engine.process(&[], 0, &mut out, 2);
        out.chunks(2).position(|f| f[0].abs() > 1e-6)
    };
    let (now, later) = (first(0).unwrap(), first(1000).unwrap());
    assert!(now < 4, "{now}");
    assert_eq!(later - now, 1000);
}

#[test]
fn divisions_add_clicks_and_every_sound_is_audible() {
    let (_, clicks) = render(ClickSettings { division: 2, ..Default::default() }, false);
    assert_eq!(clicks, 16);
    for sound in [ClickSound::Classic, ClickSound::Click, ClickSound::Woodblock, ClickSound::Beep] {
        let (peak, clicks) = render(ClickSettings { sound, ..Default::default() }, false);
        assert!(peak > 0.15 && peak < 1.0, "{sound:?} peak {peak}");
        assert_eq!(clicks, 8, "{sound:?}");
    }
}
/// Frames where a click starts (output waking up after 5 ms of silence) and its peak. A
/// click's first sample is silent (its sine starts at 0), so the frame before is reported.
fn onsets(out: &[f32]) -> Vec<(usize, f32)> {
    let mut found: Vec<(usize, f32)> = Vec::new();
    let mut quiet = usize::MAX;
    for (i, f) in out.chunks(2).enumerate() {
        if f[0].abs() > 1e-4 {
            if quiet > 240 {
                found.push((i.saturating_sub(1), 0.0));
            }
            if let Some(last) = found.last_mut() {
                last.1 = last.1.max(f[0].abs());
            }
            quiet = 0;
        } else {
            quiet = quiet.saturating_add(1);
        }
    }
    found
}

fn play_clicks(map: TempoMap, from: i64, punch: Option<i64>, frames: usize) -> Vec<(usize, f32)> {
    let mut r = rig();
    r.tx.push(Cmd::Tempo(Arc::new(map))).ok().unwrap();
    r.tx.push(Cmd::Metronome { on: true, count_in: punch.is_some() }).ok().unwrap();
    r.tx.push(match punch {
        Some(punch) => Cmd::Record { from, punch },
        None => Cmd::Play { from },
    })
    .ok()
    .unwrap();
    let mut out = vec![0.0f32; frames * 2];
    r.engine.process(&[], 0, &mut out, 2);
    onsets(&out)
}

#[test]
fn clicks_follow_tempo_and_signature_changes() {
    use crate::tempo::{SigEvent, TempoEvent};
    // 240 BPM (12000 samples a quarter), 480 BPM from 1 s; 3/4 from bar 2.
    let map = TempoMap::new(
        48000.0,
        &[TempoEvent { pos: 0, bpm: 240.0, ramp: false }, TempoEvent { pos: 48000, bpm: 480.0, ramp: false }],
        &[SigEvent { bar: 2, num: 3, den: 4 }],
        240.0,
        (4, 4),
    );
    let clicks = play_clicks(map, 0, None, 96000);
    let at: Vec<usize> = clicks.iter().map(|c| c.0).collect();
    // Bar 1: four quarters of 12000. Bar 2 starts at 48000; from there a quarter is 6000.
    let want: Vec<usize> = (0..4).map(|k| k * 12000).chain((0..8).map(|k| 48000 + k * 6000)).collect();
    assert_eq!(at, want);
    // Downbeats (accented) at bars 1, 2 (48000), 3 (66000) and 4 (84000).
    let loud: Vec<usize> = clicks.iter().filter(|c| c.1 > 0.4).map(|c| c.0).collect();
    assert_eq!(loud, vec![0, 48000, 66000, 84000]);
}

#[test]
fn count_in_uses_the_tempo_at_the_punch() {
    use crate::tempo::TempoEvent;
    // 480 BPM until the punch at 2 s, then 240 BPM: a 4/4 count-in bar is 48000 samples
    // at the punch's tempo, whatever the tempo before it.
    let map = TempoMap::new(48000.0, &[TempoEvent { pos: 0, bpm: 480.0, ramp: false }, TempoEvent { pos: 96000, bpm: 240.0, ramp: false }], &[], 480.0, (4, 4));
    let clicks = play_clicks(map, 48000, Some(96000), 48000 + 24000);
    let at: Vec<usize> = clicks.iter().map(|c| c.0 + 48000).collect();
    assert_eq!(at, vec![48000, 60000, 72000, 84000, 96000, 108000]);
    assert!(clicks[0].1 > 0.4 && clicks[1].1 < 0.4 && clicks[4].1 > 0.4);
}

#[test]
fn plugins_get_the_position_in_the_map() {
    use crate::tempo::{SigEvent, TempoEvent};
    let map = TempoMap::new(
        48000.0,
        &[
            TempoEvent { pos: 0, bpm: 120.0, ramp: false },
            TempoEvent { pos: 96000, bpm: 60.0, ramp: true },
            TempoEvent { pos: 192000, bpm: 120.0, ramp: false },
        ],
        &[SigEvent { bar: 2, num: 7, den: 8 }],
        120.0,
        (4, 4),
    );
    let mut r = rig();
    r.tx.push(Cmd::Tempo(Arc::new(map))).ok().unwrap();
    r.tx.push(Cmd::Locate(144000)).ok().unwrap();
    r.engine.process(&[], 0, &mut [], 0);
    let t = r.engine.transport();
    // Halfway through the 60 → 120 ramp: 90 BPM, and 4 + 1 s × 75 BPM / 60 quarters.
    assert!((t.tempo - 90.0).abs() < 1e-9);
    assert!((t.position_quarters - 5.25).abs() < 1e-9);
    assert_eq!((t.sig_num, t.sig_den), (7, 8));
    assert!((t.bar_start_quarters - 4.0).abs() < 1e-9);
}

/// An audio track at unity, centred, playing `channels` from the start of the timeline.
fn audio_track(id: Id, channels: Vec<Vec<f32>>) -> TrackDsp {
    let p = TrackParams {
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        armed: false,
        monitor: false,
        input: InputSel::None,
        output: None,
        sends: [None; SEND_SLOTS],
        stack_gain: 1.0,
        stack_mute: false,
        folder: None,
        own_gain: 1.0,
        own_mute: false,
        bus_mute: false,
        vca_gain: 1.0,
        vca_mute: false,
        off: false,
        instrument_output: None,
    };
    let mut t = TrackDsp::new(id, TrackKind::Audio, p, Arc::new(Meter::default()));
    let len = channels[0].len() as i64;
    let clip = Arc::new(Clip::from_channels("track.wav".into(), 48_000, channels));
    t.set_regions(Arc::new(vec![RegionDsp::Audio {
        start: 0,
        length: len,
        offset: 0,
        gain: 1.0,
        clip,
        fade_in: (0, 0),
        fade_out: (len, 0),
        laws: Default::default(),
    }]));
    t
}

#[test]
fn audio_in_the_compensation_after_a_stop_keeps_following_automation() {
    use crate::model::AutoPoint;
    const DELAY: u32 = 4800;
    // A steady 0.5 that its volume lane takes down to -60 dB at 9600.
    let mut r = rig();
    let len = 48_000;
    let t = audio_track(1, vec![vec![0.5; len]; 2]);
    r.tx.push(Cmd::AddTrack(Box::new(t), 0)).ok().unwrap();
    let point = |pos, value| AutoPoint { pos, value, curve: 0.0 };
    let lane = LaneDsp { target: AutoTarget::Volume, points: vec![point(0, 0.0), point(9600, 0.0), point(9601, -60.0)] };
    r.tx.push(Cmd::Automation(1, Arc::new(vec![lane]))).ok().unwrap();
    // Another track's plug-ins are DELAY late: this one waits that long in its line.
    let pdc = PdcParams { output: DELAY, output_time: DELAY, ..Default::default() };
    r.tx.push(Cmd::Pdc { track: 1, params: pdc, line: Some(Box::new(DelayLine::new(8192))), feed_line: None }).ok().unwrap();
    r.tx.push(Cmd::PdcGlobal(PdcGlobal { total: DELAY, low_latency: false })).ok().unwrap();
    r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    let run = |r: &mut Rig, frames: usize| {
        let mut out = vec![0.0f32; frames * 2];
        for c in out.chunks_mut(256 * 2) {
            r.engine.process(&[], 0, c, 2);
        }
        out.chunks(2).map(|f| f[0]).collect::<Vec<f32>>()
    };
    run(&mut r, 12_000);
    let stopped_at = r.engine.pos;
    r.tx.push(Cmd::Stop).ok().unwrap();
    let after = run(&mut r, 256);
    let status = &r.engine.status;
    assert!(!status.playing.load(Ordering::Relaxed) && status.pos.load(Ordering::Relaxed) == stopped_at, "the session sees it stopped at once");
    let after = [after, run(&mut r, DELAY as usize)].concat();
    // What was on its way: 7200..9600 of the timeline at full level, then the lane's -60 dB.
    let loud = (9600 - (stopped_at - DELAY as i64)) as usize;
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(after[..loud - 300].iter().all(|&x| (x - 0.5).abs() < 1e-3), "it plays out");
    assert!(peak(&after[loud + 1200..DELAY as usize]) < 0.01, "at the lane's level, not the fader's: {}", peak(&after[loud + 1200..DELAY as usize]));
    assert!(!r.engine.playing && r.engine.pos == stopped_at, "then the transport stops where it stopped");
}

/// A large project can be more than the device's callbacks keep up with while it plays live,
/// and then the callbacks after the overload are shed. A render that starts meanwhile must not
/// lose its first blocks to that (silence, with the transport held), or it comes out late, unlike
/// the renders after it.
#[test]
fn a_render_after_an_overload_starts_on_time() {
    let mut r = rig();
    let len = 20_000;
    let ramp: Vec<f32> = (0..len).map(|i| i as f32 / len as f32).collect();
    r.tx.push(Cmd::AddTrack(Box::new(audio_track(1, vec![ramp.clone(), ramp])), 0)).ok().unwrap();
    // A latent plug-in elsewhere: the track waits in its line, and the render drops the lead-in.
    const DELAY: u32 = 1500;
    let pdc = PdcParams { output: DELAY, output_time: DELAY, ..Default::default() };
    r.tx.push(Cmd::Pdc { track: 1, params: pdc, line: Some(Box::new(DelayLine::new(8192))), feed_line: None }).ok().unwrap();
    r.tx.push(Cmd::PdcGlobal(PdcGlobal { total: DELAY, low_latency: false })).ok().unwrap();
    r.engine.process(&[], 0, &mut [], 0);
    let clean = r.engine.render(0, len as i64, 0, None, &mut |_| {});
    assert!(clean[0][1..].iter().all(|&x| x > 0.0), "the ramp from its first frame");
    // As a sustained overload leaves it (`a_sustained_overload_sheds_load`).
    r.engine.shedding = 12;
    let after = r.engine.render(0, len as i64, 0, None, &mut |_| {});
    let bits = |x: &[Vec<f32>]| x.iter().flatten().map(|s| s.to_bits()).collect::<Vec<u32>>();
    assert_eq!(bits(&after), bits(&clean), "the same render, from the same frame");
}
