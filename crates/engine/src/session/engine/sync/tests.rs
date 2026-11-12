//! Sending commands to the engine: a full queue, no device, rebuilds and reopening the device.

use super::*;

/// A session on the null device, whose callback plays block after block as fast as it can.
fn session() -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    assert!(s.audio.is_some(), "the null device runs");
    s.wait_for_engine();
    s.warning = None;
    s
}

fn engine_volume(s: &Session) -> f32 {
    s.engine.lock().master_volume_for_tests()
}

fn dropped(s: &Session) -> u64 {
    s.status.cmds_dropped.load(Ordering::Relaxed)
}

/// Two and a half queues of commands while the callback is held off for 200 ms, as by a long
/// buffer: the main thread waits for room, and every one is applied, in order.
#[test]
fn a_burst_longer_than_the_queue_waits_for_room() {
    let mut s = session();
    let engine = s.engine.clone();
    let (held_tx, held) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _callback_held_off = engine.lock();
        held_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
    });
    held.recv().unwrap();
    let before = s.cmds_sent;
    let t = Instant::now();
    for k in 0..10_000 {
        s.send(Cmd::MasterVolume(k as f32));
    }
    assert!(t.elapsed() >= Duration::from_millis(150), "it waited for the engine");
    holder.join().unwrap();
    s.wait_for_engine();
    assert_eq!(s.cmds_sent - before, 10_000);
    assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent);
    assert_eq!(engine_volume(&s), 9999.0);
    assert_eq!(dropped(&s), 0);
    assert!(!s.engine_resync && s.warning.is_none());
}

/// With no device, each command is applied as it's sent, however many there are.
#[test]
fn without_a_device_commands_apply_at_once() {
    let mut s = session();
    s.audio = None;
    for k in 0..10_000 {
        s.send(Cmd::MasterVolume(k as f32));
        assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent);
    }
    assert_eq!(engine_volume(&s), 9999.0);
    assert_eq!(dropped(&s), 0);
    assert!(!s.engine_resync && s.warning.is_none());
}

/// A callback that takes nothing for longer than `SEND_WAIT`: the edit that finds the queue
/// full waits that long, is dropped, counted and warned about, and those after it are dropped
/// at once. When the engine takes commands again it plays what was queued, out of step with the
/// project, until `idle` stops the transport and rebuilds it from the project.
#[test]
fn an_engine_taking_nothing_is_rebuilt_once_it_does() {
    let mut s = session();
    s.set_master_volume(-6.0);
    s.play();
    s.wait_for_engine();
    assert!(s.is_playing());
    let engine = s.engine.clone();
    let callback_held_off = engine.lock();
    for _ in 0..s.cmd_tx.slots() {
        s.send(Cmd::MasterVolume(0.5));
    }
    let t = Instant::now();
    s.set_master_volume(-12.0);
    assert!(t.elapsed() >= SEND_WAIT);
    assert_eq!(dropped(&s), 1);
    assert!(s.engine_resync);
    assert!(s.warning.take().is_some_and(|w| w.contains("stopped taking changes")));
    let t = Instant::now();
    for _ in 0..100 {
        s.set_master_volume(-18.0);
    }
    assert!(t.elapsed() < SEND_WAIT / 4, "later commands don't wait: {:?}", t.elapsed());
    assert_eq!(dropped(&s), 101);
    // Not while the engine has yet to take what's queued.
    s.resync_engine();
    assert!(s.engine_resync && s.is_playing());

    drop(callback_held_off);
    s.wait_for_engine();
    assert_eq!(engine_volume(&s), 0.5, "the engine is out of step with the project");
    // As the app's idle timer does: an idle may send commands of its own first, and the
    // rebuild waits for the engine to take them.
    let idle_until = |s: &mut Session, done: fn(&Session) -> bool| {
        for _ in 0..100 {
            s.idle();
            s.wait_for_engine();
            if done(s) {
                return;
            }
        }
        panic!("idle didn't get there");
    };
    idle_until(&mut s, |s| !s.is_playing());
    assert!(s.engine_resync, "first the transport stops");
    let w = s.warning.take();
    assert!(w.as_deref().is_some_and(|w| w.contains("Playback stopped")), "{w:?}");
    idle_until(&mut s, |s| !s.engine_resync);
    assert_eq!(engine_volume(&s), volume_gain(-18.0));

    // And commands go as before.
    s.set_master_volume(-3.0);
    s.wait_for_engine();
    assert_eq!(engine_volume(&s), volume_gain(-3.0));
    assert_eq!(dropped(&s), 101);
    assert!(s.warning.is_none());
}

