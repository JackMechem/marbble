//! Exercises controller feedback through a real MIDI output: the check opens a virtual MIDI input
//! port (an ALSA sequencer port on Linux) that stands for a controller's input, the session
//! connects to it as one of its outputs, and the controller's moves come in through the hardware
//! input path (`inject_midi_from`) under the same port name, so the two pair automatically.
//! It checks what reaches the port after a mouse-style change, an undo, automation playback, a
//! button press and the transport, a pitch bend's 14 bits, a button matrix's pads (notes) and
//! none for a key command's, the rate limit, that nothing is echoed while the controller moves,
//! Feedback off, pairing and a reconnected output.
//! Prints SKIP when the system has no MIDI sequencer or virtual ports (Windows, CI runners
//! without the ALSA sequencer).
//! usage: feedback_check <scratch dir>
use std::sync::Arc;
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::controllers::{db_to_fader, CtlKind, CtlTarget, Takeover, TransportAction};
use daw_engine::feedback::MIN_GAP;
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, InputSel, TrackKind};
use daw_engine::session::{AssignmentPatch, TrackPatch};
use daw_engine::Session;
use parking_lot::Mutex;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// What arrived at the controller's port, with when.
type Received = Arc<Mutex<Vec<(Instant, [u8; 3])>>>;

/// The messages received since the last call, after the output thread and the sequencer have
/// had time to deliver them.
fn take(rx: &Received) -> Vec<[u8; 3]> {
    std::thread::sleep(Duration::from_millis(60));
    rx.lock().drain(..).map(|m| m.1).collect()
}

/// The controller's messages of one kind (status byte and first data byte).
fn of(msgs: &[[u8; 3]], status: u8, data1: u8) -> Vec<[u8; 3]> {
    msgs.iter().filter(|m| m[0] == status && (status & 0xF0 == 0xE0 || m[1] == data1)).copied().collect()
}

/// CC value showing a level in dB on the fader's taper.
fn cc_of(db: f32) -> u8 {
    (db_to_fader(db) * 127.0).round() as u8
}

/// Opens the controller's MIDI input: a virtual port of a client of its own, which the session
/// sees as an output. Kept open while the value lives.
#[cfg(unix)]
fn controller_port(sink: Received) -> Result<Box<dyn std::any::Any>, String> {
    use midir::os::unix::VirtualInput;
    let input = midir::MidiInput::new("Feedback Check").map_err(|e| format!("no MIDI system ({e})"))?;
    let conn = input
        .create_virtual(
            "Feedback Check MIDI 1",
            move |_, msg, _| {
                if msg.len() == 3 {
                    sink.lock().push((Instant::now(), [msg[0], msg[1], msg[2]]));
                }
            },
            (),
        )
        .map_err(|e| format!("could not open a virtual MIDI port ({e})"))?;
    Ok(Box::new(conn))
}

#[cfg(not(unix))]
fn controller_port(_: Received) -> Result<Box<dyn std::any::Any>, String> {
    Err("no virtual MIDI ports on this system".into())
}

struct Rig {
    s: Session,
    rx: Received,
    /// The input port name the controller's moves pretend to come from, with ALSA's numbers.
    port: String,
}

impl Rig {
    /// A control change from the controller, handled as `idle` does every 30 ms in the app.
    fn cc(&mut self, number: u8, value: u8) {
        self.s.inject_midi_from(&self.port, &[0xB0, number, value]);
        self.s.idle();
    }

    fn learn(&mut self, target: CtlTarget, msg: &[u8]) {
        self.s.start_learn(Some(target)).unwrap();
        self.s.inject_midi_from(&self.port, msg);
        self.s.idle();
        // A CC 0..31 may be a 14-bit controller's coarse part: the learn waits for a fine part.
        if self.s.learning().is_some() {
            std::thread::sleep(Duration::from_millis(150));
            self.s.idle();
        }
        assert!(self.s.tick().learned.is_some(), "learned");
    }

    fn index_of(&self, kind: CtlKind) -> usize {
        self.s.controller_assignments().iter().position(|r| r.assignment.controller.kind == kind).expect("assignment")
    }

