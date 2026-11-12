//! The snapshot as the session assembles it: tracks in the project's order with the thread that
//! played them, plug-ins and Track Off, the load figures and their reset, and the serialized names
//! the window reads.

use crate::audio_io::NULL_OUTPUT;
use crate::session::*;

fn session() -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

/// Plays `blocks` blocks of 256 frames, then takes a snapshot. Holding the engine's lock keeps the
/// device out meanwhile, so these are the only blocks; taking a snapshot never locks the engine.
fn play_then(s: &Session, blocks: usize, then: impl FnOnce(&Session)) {
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    let mut out = vec![0.0f32; 512];
    for _ in 0..blocks {
        e.process(&[], 0, &mut out, 2);
    }
    then(s);
}

#[test]
fn tracks_say_which_thread_played_them_and_their_share_of_the_block() {
    let mut s = session();
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let b = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.update_track(b, TrackPatch { name: Some("Keys".into()), off: Some(true), ..Default::default() });
    let before = s.diagnostics();
    assert_eq!(before.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), [a, b], "in the project's order");
    let mut d = before;
    play_then(&s, 8, |s| d = s.diagnostics());
    let (ta, tb) = (&d.tracks[0], &d.tracks[1]);
    assert_eq!(ta.thread, Some(0), "played on the audio callback, as there's no pool");
    assert!(ta.share > 0.0 && ta.peak >= ta.share, "{} {}", ta.share, ta.peak);
    assert_eq!((tb.name.as_str(), tb.off, tb.held), ("Keys", true, false));
    assert!(tb.plugins.iter().any(|p| p.instrument && p.builtin), "a new instrument track has a built-in instrument: {:?}", tb.plugins);
    assert!(d.load.blocks >= 8 && d.load.max > 0.0 && d.load.p99 >= d.load.p50);
    assert!(d.load.cmds_done <= d.load.cmds_sent);
    assert_eq!(d.threads.setting, s.audio_config.processing_threads);
    assert!(d.threads.logical_cpus >= 1);
}

#[test]
fn a_deleted_track_leaves_the_snapshot_and_a_reset_starts_the_load_figures_afresh() {
    let mut s = session();
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    play_then(&s, 4, |_| {});
    s.delete_tracks(&[a]);
    assert!(s.diagnostics().tracks.is_empty());
    assert!(!s.strip_loads.contains_key(&a), "its slot goes with it");
    play_then(&s, 0, |s| {
        s.reset_load_statistics();
        let d = s.diagnostics();
        assert_eq!((d.load.blocks, d.load.max), (0, 0.0));
    });
}

#[test]
fn the_window_reads_the_snapshot_by_these_names() {
    let mut s = session();
    s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let v = serde_json::to_value(s.diagnostics()).unwrap();
    for (section, field) in [
        ("audio", "buffer_size_asked"),
        ("audio", "device_gaps"),
        ("load", "p99"),
        ("load", "late_blocks"),
        ("threads", "fast_cpus"),
        ("threads", "pool"),
        ("memory", "resident_kb"),
        ("memory", "lock"),
        ("midi", "recent"),
    ] {
        assert!(v[section].get(field).is_some(), "{section}.{field}");
    }
    let t = &v["tracks"][0];
    assert_eq!(t["kind"], "audio");
    assert!(t.get("thread").is_some() && t.get("share").is_some() && t["plugins"].is_array());
    // Linux has /proc; elsewhere the figure is unknown, not 0.
    assert_eq!(v["memory"]["resident_kb"].is_null(), !cfg!(target_os = "linux"));
}

/// The activity log's lines: a counter that grew says by how much and where the playhead was, one
/// that went down (a reopened device) counts all of it, and the transport and the device say when
/// they start and stop.
#[test]
fn a_watch_says_what_changed_since_the_last() {
    let s = session();
    let before = s.watch();
    let mut now = before.clone();
    assert!(now.changes(&before).is_empty(), "nothing changed");
    now.audio.device_gaps += 2;
    now.load.late_blocks += 1;
    now.load.overloads += 1;
    now.load.overload_ms = Some(23.0);
    now.load.overload_track = Some("Bass".into());
    now.playing = true;
    now.pos = now.audio.sample_rate.max(1) as i64 * 12;
    let lines = now.changes(&before);
    let text = |k: WatchKind| lines.iter().filter(|(kind, _)| *kind == k).map(|(_, t)| t.clone()).collect::<Vec<_>>();
    let gaps = text(WatchKind::Gap);
    assert_eq!(gaps.len(), 3, "{lines:?}");
    assert!(gaps[0].starts_with("device gap ×2") && gaps[0].contains("playing at 0:12.000"), "{}", gaps[0]);
    assert!(gaps[1].starts_with("late buffer ("), "one late buffer, no count: {}", gaps[1]);
    assert!(gaps[2].contains("a buffer took 23.0 ms, \"Bass\" took longest"), "{}", gaps[2]);
    assert_eq!(text(WatchKind::Transport), [format!("playing from 0:12.000 ({})", now.pos)]);
    // A reopened device counts afresh: 1 after 2 is one new gap.
    let mut reopened = now.clone();
    reopened.audio.device_gaps = 1;
    reopened.playing = false;
    let lines = reopened.changes(&now);
    assert!(lines.iter().any(|(k, t)| *k == WatchKind::Gap && t.starts_with("device gap (")), "{lines:?}");
    assert!(lines.iter().any(|(k, t)| *k == WatchKind::Transport && t.starts_with("stopped at 0:12.000")), "{lines:?}");
}
