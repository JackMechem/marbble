//! Exercises controller assignments (MIDI Learn) against a running engine (silent ALSA null
//! output), with controller messages fed in through the hardware input path: learning, levels
//! and plug-in parameters following, Pickup, ranges, buttons, notes and program changes with
//! Toggle, Momentary and Trigger, latching buttons, key commands passed to the window, relative
//! encoders, Selected Track targets, VCAs, 14-bit controllers and NRPNs, automation writing,
//! consumed messages and the saved list.
//! usage: controller_check <audio file> <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::control::controllers::{ButtonMode, CtlKind, CtlTarget, Encoding, SelectedParam, Takeover, TransportAction};
use daw_engine::model::{AutoMode, AutoParam, InputSel, TrackKind};
use daw_engine::session::{AssignmentPatch, TrackPatch};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// The port the messages pretend to come from, with the numbers ALSA appends.
const PORT: &str = "Test Controller:Test Controller MIDI 1 20:0";

/// Sends a control change on channel 1 and lets the session handle it, as `idle` does every
/// 30 ms in the app.
fn cc(s: &mut Session, number: u8, value: u8) {
    s.inject_midi_from(PORT, &[0xB0, number, value]);
    s.idle();
}

/// Sends any message (a note, a program change) and lets the session handle it.
fn send(s: &mut Session, msg: &[u8]) {
    s.inject_midi_from(PORT, msg);
    s.idle();
}

fn learn(s: &mut Session, target: CtlTarget, number: u8) -> Option<String> {
    learn_msg(s, target, &[0xB0, number, 64])
}

fn learn_msg(s: &mut Session, target: CtlTarget, msg: &[u8]) -> Option<String> {
    s.start_learn(Some(target)).unwrap();
    send(s, msg);
    learn_wait(s);
    s.tick().learned
}

/// A control change 0..31 may be the coarse part of a 14-bit pair: the learn waits a moment
/// for the fine part before taking it as a 7-bit controller.
fn learn_wait(s: &mut Session) {
    if s.learning().is_some() {
        std::thread::sleep(Duration::from_millis(150));
        s.idle();
    }
}

fn index_of(s: &Session, number: u8) -> usize {
    index_of_kind(s, CtlKind::Cc { number })
}

fn index_of_kind(s: &Session, kind: CtlKind) -> usize {
    s.controller_assignments().iter().position(|r| r.assignment.controller.kind == kind).expect("assignment")
}

/// Sets an assignment's takeover mode and range, as the window does.
fn set_range(s: &mut Session, i: usize, mode: Takeover, min: f32, max: f32) {
    s.update_controller_assignment(i, AssignmentPatch { mode: Some(mode), min: Some(min), max: Some(max), ..Default::default() }).unwrap();
}