    /// Sets an assignment's takeover mode and range, as the window does.
    fn set_range(&mut self, i: usize, mode: Takeover, min: f32, max: f32) {
        self.s.update_controller_assignment(i, AssignmentPatch { mode: Some(mode), min: Some(min), max: Some(max), ..Default::default() }).unwrap();
    }

    /// A message from the controller (a pad's note), handled as `idle` does.
    fn send(&mut self, msg: &[u8]) {
        self.s.inject_midi_from(&self.port, msg);
        self.s.idle();
    }

    /// Idles past the quiet time after a controller's move.
    fn settle(&mut self) {
        std::thread::sleep(daw_engine::feedback::QUIET + Duration::from_millis(50));
        self.s.idle();
    }
}

fn main() {
    let scratch = std::path::PathBuf::from(std::env::args().nth(1).expect("scratch dir"));

    let rx: Received = Arc::default();
    let _controller = match controller_port(rx.clone()) {
        Ok(c) => c,
        Err(e) => {
            println!("SKIP {e}");
            return;
        }
    };

    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("FeedbackCheck.marbble");
    s.load_controller_assignments(scratch.join("controller-assignments.json"));
    s.refresh_midi();
    let Some(out) = s.midi_outputs().into_iter().find(|p| p.name.contains("Feedback Check MIDI 1")) else {
        println!("SKIP the virtual port isn't listed as an output");
        return;
    };
    check(&format!("the controller's port is listed and connected as an output ({})", out.name), out.connected && out.enabled);
    check("Marbble's own ports aren't listed", s.midi_outputs().iter().chain(s.midi_ports().iter()).all(|p| !p.name.starts_with("Marbble:")));
    // The input port of the same device, as ALSA names a hardware port (its numbers change).
    let port = format!("{} 20:0", out.key);
    let mut r = Rig { s, rx, port };

    let a = r.s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let vol = |s: &Session| s.project.track(a).unwrap().volume_db;

    // Learning: the new assignment gets the parameter's value at once.
    r.learn(CtlTarget::Volume { track: a }, &[0xB0, 7, 30]);
    let rows = r.s.controller_assignments();
    check(&format!("the input is paired with the output of the same name ({:?})", rows[0].output), rows[0].output.as_deref() == Some(out.key.as_str()));
    check("feedback is on for a new assignment", rows[0].assignment.feedback);
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("a learnt fader is sent its value ({got:?})"), of(&got, 0xB0, 7) == vec![[0xB0, 7, cc_of(vol(&r.s))]]);
    r.s.idle();
    check("an unchanged value isn't sent again", take(&r.rx).is_empty());

    // The mouse moves the fader.
    r.s.checkpoint("Volume", Some(format!("track:{a}:volume")), false);
    r.s.update_track_linked(a, TrackPatch { volume_db: Some(-12.0), ..Default::default() });
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("a mouse move is sent on the fader's taper ({got:?})"), got == vec![[0xB0, 7, cc_of(-12.0)]]);

    // Undo puts it back, and the controller follows.
    r.s.undo();
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("an undo is sent ({got:?}, {} dB)", vol(&r.s)), got == vec![[0xB0, 7, cc_of(vol(&r.s))]] && vol(&r.s) > -12.0);

    // A range: the controller shows where the fader is inside it.
    let i = r.index_of(CtlKind::Cc { number: 7 });
    r.set_range(i, Takeover::Jump, 0.0, 0.5);
    r.s.update_track_linked(a, TrackPatch { volume_db: Some(-40.0), ..Default::default() });
    r.s.idle();
    let got = take(&r.rx);
    let want = ((db_to_fader(-40.0) / 0.5) * 127.0).round() as u8;
    check(&format!("feedback goes through the range ({got:?}, want {want})"), got == vec![[0xB0, 7, want]]);
    r.set_range(i, Takeover::Jump, 0.0, 1.0);
    r.s.idle();
    take(&r.rx);

    // No echo: while the controller moves the fader nothing goes back, nor after it stops when
    // the fader is where the controller put it.
    for v in [20u8, 40, 60, 80, 100] {
        r.cc(7, v);
        std::thread::sleep(Duration::from_millis(30));
        r.s.idle();
    }
    let during = take(&r.rx);
    r.settle();
    let after = take(&r.rx);
    check(&format!("nothing is echoed while the controller moves ({during:?}), nor after ({after:?})"), during.is_empty() && after.is_empty());

    // Pickup: the controller's moves don't reach the fader; once it stops, it is told where the
    // fader is (a motor fader goes there).
    r.set_range(i, Takeover::Pickup, 0.0, 1.0);
    r.s.update_track_linked(a, TrackPatch { volume_db: Some(0.0), ..Default::default() });
    r.s.idle();
    take(&r.rx);
    for v in [10u8, 20, 30] {
        r.cc(7, v);
        std::thread::sleep(Duration::from_millis(30));
    }
    let during = take(&r.rx);
    r.settle();
    let after = take(&r.rx);
    check(
        &format!("a Pickup controller that didn't catch the fader is told where it is once it stops ({during:?} then {after:?})"),
        during.is_empty() && after == vec![[0xB0, 7, cc_of(0.0)]] && vol(&r.s) == 0.0,
    );
    r.set_range(i, Takeover::Jump, 0.0, 1.0);

    // The rate limit: the mouse moves the fader every 2 ms for 400 ms.
    let t0 = Instant::now();
    let mut n = 0;
    while t0.elapsed() < Duration::from_millis(400) {
        n += 1;
        r.s.update_track_linked(a, TrackPatch { volume_db: Some(-30.0 + (n % 300) as f32 * 0.1), ..Default::default() });
        r.s.idle();
        std::thread::sleep(Duration::from_millis(2));
    }
    let last = vol(&r.s);
    std::thread::sleep(MIN_GAP);
    r.s.idle();
    std::thread::sleep(Duration::from_millis(60));
    let got: Vec<(Instant, [u8; 3])> = r.rx.lock().drain(..).collect();
    // (Arrival times jitter a little on the way through the sequencer.)
    let gaps_ok = got.windows(2).all(|w| w[1].0.duration_since(w[0].0) + Duration::from_millis(15) >= MIN_GAP);
    let limit = 400 / MIN_GAP.as_millis() as usize + 2;
    check(
        &format!("{n} changes in 400 ms send {} messages (at most {limit}), {} ms apart or more", got.len(), MIN_GAP.as_millis()),
        got.len() <= limit && got.len() >= 5 && gaps_ok,
    );
    check(&format!("the last value always goes ({:?} for {last} dB)", got.last().map(|m| m.1)), got.last().map(|m| m.1) == Some([0xB0, 7, cc_of(last)]));

    // Automation playback in Read: the controller follows the lane. The null device plays
    // faster than real time (the macOS stand-in doesn't), so the ramp is as long as about 1.5 s
    // of what this machine plays.
    r.s.locate(0);
    r.s.play();
    std::thread::sleep(Duration::from_millis(200));
    let rate = r.s.position() as f64 / 0.2;
    r.s.stop();
    std::thread::sleep(Duration::from_millis(60));
    let len = ((rate * 1.5) as i64).max(48000);
    let lane = |pos: i64| -60.0 + 66.0 * (pos.clamp(0, len) as f32 / len as f32);
    r.s.set_automation_lane(a, AutoParam::Volume, vec![AutoPoint { pos: 0, value: -60.0, curve: 0.0 }, AutoPoint { pos: len, value: 6.0, curve: 0.0 }]);
    r.s.set_auto_mode(a, AutoMode::Read);
    r.s.update_track_linked(a, TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    r.s.idle();
    take(&r.rx);
    r.s.locate(0);
    r.s.play();
    // Each idle sends what the lane plays at the playhead then (between the positions before
    // and after it).
    let (mut played, mut off) = (Vec::new(), Vec::new());
    for _ in 0..12 {
        std::thread::sleep(Duration::from_millis(30));
        let p0 = r.s.position();
        r.s.idle();
        let p1 = r.s.position();
        for m in of(&take(&r.rx), 0xB0, 7) {
            let (lo, hi) = (cc_of(lane(p0)), cc_of(lane(p1)));
            if m[2] + 1 < lo || m[2] > hi + 1 {
                off.push((m[2], lo, hi));
            }
            played.push(m);
        }
    }
    r.s.stop();
    std::thread::sleep(Duration::from_millis(60));
    take(&r.rx);
    check(
        &format!("automation playback is sent as it plays ({} messages, {:?} .. {:?}; off the lane: {off:?})", played.len(), played.first(), played.last()),
        played.len() >= 8 && off.is_empty() && played.last().is_some_and(|m| m[2] > played[0][2] + 10),
    );
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("stopped, the fader's own value is sent ({got:?})"), got == vec![[0xB0, 7, cc_of(-6.0)]]);
    r.s.set_auto_mode(a, AutoMode::Off);
    r.s.clear_automation(a, None);

    // A button: Mute's light is set on the release, and follows the mouse.
    r.learn(CtlTarget::Mute { track: a }, &[0xB0, 30, 0]);
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("a learnt Mute button is sent its state ({got:?})"), of(&got, 0xB0, 30) == vec![[0xB0, 30, 0]]);
    r.cc(30, 127);
    let pressed = take(&r.rx);
    r.cc(30, 0);
    let released = take(&r.rx);
    check(
        &format!("a press mutes; its light is lit on the release ({pressed:?} then {released:?})"),
        r.s.project.track(a).unwrap().mute && pressed.is_empty() && released == vec![[0xB0, 30, 127]],
    );
    r.s.update_track_linked(a, TrackPatch { mute: Some(false), ..Default::default() });
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("unmuting with the mouse puts the light out ({got:?})"), got == vec![[0xB0, 30, 0]]);

    // Transport buttons.
    r.learn(CtlTarget::Transport { action: TransportAction::Cycle }, &[0xB0, 31, 0]);
    r.learn(CtlTarget::Transport { action: TransportAction::Play }, &[0xB0, 32, 0]);
    r.s.set_cycle(false, 0, 48000);
    r.s.idle();
    take(&r.rx);
    r.s.set_cycle(true, 0, 48000);
    r.s.play();
    std::thread::sleep(Duration::from_millis(60));
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("Cycle and Play light up ({got:?})"), of(&got, 0xB0, 31) == vec![[0xB0, 31, 127]] && of(&got, 0xB0, 32) == vec![[0xB0, 32, 127]]);
    r.s.stop();
    std::thread::sleep(Duration::from_millis(60));
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("stopping puts Play out ({got:?})"), of(&got, 0xB0, 32) == vec![[0xB0, 32, 0]]);

    // Pitch bend: 14 bits.
    r.learn(CtlTarget::Pan { track: a }, &[0xE0, 0, 0x40]);
    r.s.idle();
    take(&r.rx);
    r.s.update_track_linked(a, TrackPatch { pan: Some(0.5), ..Default::default() });
    r.s.idle();
    let got = take(&r.rx);
    let n = (0.75f32 * 16383.0).round() as u16;
    check(&format!("a pitch bend controller gets 14 bits ({got:?})"), got == vec![[0xE0, (n & 0x7F) as u8, (n >> 7) as u8]]);

    // A button matrix sending notes: each pad's light follows its Mute, toggled by the pad (lit
    // when the pad is let go) or by the mouse.
    let pads =
        [r.s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap(), r.s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()];
    let muted = |s: &Session, t| s.project.track(t).unwrap().mute;
    r.learn(CtlTarget::Mute { track: pads[0] }, &[0x90, 36, 100]);
    r.send(&[0x80, 36, 0]);
    r.learn(CtlTarget::Mute { track: pads[1] }, &[0x90, 37, 100]);
    r.send(&[0x80, 37, 0]);
    r.settle();
    take(&r.rx);
    r.send(&[0x90, 36, 100]);
    let pressed = take(&r.rx);
    r.send(&[0x80, 36, 0]);
    let released = take(&r.rx);
    check(
        &format!("a pad toggles Mute on and its light is lit on release ({pressed:?} then {released:?})"),
        muted(&r.s, pads[0]) && pressed.is_empty() && released == vec![[0x90, 36, 127]],
    );
    r.send(&[0x90, 36, 100]);
    r.send(&[0x80, 36, 0]);
    let got = take(&r.rx);
    check(&format!("pressed again, Mute goes off and its light is put out ({got:?})"), !muted(&r.s, pads[0]) && got == vec![[0x90, 36, 0]]);
    r.s.update_track_linked(pads[1], TrackPatch { mute: Some(true), ..Default::default() });
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("muting with the mouse lights the other pad ({got:?})"), got == vec![[0x90, 37, 127]]);
    r.s.update_track_linked(pads[1], TrackPatch { mute: Some(false), ..Default::default() });
    r.s.idle();
    let got = take(&r.rx);
    check(&format!("and unmuting puts it out ({got:?})"), got == vec![[0x90, 37, 0]]);

    // A key command has no state to show: its pad is sent nothing.
    r.s.start_learn_named(Some(CtlTarget::Command { id: "next-marker".into() }), Some("Key Command › Go to Next Marker".into())).unwrap();
    r.send(&[0x90, 38, 100]);
    r.send(&[0x80, 38, 0]);
    r.settle();
    let got = take(&r.rx);
    check(&format!("a key command's pad gets no feedback ({got:?})"), of(&got, 0x90, 38).is_empty());

    // Feedback off for one assignment.
    let i = r.index_of(CtlKind::Cc { number: 7 });
    r.s.set_controller_feedback(i, false).unwrap();
    r.s.update_track_linked(a, TrackPatch { volume_db: Some(-20.0), ..Default::default() });
    r.s.idle();
    check("with Feedback off nothing is sent", of(&take(&r.rx), 0xB0, 7).is_empty());
    r.s.set_controller_feedback(i, true).unwrap();
    r.s.idle();
    check("turned back on, its value is sent", of(&take(&r.rx), 0xB0, 7) == vec![[0xB0, 7, cc_of(-20.0)]]);

    // Settings › MIDI: feedback for the input turned off, then the output switched off and on
    // again (as when it is unplugged and plugged back in): everything is sent anew.
    let mut prefs = r.s.prefs.clone();
    prefs.midi.feedback_outputs.insert(out.key.clone(), String::new());
    r.s.set_prefs(prefs.clone());
    r.s.update_track_linked(a, TrackPatch { volume_db: Some(-3.0), ..Default::default() });
    r.s.idle();
    check(
        &format!("no feedback when the input is set to none ({:?})", r.s.controller_assignments()[0].output),
        take(&r.rx).is_empty() && r.s.controller_assignments().iter().all(|row| row.output.is_none()),
    );
    prefs.midi.feedback_outputs.clear();
    prefs.midi.disabled_outputs = vec![out.name.clone()];
    r.s.set_prefs(prefs.clone());
    r.s.idle();
    check(
        "a switched-off output isn't connected",
        r.s.midi_outputs().iter().any(|p| p.name == out.name && !p.connected && !p.enabled) && take(&r.rx).is_empty(),
    );
    prefs.midi.disabled_outputs.clear();
    r.s.set_prefs(prefs);
    r.s.idle();
    let got = take(&r.rx);
    check(
        &format!("a reconnected output gets every assignment's value ({got:?})"),
        got.len() == 7
            && of(&got, 0xB0, 7) == vec![[0xB0, 7, cc_of(-3.0)]]
            && of(&got, 0x90, 36) == vec![[0x90, 36, 0]]
            && of(&got, 0xB0, 30) == vec![[0xB0, 30, 0]]
            && of(&got, 0xB0, 31) == vec![[0xB0, 31, 127]],
    );

    // Deleting an assignment stops its feedback.
    let i = r.index_of(CtlKind::Cc { number: 7 });
    r.s.delete_controller_assignment(i).unwrap();
    r.s.update_track_linked(a, TrackPatch { volume_db: Some(-9.0), ..Default::default() });
    r.s.idle();
    check("a deleted assignment gets no feedback", of(&take(&r.rx), 0xB0, 7).is_empty());

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