/// A command the engine has yet to take, as when the device stops before its next callback:
/// pushed straight onto the queue, with nothing to drain it.
fn queue(s: &mut Session, cmd: Cmd) {
    assert!(s.audio.is_none());
    assert!(s.cmd_tx.push(cmd).is_ok());
    s.cmds_sent += 1;
}

/// A new track the engine hasn't taken yet: in the project, its strip still in the queue.
fn queue_new_track(s: &mut Session) -> Id {
    let first = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let mut t = s.project.track(first).unwrap().clone();
    t.id = s.project.alloc_id();
    s.project.tracks.push(t.clone());
    let strip = s.build_track(&t);
    queue(s, Cmd::AddTrack(strip, usize::MAX));
    t.id
}

/// The engine is rebuilt (reopening the device at another rate, opening a project) while
/// commands for the old graph are still queued, as when the device has yet to call back: they
/// are applied to that graph before the swap, not on top of the new one.
#[test]
fn a_rebuild_applies_what_is_queued_to_the_old_graph() {
    let mut s = session();
    s.audio = None;
    // The same project: the queued strip would be a second one for its track.
    let id = queue_new_track(&mut s);
    s.rebuild_graph();
    assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent);
    let ids = s.engine.lock().track_ids_for_tests();
    assert_eq!(ids.iter().filter(|&&t| t == id).count(), 1, "one strip per track: {ids:?}");
    assert_eq!(ids.len(), s.project.tracks.len());

    // Another project: the old one's strip would play in it, under an id of the new one's.
    queue_new_track(&mut s);
    s.project = Project::new("Other", s.project.sample_rate);
    s.rebuild_graph();
    assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent);
    assert_eq!(s.engine.lock().track_ids_for_tests(), Vec::<Id>::new());
}

/// Reopened, the device takes what the old one didn't, in order, and the count of commands
/// done still matches those sent, so waiting for the engine waits for what's sent afterwards.
#[test]
fn reopening_the_device_keeps_the_queue_and_its_count() {
    let mut s = session();
    s.audio = None;
    let id = queue_new_track(&mut s);
    s.restart_audio();
    assert!(s.audio.is_some());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent, "the queue was taken and counted");
    assert!(s.engine.lock().track_ids_for_tests().contains(&id));
    s.set_master_volume(-9.0);
    s.wait_for_engine();
    assert_eq!(engine_volume(&s), volume_gain(-9.0));
}

<<<<<<< HEAD
/// A track added to a project on the pool doesn't send its next block to the callback alone until
/// the session's graph follows: the engine builds one of its own at once. With twelve amp models
/// playing, that block overloaded (#475).
#[test]
fn a_track_added_on_the_pool_keeps_the_next_block_there() {
    let mut s = session();
    s.audio = None;
    s.audio_config.processing_threads = 2;
=======
/// A project opened (or otherwise rebuilt) plays its first block on the pool: the task graph, and
/// the workers it wants, go in with the new strips. Left to the next `idle`, they came a block or
/// more late, and a heavy project's first block, every plug-in played on the callback alone,
/// overloaded (#468).
#[test]
fn a_rebuilt_project_plays_its_first_block_on_the_pool() {
    let mut s = session();
    s.audio = None;
    s.audio_config.processing_threads = 2;
    // Two strips with plug-ins that can play at once, as yet unknown to the pool.
>>>>>>> origin/fix/436-undo-notes
    for _ in 0..2 {
        let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        s.set_plugin(t, 0, "builtin:compressor").unwrap();
    }
<<<<<<< HEAD
    s.sync_parallel();
    assert!(s.engine.lock().task_graph_fits(), "the session's graph");
    // The engine has the new strip before the session's next graph.
    s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let before = s.status.parallel_blocks.load(Ordering::Relaxed);
    let mut out = vec![0.0f32; 256 * 2];
    s.engine.lock().process(&[], 0, &mut out, 2);
    assert_eq!(s.status.parallel_blocks.load(Ordering::Relaxed), before + 1, "the block is pooled");
=======
    s.rebuild_graph();
    let before = s.status.parallel_blocks.load(Ordering::Relaxed);
    let mut out = vec![0.0f32; 256 * 2];
    s.engine.lock().process(&[], 0, &mut out, 2);
    assert_eq!(s.status.parallel_blocks.load(Ordering::Relaxed), before + 1, "the first block is pooled");
>>>>>>> origin/fix/436-undo-notes
}