fn set_button(s: &mut Session, i: usize, button: ButtonMode) {
    s.update_controller_assignment(i, AssignmentPatch { button_mode: Some(button), ..Default::default() }).unwrap();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = std::path::PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("ControllerCheck.marbble");
    let file = scratch.join("controller-assignments.json");
    s.load_controller_assignments(file.clone());

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.select_track(Some(inst));
    let comp = s.set_plugin(a, 0, "builtin:compressor").unwrap();
    let vol = |s: &Session| s.project.track(a).unwrap().volume_db;

    // Learning the last touched parameter.
    check("learning needs a touched parameter first", s.start_learn(None).is_err());
    s.touched_patch(a, &TrackPatch { volume_db: Some(0.0), ..Default::default() });
    let name = s.start_learn(None).unwrap();
    check(&format!("learn names the parameter ({name})"), name.ends_with("› Volume"));
    check("the tick shows the learn", s.tick().learn.as_deref() == Some(name.as_str()));
    let before = vol(&s);
    cc(&mut s, 7, 30);
    learn_wait(&mut s);
    let t = s.tick();
    check(&format!("a control change is learned ({:?})", t.learned), t.learned.is_some() && t.learn.is_none());
    check("the learning move doesn't act", vol(&s) == before);
    let rows = s.controller_assignments();
    let c = &rows[0].assignment.controller;
    check(
        &format!("assigned to the port without ALSA's numbers ({:?})", c),
        rows.len() == 1 && c.port == "Test Controller:Test Controller MIDI 1" && c.channel == 0 && c.kind == CtlKind::Cc { number: 7 },
    );

    // The fader follows, on its taper.
    cc(&mut s, 7, 127);
    check(&format!("CC 127 is +6 dB ({})", vol(&s)), (vol(&s) - 6.0).abs() < 0.01);
    cc(&mut s, 7, 0);
    check(&format!("CC 0 is -inf ({})", vol(&s)), vol(&s) <= -96.0);
    cc(&mut s, 7, 79);
    check(&format!("CC 79 is near -12 dB, like the fader ({})", vol(&s)), (vol(&s) + 12.0).abs() < 0.5);
    check("a controller move is an undo step", s.undo_label().as_deref() == Some("Volume"));

    // Assigned messages don't reach the instrument; others do.
    std::thread::sleep(Duration::from_millis(50));
    let seen = s.tick().midi_activity;
    for v in 0..20 {
        cc(&mut s, 7, 40 + v);
    }
    std::thread::sleep(Duration::from_millis(50));
    let after = s.tick().midi_activity;
    check(&format!("assigned controllers are consumed ({seen} -> {after})"), after == seen);
    s.inject_midi_from(PORT, &[0xB0, 1, 64]);
    s.inject_midi_from(PORT, &[0x90, 60, 100]);
    std::thread::sleep(Duration::from_millis(50));
    let after = s.tick().midi_activity;
    check(&format!("other messages still play the instrument ({seen} -> {after})"), after == seen + 2);
    // Another channel or port is another controller.
    s.inject_midi_from(PORT, &[0xB1, 7, 127]);
    s.inject_midi_from("Other Port 24:0", &[0xB0, 7, 127]);
    s.idle();
    std::thread::sleep(Duration::from_millis(50));
    check("another channel or port isn't assigned", s.tick().midi_activity == seen + 4 && vol(&s) < 0.0);

    // A built-in effect's parameter.
    let threshold = |s: &Session| s.builtin_params(comp).iter().find(|p| p.0 == 1).map(|p| p.1).unwrap();
    let learned = learn(&mut s, CtlTarget::Plugin { track: a, uid: comp, plugin: String::new(), param: 1 }, 20);
    check(&format!("a plug-in parameter is learned ({learned:?})"), learned.is_some_and(|l| l.contains("Threshold")));
    cc(&mut s, 20, 0);
    let lo = threshold(&s);
    cc(&mut s, 20, 127);
    let hi = threshold(&s);
    check(&format!("the plug-in parameter follows ({lo} .. {hi})"), lo <= -59.9 && hi >= -0.1);

    // Pickup: ignored until the controller reaches the fader.
    let i = index_of(&s, 7);
    set_range(&mut s, i, Takeover::Pickup, 0.0, 1.0);
    s.update_track_linked(a, TrackPatch { volume_db: Some(-12.0), ..Default::default() });
    cc(&mut s, 7, 10);
    cc(&mut s, 7, 50);
    check(&format!("pickup ignores the controller below the fader ({})", vol(&s)), vol(&s) == -12.0);
    cc(&mut s, 7, 100);
    let caught = vol(&s);
    check(&format!("pickup catches it on the way past ({caught})"), caught > -12.0);
    cc(&mut s, 7, 60);
    check(&format!("then it follows ({})", vol(&s)), vol(&s) < -12.0);
    s.update_track_linked(a, TrackPatch { volume_db: Some(0.0), ..Default::default() });
    cc(&mut s, 7, 61);
    check("moving the fader with the mouse lets go of it", vol(&s) == 0.0);

    // A range: the controller covers the middle of the pan knob's travel.
    learn(&mut s, CtlTarget::Pan { track: a }, 10);
    let i = index_of(&s, 10);
    set_range(&mut s, i, Takeover::Jump, 0.25, 0.75);
    cc(&mut s, 10, 127);
    let pan = s.project.track(a).unwrap().pan;
    check(&format!("the range limits the travel ({pan})"), (pan - 0.5).abs() < 0.02);

    // Buttons: a press toggles, the release does nothing.
    learn(&mut s, CtlTarget::Mute { track: a }, 30);
    let muted = |s: &Session| s.project.track(a).unwrap().mute;
    cc(&mut s, 30, 127);
    let on = muted(&s);
    cc(&mut s, 30, 0);
    let held = muted(&s);
    cc(&mut s, 30, 127);
    check(&format!("a mute button toggles on press ({on} {held} {})", muted(&s)), on && held && !muted(&s));
    learn(&mut s, CtlTarget::Transport { action: TransportAction::Cycle }, 31);
    let was = s.project.cycle.enabled;
    cc(&mut s, 31, 127);
    cc(&mut s, 31, 0);
    check("a cycle button toggles Cycle", s.project.cycle.enabled != was);
    learn(&mut s, CtlTarget::Transport { action: TransportAction::Play }, 32);
    s.set_cycle(true, 0, 48000);
    cc(&mut s, 32, 127);
    cc(&mut s, 32, 0);
    std::thread::sleep(Duration::from_millis(100));
    let playing = s.is_playing();
    cc(&mut s, 32, 127);
    cc(&mut s, 32, 0);
    std::thread::sleep(Duration::from_millis(100));
    check(&format!("a play button starts and stops ({playing} {})", s.is_playing()), playing && !s.is_playing());

    // Automation: in Touch, controller moves are written like fader moves.
    let i = index_of(&s, 7);
    set_range(&mut s, i, Takeover::Jump, 0.0, 1.0);
    s.set_auto_mode(a, AutoMode::Touch);
    s.locate(0);
    s.play();
    for v in [20u8, 40, 60, 80, 100] {
        cc(&mut s, 7, v);
        std::thread::sleep(Duration::from_millis(40));
        s.idle();
    }
    // Let go: the controller stops moving and its pass ends.
    std::thread::sleep(Duration::from_millis(700));
    s.idle();
    let lane = s.project.track(a).unwrap().automation.iter().find(|l| matches!(l.param, AutoParam::Volume)).map(|l| l.points.len()).unwrap_or(0);
    check(&format!("touch writes a volume lane after the controller stops ({lane} points)"), lane >= 2);
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    s.set_auto_mode(a, AutoMode::Read);

    // Notes: a pad on channel 10. Learning skips the release of a note already held.
    let soloed = |s: &Session| s.project.track(a).unwrap().solo;
    s.start_learn(Some(CtlTarget::Solo { track: a })).unwrap();
    send(&mut s, &[0x89, 37, 0]);
    check("a note's release isn't learned", s.tick().learn.is_some());
    send(&mut s, &[0x99, 36, 100]);
    let t = s.tick();
    check(&format!("a note is learned ({:?})", t.learned), t.learned.as_deref().is_some_and(|l| l.starts_with("Note C1, channel 10")));
    check("the learning press doesn't act", !soloed(&s));
    send(&mut s, &[0x89, 36, 0]);
    let pad = index_of_kind(&s, CtlKind::Note { number: 36 });
    let row = &s.controller_assignments()[pad];
    check(&format!("a pad on a switch toggles ({:?})", row.assignment.button_mode), row.assignment.button_mode == ButtonMode::Toggle);
    send(&mut s, &[0x99, 36, 100]);
    let on = soloed(&s);
    send(&mut s, &[0x89, 36, 0]);
    let held = soloed(&s);
    send(&mut s, &[0x99, 36, 100]);
    send(&mut s, &[0x99, 36, 0]);
    check(&format!("Toggle: a press flips, the release doesn't ({on} {held} {})", soloed(&s)), on && held && !soloed(&s));
    set_button(&mut s, pad, ButtonMode::Momentary);
    send(&mut s, &[0x99, 36, 100]);
    let on = soloed(&s);
    send(&mut s, &[0x89, 36, 0]);
    check(&format!("Momentary: on while held ({on} {})", soloed(&s)), on && !soloed(&s));
    set_button(&mut s, pad, ButtonMode::Trigger);
    send(&mut s, &[0x99, 36, 100]);
    send(&mut s, &[0x89, 36, 0]);
    let on = soloed(&s);
    send(&mut s, &[0x99, 36, 100]);
    send(&mut s, &[0x89, 36, 0]);
    check(&format!("Trigger: each press switches it on ({on} {})", soloed(&s)), on && soloed(&s));
    s.update_track_linked(a, TrackPatch { solo: Some(false), ..Default::default() });

    // Assigned notes don't play the instrument or get recorded; other notes do.
    std::thread::sleep(Duration::from_millis(50));
    let seen = s.tick().midi_activity;
    send(&mut s, &[0x99, 36, 90]);
    send(&mut s, &[0x89, 36, 0]);
    std::thread::sleep(Duration::from_millis(50));
    let after = s.tick().midi_activity;
    check(&format!("an assigned note is consumed ({seen} -> {after})"), after == seen);
    // Its polyphonic aftertouch too; another note's still plays.
    send(&mut s, &[0xA9, 36, 60]);
    std::thread::sleep(Duration::from_millis(50));
    let after = s.tick().midi_activity;
    check(&format!("an assigned note's aftertouch is consumed ({seen} -> {after})"), after == seen);
    s.inject_midi_from(PORT, &[0xA9, 38, 60]);
    std::thread::sleep(Duration::from_millis(50));
    let after = s.tick().midi_activity;
    check(&format!("another note's aftertouch still plays ({seen} -> {after})"), after == seen + 1);
    let seen = after;
    send(&mut s, &[0x99, 38, 90]);
    send(&mut s, &[0x89, 38, 0]);
    s.inject_midi_from(PORT, &[0x99, 38, 90]);
    std::thread::sleep(Duration::from_millis(50));
    check("another note still plays", s.tick().midi_activity > seen);

    // A pad on a fader: its velocity sets the level; without velocity it goes to the top of
    // the range; Momentary goes back to the bottom on release.
    learn_msg(&mut s, CtlTarget::Volume { track: a }, &[0x99, 40, 1]);
    send(&mut s, &[0x89, 40, 0]);
    let fader = index_of_kind(&s, CtlKind::Note { number: 40 });
    check("a pad on a fader triggers", s.controller_assignments()[fader].assignment.button_mode == ButtonMode::Trigger);
    send(&mut s, &[0x99, 40, 127]);
    let top = vol(&s);
    send(&mut s, &[0x89, 40, 0]);
    let after = vol(&s);
    send(&mut s, &[0x99, 40, 79]);
    let mid = vol(&s);
    check(&format!("the velocity sets the level ({top} {after} {mid})"), (top - 6.0).abs() < 0.01 && after == top && (mid + 12.0).abs() < 0.5);
    s.update_controller_assignment(
        fader,
        AssignmentPatch { velocity: Some(false), min: Some(0.5), max: Some(0.8), button_mode: Some(ButtonMode::Momentary), ..Default::default() },
    )
    .unwrap();
    send(&mut s, &[0x99, 40, 5]);
    let held = vol(&s);
    send(&mut s, &[0x89, 40, 0]);
    let released = vol(&s);
    let (hi, lo) = (daw_engine::control::controllers::fader_to_db(0.8), daw_engine::control::controllers::fader_to_db(0.5));
    check(
        &format!("a fixed value while held, the bottom of the range on release ({held} {released})"),
        (held - hi).abs() < 0.06 && (released - lo).abs() < 0.06,
    );
    set_button(&mut s, fader, ButtonMode::Toggle);
    send(&mut s, &[0x99, 40, 5]);
    send(&mut s, &[0x89, 40, 0]);
    let first = vol(&s);
    send(&mut s, &[0x99, 40, 5]);
    send(&mut s, &[0x89, 40, 0]);
    check(&format!("Toggle on a fader goes between the two ({first} {})", vol(&s)), (first - hi).abs() < 0.06 && (vol(&s) - lo).abs() < 0.06);

    // Program changes: a press each time, with no release.
    let was = s.project.cycle.enabled;
    let learned = learn_msg(&mut s, CtlTarget::Transport { action: TransportAction::Cycle }, &[0xC0, 4]);
    check(&format!("a program change is learned ({learned:?})"), learned.as_deref().is_some_and(|l| l.starts_with("Program 5, channel 1")));
    send(&mut s, &[0xC0, 4]);
    let once = s.project.cycle.enabled;
    send(&mut s, &[0xC0, 4]);
    check(&format!("each program change toggles ({was} {once} {})", s.project.cycle.enabled), once != was && s.project.cycle.enabled == was);
    send(&mut s, &[0xC0, 5]);
    check("another program isn't assigned", s.project.cycle.enabled == was);

    // A latching button (127 at one press, 0 at the next, nothing in between): found when no
    // release follows the learnt press within a second; then every message is a press.
    learn(&mut s, CtlTarget::Mute { track: a }, 70);
    std::thread::sleep(Duration::from_millis(1100));
    s.idle();
    let latch = index_of(&s, 70);
    check("a button with no release after its press is latching", s.controller_assignments()[latch].assignment.latching);
    cc(&mut s, 70, 127);
    let on = muted(&s);
    cc(&mut s, 70, 0);
    check(&format!("each of its messages toggles ({on} {})", muted(&s)), on && !muted(&s));
    learn(&mut s, CtlTarget::Solo { track: a }, 71);
    cc(&mut s, 71, 0);
    std::thread::sleep(Duration::from_millis(1100));
    s.idle();
    check("one that sends its release isn't", !s.controller_assignments()[index_of(&s, 71)].assignment.latching);
    s.update_track_linked(a, TrackPatch { solo: Some(false), mute: Some(false), ..Default::default() });

    // Momentary on Play: plays while the pad is held.
    learn_msg(&mut s, CtlTarget::Transport { action: TransportAction::Play }, &[0x99, 42, 100]);
    send(&mut s, &[0x89, 42, 0]);
    let play = index_of_kind(&s, CtlKind::Note { number: 42 });
    set_button(&mut s, play, ButtonMode::Momentary);
    send(&mut s, &[0x99, 42, 100]);
    std::thread::sleep(Duration::from_millis(100));
    let playing = s.is_playing();
    send(&mut s, &[0x89, 42, 0]);
    std::thread::sleep(Duration::from_millis(100));
    check(&format!("Momentary Play plays while held ({playing} {})", s.is_playing()), playing && !s.is_playing());

    // Key commands: the session passes a fired command's id to the window in the tick.
    let next = || CtlTarget::Command { id: "next-marker".into() };
    s.start_learn_named(Some(next()), Some("Key Command › Go to Next Marker".into())).unwrap();
    send(&mut s, &[0x99, 44, 100]);
    let t = s.tick();
    check(
        &format!("a key command is learned, by the name the window gave ({:?})", t.learned),
        t.learned.as_deref() == Some("Note G♯1, channel 10 assigned to Key Command › Go to Next Marker"),
    );
    check("the learning press doesn't fire it", t.commands.is_empty());
    send(&mut s, &[0x89, 44, 0]);
    let cmd = index_of_kind(&s, CtlKind::Note { number: 44 });
    let row = &s.controller_assignments()[cmd];
    check(
        &format!("a command triggers ({:?}, {})", row.assignment.button_mode, row.name),
        row.assignment.button_mode == ButtonMode::Trigger && row.button && row.available,
    );
    let seen = s.tick().midi_activity;
    send(&mut s, &[0x99, 44, 100]);
    send(&mut s, &[0x89, 44, 0]);
    send(&mut s, &[0x99, 44, 100]);
    send(&mut s, &[0x89, 44, 0]);
    let t = s.tick();
    check(&format!("each press fires it once ({:?})", t.commands), t.commands == ["next-marker", "next-marker"]);
    check("and once only", s.tick().commands.is_empty());
    std::thread::sleep(Duration::from_millis(50));
    check("a command's note doesn't play the instrument", s.tick().midi_activity == seen);
    set_button(&mut s, cmd, ButtonMode::Momentary);
    send(&mut s, &[0x99, 44, 100]);
    let pressed = s.tick().commands;
    send(&mut s, &[0x89, 44, 0]);
    let released = s.tick().commands;
    check(&format!("Momentary fires on the press and the release ({pressed:?} {released:?})"), pressed == ["next-marker"] && released == ["next-marker"]);
    // A control change fires on going above 63, once until it comes back down.
    s.start_learn_named(Some(CtlTarget::Command { id: "undo".into() }), Some("Key Command › Undo".into())).unwrap();
    cc(&mut s, 60, 127);
    learn_wait(&mut s);
    s.tick();
    cc(&mut s, 60, 0);
    cc(&mut s, 60, 127);
    cc(&mut s, 60, 127);
    cc(&mut s, 60, 0);
    let t = s.tick();
    check(&format!("a button sending CCs fires on its press ({:?})", t.commands), t.commands == ["undo"]);
    for id in ["next-marker", "undo"] {
        let i = s.controller_assignments().iter().position(|r| r.assignment.target == CtlTarget::Command { id: id.into() }).unwrap();
        s.delete_controller_assignment(i).unwrap();
    }

    // Relative encoders: steps move the parameter from where it is, and add up within a batch.
    let pan = |s: &Session, t| s.project.track(t).unwrap().pan;
    learn(&mut s, CtlTarget::Pan { track: a }, 16);
    let enc = index_of(&s, 16);
    s.update_controller_assignment(enc, AssignmentPatch { encoding: Some(Encoding::TwosComplement), ..Default::default() }).unwrap();
    s.update_track_linked(a, TrackPatch { pan: Some(0.0), ..Default::default() });
    for _ in 0..4 {
        s.inject_midi_from(PORT, &[0xB0, 16, 2]);
    }
    s.idle();
    let up = pan(&s, a);
    check(&format!("an encoder's steps add up (8 steps: {up})"), (up - 16.0 / 127.0).abs() < 1.0 / 64.0);
    cc(&mut s, 16, 127);
    cc(&mut s, 16, 120);
    let down = pan(&s, a);
    check(&format!("two's complement turns down ({up} -> {down})"), down < up && (down - (up - 18.0 / 127.0)).abs() < 1.0 / 64.0);
    s.update_controller_assignment(enc, AssignmentPatch { encoding: Some(Encoding::BinaryOffset), sensitivity: Some(4.0), ..Default::default() }).unwrap();
    s.update_track_linked(a, TrackPatch { pan: Some(0.0), ..Default::default() });
    cc(&mut s, 16, 66);
    check(&format!("binary offset with sensitivity 4 ({})", pan(&s, a)), (pan(&s, a) - 16.0 / 127.0).abs() < 1.0 / 64.0);
    s.update_controller_assignment(enc, AssignmentPatch { encoding: Some(Encoding::SignMagnitude), sensitivity: Some(1.0), ..Default::default() }).unwrap();
    for _ in 0..200 {
        s.inject_midi_from(PORT, &[0xB0, 16, 0x40 | 10]);
    }
    s.idle();
    check(&format!("sign-magnitude turns down and stops at the end ({})", pan(&s, a)), pan(&s, a) == -1.0);
    s.update_track_linked(a, TrackPatch { pan: Some(0.0), ..Default::default() });

    // Selected Track targets follow the selection, and do nothing without one.
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let vol_of = |s: &Session, t| s.project.track(t).unwrap().volume_db;
    let name = learn(&mut s, CtlTarget::Selected { param: SelectedParam::Volume }, 17);
    check(&format!("a Selected Track target is learned ({name:?})"), name.is_some_and(|n| n.ends_with("Selected Track › Volume")));
    s.select_track(Some(a));
    cc(&mut s, 17, 127);
    s.select_track(Some(b));
    cc(&mut s, 17, 0);
    check(&format!("it moves the selected track's fader ({} {})", vol_of(&s, a), vol_of(&s, b)), vol_of(&s, a) == 6.0 && vol_of(&s, b) <= -96.0);
    s.select_track(None);
    cc(&mut s, 17, 79);
    check("with nothing selected it does nothing", vol_of(&s, a) == 6.0 && vol_of(&s, b) <= -96.0);
    learn_msg(&mut s, CtlTarget::Selected { param: SelectedParam::Mute }, &[0x99, 50, 100]);
    send(&mut s, &[0x89, 50, 0]);
    s.select_track(Some(b));
    send(&mut s, &[0x99, 50, 100]);
    send(&mut s, &[0x89, 50, 0]);
    check("Selected Track › Mute mutes the selected track", s.project.track(b).unwrap().mute && !s.project.track(a).unwrap().mute);
    learn_msg(&mut s, CtlTarget::Selected { param: SelectedParam::Bypass { slot: 0 } }, &[0x99, 51, 100]);
    send(&mut s, &[0x89, 51, 0]);
    s.select_track(Some(a));
    send(&mut s, &[0x99, 51, 100]);
    send(&mut s, &[0x89, 51, 0]);
    let bypassed = |s: &Session| s.project.track(a).unwrap().inserts[0].as_ref().unwrap().bypass;
    check("Selected Track › Insert 1 Bypass bypasses its compressor", bypassed(&s));
    send(&mut s, &[0x99, 51, 100]);
    send(&mut s, &[0x89, 51, 0]);
    check("and enables it again", !bypassed(&s));
    // The focused plug-in's parameters by position.
    let first = s.plugin_params(comp)[0].id;
    let p0 = |s: &Session| s.builtin_params(comp).iter().find(|p| p.0 == first).map(|p| p.1).unwrap();
    learn(&mut s, CtlTarget::Selected { param: SelectedParam::PluginParam { index: 0 } }, 18);
    s.focus_plugin(comp);
    cc(&mut s, 18, 0);
    let lo = p0(&s);
    cc(&mut s, 18, 127);
    let hi = p0(&s);
    check(&format!("Selected Track › Plug-in Parameter 1 moves the focused plug-in's first parameter ({lo} .. {hi})"), hi > lo);
    s.select_track(Some(b));
    cc(&mut s, 18, 0);
    check("a track without plug-ins leaves it alone", p0(&s) == hi);
    s.update_track_linked(b, TrackPatch { mute: Some(false), ..Default::default() });

    // A VCA fader (#63).
    let vca = s.create_vca(&[b]);
    let level = |s: &Session| s.vca(vca).unwrap().level_db;
    let name = learn(&mut s, CtlTarget::VcaLevel { vca }, 21);
    check(&format!("a VCA's level is learned ({name:?})"), name.is_some_and(|n| n.ends_with("› Level")));
    cc(&mut s, 21, 127);
    let top = level(&s);
    cc(&mut s, 21, 0);
    check(&format!("the VCA fader follows ({top} {})", level(&s)), top == 6.0 && level(&s) <= -96.0);
    check(&format!("as an undo step ({:?})", s.undo_label()), s.undo_label().as_deref() == Some("VCA Level"));
    learn_msg(&mut s, CtlTarget::VcaMute { vca }, &[0x99, 52, 100]);
    send(&mut s, &[0x89, 52, 0]);
    send(&mut s, &[0x99, 52, 100]);
    check("a VCA's mute button", s.vca(vca).unwrap().mute);
    send(&mut s, &[0x89, 52, 0]);

    // 14-bit controllers: learned from the pair, fine steps between coarse ones.
    s.start_learn(Some(CtlTarget::Volume { track: b })).unwrap();
    s.inject_midi_from(PORT, &[0xB0, 1, 100]);
    s.inject_midi_from(PORT, &[0xB0, 33, 0]);
    s.idle();
    let t = s.tick();
    check(&format!("a 14-bit pair is learned as one controller ({:?})", t.learned), t.learned.as_deref().is_some_and(|l| l.starts_with("CC 1/33 (14-bit)")));
    std::thread::sleep(Duration::from_millis(50));
    let seen = s.tick().midi_activity;
    s.inject_midi_from(PORT, &[0xB0, 1, 120]);
    s.inject_midi_from(PORT, &[0xB0, 33, 0]);
    s.idle();
    let coarse = vol_of(&s, b);
    s.inject_midi_from(PORT, &[0xB0, 1, 120]);
    s.inject_midi_from(PORT, &[0xB0, 33, 127]);
    s.idle();
    let fine = vol_of(&s, b);
    check(
        &format!("the fine part moves it between coarse steps ({coarse} < {fine})"),
        fine > coarse && fine < daw_engine::control::controllers::fader_to_db(121.0 / 127.0),
    );
    std::thread::sleep(Duration::from_millis(50));
    check("both parts are taken off the live path", s.tick().midi_activity == seen);
    // A controller in 0..31 that sends no fine part is learned as an ordinary one.
    s.start_learn(Some(CtlTarget::Pan { track: b })).unwrap();
    cc(&mut s, 2, 64);
    check("a coarse part alone waits", s.tick().learn.is_some());
    std::thread::sleep(Duration::from_millis(150));
    s.idle();
    let t = s.tick();
    check(&format!("then is a 7-bit controller ({:?})", t.learned), t.learned.as_deref().is_some_and(|l| l.starts_with("CC 2, channel 1")));

    // NRPN: number 130 (1, 2), data entry MSB and LSB.
    let nrpn = |s: &mut Session, msb: u8, lsb: u8| {
        for d in [[0xB0, 99, 1], [0xB0, 98, 2], [0xB0, 6, msb], [0xB0, 38, lsb]] {
            s.inject_midi_from(PORT, &d);
        }
        s.idle();
    };
    s.start_learn(Some(CtlTarget::Pan { track: b })).unwrap();
    nrpn(&mut s, 64, 0);
    let t = s.tick();
    check(&format!("an NRPN is learned ({:?})", t.learned), t.learned.as_deref().is_some_and(|l| l.starts_with("NRPN 130")));
    nrpn(&mut s, 127, 127);
    let right = pan(&s, b);
    nrpn(&mut s, 0, 0);
    check(&format!("the NRPN moves the pan ({right} {})", pan(&s, b)), right == 1.0 && pan(&s, b) == -1.0);
    // Data increment and decrement step it, 1/127 of its travel each.
    let step = |s: &mut Session, cc: u8, times: usize| {
        for _ in 0..times {
            s.inject_midi_from(PORT, &[0xB0, cc, 0]);
        }
        s.idle();
    };
    step(&mut s, 96, 4);
    let up = pan(&s, b);
    step(&mut s, 97, 1);
    let down = pan(&s, b);
    check(
        &format!("data increment and decrement step the NRPN's pan ({up} {down})"),
        (up - (-1.0 + 8.0 / 127.0)).abs() < 0.02 && (down - (up - 2.0 / 127.0)).abs() < 0.02,
    );
    std::thread::sleep(Duration::from_millis(50));
    let seen = s.tick().midi_activity;
    nrpn(&mut s, 64, 0);
    std::thread::sleep(Duration::from_millis(50));
    check("its messages are taken off the live path", s.tick().midi_activity == seen);

    // A target that isn't in the project is shown as missing and does nothing.
    learn(&mut s, CtlTarget::Volume { track: inst }, 40);
    s.delete_track(inst);
    let row = s.controller_assignments().into_iter().find(|r| r.assignment.controller.kind == CtlKind::Cc { number: 40 }).unwrap();
    check(&format!("a deleted track's assignment is unavailable ({})", row.name), !row.available);
    let before = vol(&s);
    cc(&mut s, 40, 127);
    check("and ignored", vol(&s) == before);

    // The list is saved, and loads back.
    let rows = s.controller_assignments();
    let saved = std::fs::read_to_string(&file).unwrap_or_default();
    s.load_controller_assignments(file.clone());
    let again = s.controller_assignments();
    check(
        &format!("assignments are saved and load back ({} in the file)", saved.matches("\"controller\"").count()),
        again.len() == rows.len() && again.iter().zip(&rows).all(|(x, y)| x.assignment == y.assignment),
    );
    check("notes, program changes and button modes are saved", saved.contains("\"note\"") && saved.contains("\"program\"") && saved.contains("\"momentary\""));
    check(
        "selected-track, VCA, encoder, 14-bit and NRPN assignments are saved",
        ["\"selected\"", "\"vca_level\"", "\"sign_magnitude\"", "\"cc14\"", "\"nrpn\""].iter().all(|k| saved.contains(k)),
    );
    // A file from before notes were learnable loads with today's defaults.
    let old = scratch.join("old-assignments.json");
    std::fs::write(&old, r#"{"assignments":[{"controller":{"port":"P","channel":0,"kind":{"type":"cc","number":7}},"target":{"type":"mute","track":3},"mode":"pickup","min":0.0,"max":1.0}]}"#).unwrap();
    s.load_controller_assignments(old);
    let rows_old = s.controller_assignments();
    check(
        "an older assignment file loads",
        rows_old.len() == 1
            && rows_old[0].assignment.button_mode == ButtonMode::Toggle
            && rows_old[0].assignment.mode == Takeover::Pickup
            && rows_old[0].assignment.velocity,
    );
    s.load_controller_assignments(file.clone());
    let i = index_of(&s, 40);
    s.delete_controller_assignment(i).unwrap();
    check("delete removes one", s.controller_assignments().len() == rows.len() - 1);
    // A deleted assignment's controller plays the instrument again.
    let seen = s.tick().midi_activity;
    s.inject_midi_from(PORT, &[0xB0, 40, 1]);
    std::thread::sleep(Duration::from_millis(50));
    check("a deleted assignment's controller isn't consumed", s.tick().midi_activity == seen + 1);

    // Cancelling a learn assigns nothing and lets controllers through again.
    let n = s.controller_assignments().len();
    s.start_learn(Some(CtlTarget::MasterVolume)).unwrap();
    s.cancel_learn();
    cc(&mut s, 50, 100);
    check("cancel assigns nothing", s.controller_assignments().len() == n && s.tick().learn.is_none());

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
