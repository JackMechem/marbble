//! Drives simulated control surfaces through virtual MIDI ports (ALSA sequencer ports on Linux):
//! a Mackie Control with an Extender, then a HUI. Each simulated unit opens a virtual output
//! (which the session connects to as an input) and a virtual input (connected as an output) of
//! one name, answers the handshake (the device query and the challenge; the HUI's pings), and
//! keeps what it is sent: fader positions, lights, V-Pot rings, the LCD or scribble strips, the
//! time display.
//!
//! It checks the handshake and what a unit coming online gets, the LCD's names and values, faders
//! both ways (a mouse move drives the motor, a fader move sets the volume, an undo, automation
//! playback), that a touched fader isn't moved, the strip buttons and their lights, banking across
//! the two units, the V-Pots and their rings, the assignment and time displays, the jog wheel, the
//! master fader, F1 and Marker running key commands, and a unit that restarts getting everything
//! again. Then the same for a HUI: pings, scribble strips, faders, touch, switches, banking, the
//! time display, and going offline when it stops answering.
//! Prints SKIP when the system has no MIDI sequencer or virtual ports (Windows, CI runners without
//! the ALSA sequencer).
//! usage: surface_check <scratch dir>
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::control::controllers::db_to_fader;
use daw_engine::control::surface::{mcu, SurfaceConfig, SurfaceKind};
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, Id, InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;
use parking_lot::Mutex;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// The challenge the simulated Mackie Controls send.
const CHALLENGE: [u8; 4] = [0x10, 0x2A, 0x05, 0x33];

/// What a simulated unit was sent, as it shows it.
struct Model {
    hui: bool,
    device: u8,
    /// It answers the device query (a Mackie Control) or pings (a HUI).
    answer: bool,
    queries: usize,
    pings: usize,
    /// The host's answer to the challenge was right.
    verified: Option<bool>,
    faders: [Option<u16>; 9],
    /// Mackie Control: lights by note (velocity). HUI: lights by zone and port.
    notes: HashMap<u8, u8>,
    leds: HashMap<(u8, u8), bool>,
    rings: [Option<u8>; 8],
    /// The LCD (112 characters), or the HUI's nine scribble strips (36).
    lcd: Vec<u8>,
    /// Mackie Control: the digits' CC values, rightmost first. HUI: the time display's message.
    digits: [Option<u8>; 12],
    hui_time: Option<Vec<u8>>,
    /// Every message, for counting.
    count: usize,
    fader_msgs: usize,
    zone: u8,
    coarse: [u8; 8],
}

impl Model {
    fn new(hui: bool, device: u8) -> Model {
        Model {
            hui,
            device,
            answer: true,
            queries: 0,
            pings: 0,
            verified: None,
            faders: [None; 9],
            notes: HashMap::new(),
            leds: HashMap::new(),
            rings: [None; 8],
            lcd: vec![b' '; if hui { 36 } else { 112 }],
            digits: [None; 12],
            hui_time: None,
            count: 0,
            fader_msgs: 0,
            zone: 0,
            coarse: [0; 8],
        }
    }

    /// Takes one message; returns the answer to send, if any.
    fn feed(&mut self, m: &[u8]) -> Option<Vec<u8>> {
        self.count += 1;
        if self.hui {
            return self.feed_hui(m);
        }
        match *m {
            [s, lsb, msb] if s & 0xF0 == 0xE0 && (s & 0x0F) < 9 => {
                self.faders[(s & 0x0F) as usize] = Some((msb as u16) << 7 | lsb as u16);
                self.fader_msgs += 1;
            }
            [0x90, n, v] => {
                self.notes.insert(n, v);
            }
            [0xB0, cc @ 0x30..=0x37, v] => self.rings[(cc - 0x30) as usize] = Some(v),
            [0xB0, cc @ 0x40..=0x4B, v] => self.digits[(cc - 0x40) as usize] = Some(v),
            _ if m.len() > 6 && m[..4] == [0xF0, 0, 0, 0x66] && m[4] == self.device => {
                let body = &m[6..m.len() - 1];
                match m[5] {
                    0x00 => {
                        self.queries += 1;
                        if self.answer {
                            let mut hello = vec![0xF0, 0, 0, 0x66, self.device, 0x01, 1, 2, 3, 4, 5, 6, 7];
                            hello.extend(CHALLENGE);
                            hello.push(0xF7);
                            return Some(hello);
                        }
                    }
                    0x02 if body.len() == 11 => {
                        self.verified = Some(body[..7] == [1, 2, 3, 4, 5, 6, 7] && body[7..] == mcu::challenge_response(CHALLENGE));
                        return Some(vec![0xF0, 0, 0, 0x66, self.device, 0x03, 1, 2, 3, 4, 5, 6, 7, 0xF7]);
                    }
                    0x12 if !body.is_empty() => {
                        let at = body[0] as usize;
                        for (i, c) in body[1..].iter().enumerate() {
                            if let Some(slot) = self.lcd.get_mut(at + i) {
                                *slot = *c;
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }

    fn feed_hui(&mut self, m: &[u8]) -> Option<Vec<u8>> {
        match *m {
            [0x90, 0, 0] => {
                self.pings += 1;
                return self.answer.then(|| vec![0x90, 0, 0x7F]);
            }
            [0xB0, 0x0C, z] => self.zone = z,
            [0xB0, 0x2C, p] => {
                self.leds.insert((self.zone, p & 0x0F), p & 0x40 != 0);
            }
            [0xB0, cc @ 0x00..=0x07, v] => self.coarse[cc as usize] = v,
            [0xB0, cc @ 0x20..=0x27, v] => {
                let s = (cc - 0x20) as usize;
                self.faders[s] = Some((self.coarse[s] as u16) << 7 | v as u16);
                self.fader_msgs += 1;
            }
            [0xB0, cc @ 0x10..=0x17, v] => self.rings[(cc - 0x10) as usize] = Some(v),
            _ if m.len() > 7 && m[..6] == [0xF0, 0, 0, 0x66, 0x05, 0x00] => match m[6] {
                0x10 if m.len() == 13 => {
                    let s = m[7] as usize;
                    self.lcd[s * 4..s * 4 + 4].copy_from_slice(&m[8..12]);
                }
                0x11 => self.hui_time = Some(m[7..m.len() - 1].to_vec()),
                _ => {}
            },
            _ => {}
        }
        None
    }

    fn line(&self, n: usize) -> String {
        String::from_utf8_lossy(&self.lcd[n * 56..n * 56 + 56]).into_owned()
    }

    /// A strip's six characters on a line.
    fn cell(&self, line: usize, strip: usize) -> String {
        String::from_utf8_lossy(&self.lcd[line * 56 + strip * 7..line * 56 + strip * 7 + 6]).into_owned()
    }

    fn scribble(&self, strip: usize) -> String {
        String::from_utf8_lossy(&self.lcd[strip * 4..strip * 4 + 4]).into_owned()
    }

    /// The time display as text, leftmost first, with its dots.
    fn time(&self) -> String {
        let mut s = String::new();
        for d in self.digits[..10].iter().rev() {
            let v = d.unwrap_or(0x20);
            let ch = match v & 0x3F {
                c @ 0x20..=0x3F => c as char,
                c => (c + 0x40) as char,
            };
            s.push(ch);
            if v & 0x40 != 0 {
                s.push('.');
            }
        }
        s
    }

    /// The assignment display's two characters.
    fn assignment(&self) -> String {
        [11, 10]
            .iter()
            .map(|&i| match self.digits[i].unwrap_or(0x20) & 0x3F {
                c @ 0x20..=0x3F => c as char,
                c => (c + 0x40) as char,
            })
            .collect()
    }

    fn lit(&self, note: u8) -> bool {
        self.notes.get(&note).is_some_and(|v| *v > 0)
    }
}

/// A simulated unit on virtual ports named `name`. Kept open while the value lives. (`m()` locks
/// its model: never twice in one argument list or tuple, whose temporaries live together.)
struct Sim {
    model: Arc<Mutex<Model>>,
    out: Arc<Mutex<Option<midir::MidiOutputConnection>>>,
    _input: Box<dyn std::any::Any>,
    key: String,
}

impl Sim {
    #[cfg(unix)]
    fn open(name: &str, hui: bool, device: u8) -> Result<Sim, String> {
        use midir::os::unix::{VirtualInput, VirtualOutput};
        let model = Arc::new(Mutex::new(Model::new(hui, device)));
        let out: Arc<Mutex<Option<midir::MidiOutputConnection>>> = Arc::default();
        let output = midir::MidiOutput::new(name).map_err(|e| format!("no MIDI system ({e})"))?;
        *out.lock() = Some(output.create_virtual(name).map_err(|e| format!("could not open a virtual MIDI port ({e})"))?);
        let mut input = midir::MidiInput::new(name).map_err(|e| format!("no MIDI system ({e})"))?;
        input.ignore(midir::Ignore::None);
        let (m, o) = (model.clone(), out.clone());
        let conn = input
            .create_virtual(
                name,
                move |_, msg, _| {
                    let answer = m.lock().feed(msg);
                    if let (Some(a), Some(c)) = (answer, o.lock().as_mut()) {
                        let _ = c.send(&a);
                    }
                },
                (),
            )
            .map_err(|e| format!("could not open a virtual MIDI port ({e})"))?;
        Ok(Sim { model, out, _input: Box::new(conn), key: format!("{name}:{name}") })
    }

    #[cfg(not(unix))]
    fn open(_: &str, _: bool, _: u8) -> Result<Sim, String> {
        Err("no virtual MIDI ports on this system".into())
    }

    fn send(&self, msg: &[u8]) {
        if let Some(c) = self.out.lock().as_mut() {
            let _ = c.send(msg);
        }
    }

    /// A button pressed and let go (a Mackie Control's note).
    fn press(&self, note: u8) {
        self.send(&[0x90, note, 0x7F]);
        self.send(&[0x90, note, 0x00]);
    }

    /// A HUI switch pressed and let go.
    fn press_hui(&self, zone: u8, port: u8) {
        self.send(&[0xB0, 0x0F, zone]);
        self.send(&[0xB0, 0x2F, 0x40 | port]);
        self.send(&[0xB0, 0x0F, zone]);
        self.send(&[0xB0, 0x2F, port]);
    }

    fn m(&self) -> parking_lot::MutexGuard<'_, Model> {
        self.model.lock()
    }
}

/// The session's idle tick every 30 ms for `ms`, as the app runs it, with time for the
/// sequencer to deliver both ways.
fn run(s: &mut Session, ms: u64) {
    let t0 = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(30));
        s.idle();
        if t0.elapsed() >= Duration::from_millis(ms) {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(40));
    s.idle();
    std::thread::sleep(Duration::from_millis(40));
}

/// A fader's 14-bit position for a level.
fn pos_of(db: f32) -> u16 {
    (db_to_fader(db) * 16383.0).round() as u16
}

fn near(a: Option<u16>, b: u16) -> bool {
    a.is_some_and(|a| a.abs_diff(b) <= 1)
}

fn main() {
    let scratch = std::path::PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let (main_unit, ext, hui) = match (
        Sim::open("Surface Check MCU", false, mcu::MAIN),
        Sim::open("Surface Check XT", false, mcu::EXTENDER),
        Sim::open("Surface Check HUI", true, 0),
    ) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            println!("SKIP {e}");
            return;
        }
    };

    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("SurfaceCheck.marbble");
    s.load_controller_assignments(scratch.join("controller-assignments.json"));
    s.refresh_midi();
    let listed =
        |s: &Session, key: &str| s.midi_ports().iter().any(|p| p.key == key && p.connected) && s.midi_outputs().iter().any(|p| p.key == key && p.connected);
    if !listed(&s, &main_unit.key) || !listed(&s, &hui.key) {
        println!("SKIP the virtual ports aren't listed");
        return;
    }
    check("the simulated units' ports are connected both ways", listed(&s, &ext.key));

    // Twenty tracks, each at its own level.
    let tracks: Vec<Id> = (0..20)
        .map(|i| {
            let id = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
            let name = if i == 1 { "Bass Guitar".to_string() } else { format!("Ch{:02}", i + 1) };
            s.update_track(id, TrackPatch { name: Some(name), volume_db: Some(-(i as f32) - 1.0), ..Default::default() });
            id
        })
        .collect();
    let vol = |s: &Session, i: usize| s.project.track(tracks[i]).unwrap().volume_db;

    // Settings › MIDI › Control Surfaces: a Mackie Control, an Extender to its right.
    let unit = |kind, sim: &Sim, offset, functions: Vec<String>| SurfaceConfig { kind, input: sim.key.clone(), output: sim.key.clone(), offset, functions };
    let mut prefs = s.prefs.clone();
    prefs.midi.surfaces = vec![unit(SurfaceKind::Mcu, &main_unit, 0, vec!["next-marker".into()]), unit(SurfaceKind::McuExtender, &ext, 8, vec![])];
    s.set_prefs(prefs.clone());
    run(&mut s, 300);

    // The handshake.
    let st = s.control_surfaces();
    check(
        &format!("both units are asked who they are, answer, and are online ({} and {} queries, {st:?})", main_unit.m().queries, ext.m().queries),
        main_unit.m().queries >= 1 && ext.m().queries >= 1 && st.len() == 2 && st.iter().all(|u| u.online && u.input && u.output),
    );
    check(
        &format!("the host answers each unit's challenge rightly ({:?}, {:?})", main_unit.m().verified, ext.m().verified),
        main_unit.m().verified == Some(true) && ext.m().verified == Some(true),
    );
    check(
        &format!("the units show tracks 1–8 and 9–16 ({:?})", st.iter().map(|u| &u.showing).collect::<Vec<_>>()),
        st[0].showing == "Tracks 1–8" && st[1].showing == "Tracks 9–16",
    );

    // The LCD: names on top, pan below.
    let top = main_unit.m().line(0);
    check(&format!("the LCD names the strips' tracks ({top:?})"), top.starts_with("Ch01   BssGtr Ch03   Ch04") && top.trim_end().ends_with("Ch08"));
    check(
        &format!("the Extender's LCD names tracks 9–16 ({:?})", ext.m().line(0)),
        ext.m().line(0).starts_with("Ch09   Ch10") && ext.m().line(0).trim_end().ends_with("Ch16"),
    );
    check(
        &format!("the bottom line shows each pan ({:?})", main_unit.m().line(1)),
        main_unit.m().cell(1, 0) == "  C   " && main_unit.m().cell(1, 7) == "  C   ",
    );

    // Faders: each at its track's level, the master at Stereo Out's.
    let faders_ok = (0..8).all(|i| near(main_unit.m().faders[i], pos_of(vol(&s, i))) && near(ext.m().faders[i], pos_of(vol(&s, i + 8))));
    check(&format!("the faders show the tracks' levels ({:?})", main_unit.m().faders), faders_ok);
    check("the master fader shows Stereo Out's", near(main_unit.m().faders[8], pos_of(s.project.master.volume_db)));
    check(&format!("the Extender has no master fader ({:?})", ext.m().faders[8]), ext.m().faders[8].is_none());
    check(&format!("the assignment display says Pan ({:?})", main_unit.m().assignment()), main_unit.m().assignment() == "PN");
    check("Pan's button is lit, Stop's too (stopped)", main_unit.m().lit(0x2A) && main_unit.m().lit(0x5D) && !main_unit.m().lit(0x5E));

    // The mouse moves a fader: the motor follows.
    s.update_track_linked(tracks[0], TrackPatch { volume_db: Some(-20.0), ..Default::default() });
    run(&mut s, 60);
    check(&format!("a mouse move drives the fader ({:?})", main_unit.m().faders[0]), near(main_unit.m().faders[0], pos_of(-20.0)));

    // The fader moves the track: an undo step, and the motor isn't sent back what it sent.
    let before = main_unit.m().fader_msgs;
    let n = pos_of(-6.0);
    main_unit.send(&[0xE0, (n & 0x7F) as u8, (n >> 7) as u8]);
    run(&mut s, 60);
    check(&format!("a fader move sets the volume ({} dB)", vol(&s, 0)), (vol(&s, 0) + 6.0).abs() < 0.05);
    check(&format!("the LCD shows the level while it moves ({:?})", main_unit.m().cell(1, 0)), main_unit.m().cell(1, 0).trim() == "-6.0");
    run(&mut s, 600);
    check(&format!("nothing is sent back to a fader that moved ({} messages)", main_unit.m().fader_msgs - before), main_unit.m().fader_msgs == before);
    s.undo();
    run(&mut s, 60);
    check(
        &format!("undo puts the fader back ({} dB, {:?})", vol(&s, 0), main_unit.m().faders[0]),
        vol(&s, 0) == -20.0 && near(main_unit.m().faders[0], pos_of(-20.0)),
    );

    // Touch: nothing moves a touched fader; once let go, it goes where its track's level is.
    main_unit.send(&[0x90, 0x68, 0x7F]);
    run(&mut s, 60);
    let before = main_unit.m().fader_msgs;
    s.update_track_linked(tracks[0], TrackPatch { volume_db: Some(-30.0), ..Default::default() });
    run(&mut s, 200);
    check(&format!("a touched fader isn't moved ({} messages)", main_unit.m().fader_msgs - before), main_unit.m().fader_msgs == before);
    main_unit.send(&[0x90, 0x68, 0x00]);
    run(&mut s, 60);
    check(&format!("let go, it goes to its track's level ({:?})", main_unit.m().faders[0]), near(main_unit.m().faders[0], pos_of(-30.0)));
    let cell = main_unit.m().cell(1, 0);
    check(&format!("the LCD shows its level a moment after it is let go ({cell:?})"), cell.trim() == "-30.0");
    run(&mut s, 1600);
    let cell = main_unit.m().cell(1, 0);
    check(&format!("then the pan again ({cell:?})"), cell == "  C   ");

    // Automation: in Read, the fader follows the lane while playing.
    s.set_automation_lane(
        tracks[2],
        AutoParam::Volume,
        vec![AutoPoint { pos: 0, value: -40.0, curve: 0.0 }, AutoPoint { pos: 48000 * 600, value: -40.0, curve: 0.0 }],
    );
    s.set_auto_mode(tracks[2], AutoMode::Read);
    s.locate(0);
    s.play();
    run(&mut s, 150);
    check(&format!("in Read the fader follows the automation ({:?})", main_unit.m().faders[2]), near(main_unit.m().faders[2], pos_of(-40.0)));
    s.stop();
    run(&mut s, 60);
    check(&format!("stopped, it shows the track's own level ({:?})", main_unit.m().faders[2]), near(main_unit.m().faders[2], pos_of(vol(&s, 2))));
    s.set_auto_mode(tracks[2], AutoMode::Off);

    // Strip buttons and their lights.
    main_unit.press(0x10); // Mute 1
    main_unit.press(0x09); // Solo 2
    main_unit.press(0x02); // Rec 3
    main_unit.press(0x1B); // Select 4
    run(&mut s, 60);
    let t = |s: &Session, i: usize| s.project.track(tracks[i]).unwrap().clone();
    check("Mute, Solo, Rec and Select act on their strips' tracks", t(&s, 0).mute && t(&s, 1).solo && t(&s, 2).armed && s.selected == Some(tracks[3]));
    {
        let m = main_unit.m();
        check(
            &format!(
                "and light up ({:?} {:?} {:?} {:?}), Solo's master light too",
                m.notes.get(&0x10),
                m.notes.get(&0x09),
                m.notes.get(&0x02),
                m.notes.get(&0x1B)
            ),
            m.lit(0x10) && m.lit(0x09) && m.lit(0x02) && m.lit(0x1B) && !m.lit(0x18) && m.lit(0x73) && m.lit(0x5A),
        );
    }
    s.update_track_linked(tracks[0], TrackPatch { mute: Some(false), ..Default::default() });
    run(&mut s, 60);
    check("unmuting with the mouse puts Mute's light out", !main_unit.m().lit(0x10));
    main_unit.press(0x5A); // the master section's Solo: clears every solo
    run(&mut s, 60);
    check("the master Solo clears every solo", !t(&s, 1).solo && !main_unit.m().lit(0x09) && !main_unit.m().lit(0x73));

    // Banking: right by the 16 strips of both units, kept within the 20 tracks.
    main_unit.press(0x2F);
    run(&mut s, 60);
    let st = s.control_surfaces();
    check(
        &format!("Bank Right moves both units, the last bank full ({:?}, {:?})", st[0].showing, main_unit.m().line(0)),
        st[0].showing == "Tracks 5–12"
            && main_unit.m().line(0).starts_with("Ch05")
            && ext.m().line(0).starts_with("Ch13")
            && ext.m().line(0).trim_end().ends_with("Ch20"),
    );
    check(
        &format!("the faders follow the bank ({:?})", main_unit.m().faders[0]),
        near(main_unit.m().faders[0], pos_of(vol(&s, 4))) && near(ext.m().faders[7], pos_of(vol(&s, 19))),
    );
    check("the lights follow the bank", !main_unit.m().lit(0x02) && !main_unit.m().lit(0x00) && !main_unit.m().lit(0x1B));
    main_unit.press(0x30); // Channel Left
    run(&mut s, 60);
    check(&format!("Channel Left moves by one ({:?})", main_unit.m().line(0)), main_unit.m().line(0).starts_with("Ch04") && main_unit.m().lit(0x18));
    main_unit.press(0x2E); // Bank Left
    run(&mut s, 60);
    check("Bank Left goes back to the start", main_unit.m().line(0).starts_with("Ch01") && s.control_surfaces()[0].showing == "Tracks 1–8");

    // A V-Pot: pan, its ring and its value on the LCD; pushed, back to the centre.
    main_unit.send(&[0xB0, 0x10, 0x14]); // 20 steps clockwise
    run(&mut s, 60);
    let pan = s.project.track(tracks[0]).unwrap().pan;
    check(&format!("a V-Pot turns the pan ({pan})"), (pan - 20.0 / 64.0).abs() < 1e-4);
    let ring = main_unit.m().rings[0];
    let cell = main_unit.m().cell(1, 0);
    check(&format!("its ring and the LCD follow ({ring:?}, {cell:?})"), ring == Some(0x08) && cell == " R20  ");
    main_unit.send(&[0xB0, 0x10, 0x45]); // 5 steps back
    run(&mut s, 60);
    check("turning back", (s.project.track(tracks[0]).unwrap().pan - 15.0 / 64.0).abs() < 1e-4);
    main_unit.press(0x20);
    run(&mut s, 60);
    check(
        &format!("pushing it centres the pan ({:?})", main_unit.m().rings[0]),
        s.project.track(tracks[0]).unwrap().pan == 0.0 && main_unit.m().rings[0] == Some(0x06),
    );
    main_unit.press(0x29); // Send
    run(&mut s, 60);
    check(
        &format!("Send: the assignment display says S1 ({:?})", main_unit.m().assignment()),
        main_unit.m().assignment() == "S1" && main_unit.m().lit(0x29) && !main_unit.m().lit(0x2A),
    );
    check(
        &format!("tracks without that send show nothing on the V-Pot ({:?})", main_unit.m().rings[0]),
        main_unit.m().rings[0] == Some(0) && main_unit.m().cell(1, 0).trim().is_empty(),
    );
    main_unit.press(0x2A);
    run(&mut s, 60);
    check("Pan again", main_unit.m().assignment() == "PN");

    // Transport, Cycle and Click.
    main_unit.press(0x5E);
    run(&mut s, 60);
    check("Play plays and lights up", s.is_playing() && main_unit.m().lit(0x5E) && !main_unit.m().lit(0x5D));
    main_unit.press(0x5D);
    run(&mut s, 60);
    check("Stop stops", !s.is_playing() && !main_unit.m().lit(0x5E) && main_unit.m().lit(0x5D));
    let (cycle, click) = (s.project.cycle.enabled, s.project.metronome);
    main_unit.press(0x56);
    main_unit.press(0x59);
    run(&mut s, 60);
    check(
        "Cycle and Click switch and light up",
        s.project.cycle.enabled != cycle
            && s.project.metronome != click
            && main_unit.m().lit(0x56) == s.project.cycle.enabled
            && main_unit.m().lit(0x59) == s.project.metronome,
    );

    // The time display, in bars and beats, then as time.
    let map = s.tempo_map();
    let bar5 = map.bar_start(5).round() as i64;
    s.locate(bar5);
    run(&mut s, 60);
    check(&format!("the time display shows bar 5 ({:?})", main_unit.m().time()), main_unit.m().time() == "  5. 1. 1.  1");
    check("the Beats light is lit", main_unit.m().lit(0x72) && !main_unit.m().lit(0x71));
    main_unit.press(0x35);
    run(&mut s, 60);
    let secs = bar5 as f64 / s.sample_rate();
    let want = format!("  0.{:02}.{:02}.{:03}", (secs / 60.0) as u32, secs as u32 % 60, (secs.fract() * 1000.0).floor() as u32);
    check(&format!("SMPTE/Beats shows the time ({:?}, want {want:?})", main_unit.m().time()), main_unit.m().time() == want && main_unit.m().lit(0x71));
    main_unit.press(0x35);

    // The jog wheel: a beat a step.
    main_unit.send(&[0xB0, 0x3C, 0x02]);
    run(&mut s, 60);
    let beat = |s: &Session| map.beats_at(s.position() as f64) - map.beats_at(bar5 as f64);
    check(&format!("the jog wheel moves the playhead two beats ({})", beat(&s)), (beat(&s) - 2.0).abs() < 1e-3 && main_unit.m().time() == "  5. 3. 1.  1");
    main_unit.send(&[0xB0, 0x3C, 0x41]);
    run(&mut s, 60);
    check(&format!("and back one ({})", beat(&s)), (beat(&s) - 1.0).abs() < 1e-3);

    // The master fader.
    let n = pos_of(-10.0);
    main_unit.send(&[0xE8, (n & 0x7F) as u8, (n >> 7) as u8]);
    run(&mut s, 60);
    check(&format!("the master fader sets Stereo Out ({} dB)", s.project.master.volume_db), (s.project.master.volume_db + 10.0).abs() < 0.05);

    // Key commands: F1 runs its command; Marker on its own makes a marker, and held with Forward
    // goes to the next one.
    s.tick();
    main_unit.press(0x36);
    main_unit.press(0x54);
    main_unit.send(&[0x90, 0x54, 0x7F]);
    main_unit.press(0x5C);
    main_unit.send(&[0x90, 0x54, 0x00]);
    run(&mut s, 60);
    let commands = s.tick().commands;
    check(&format!("F1 and Marker run key commands ({commands:?})"), commands == ["next-marker", "create-marker", "next-marker"]);

    // A unit that restarts says hello again, and gets everything.
    {
        let mut m = main_unit.m();
        m.lcd.fill(b' ');
        m.faders = [None; 9];
        m.notes.clear();
    }
    let mut hello = vec![0xF0, 0, 0, 0x66, mcu::MAIN, 0x01, 1, 2, 3, 4, 5, 6, 7];
    hello.extend(CHALLENGE);
    hello.push(0xF7);
    main_unit.send(&hello);
    // (The master fader moved a moment ago: it is sent once its half second of quiet is over.)
    run(&mut s, 600);
    let faders = main_unit.m().faders;
    check(
        &format!("a restarted unit gets everything again ({:?}, {faders:?})", main_unit.m().line(0)),
        main_unit.m().line(0).starts_with("Ch01") && main_unit.m().faders.iter().all(|f| f.is_some()) && main_unit.m().lit(0x5D),
    );

    // ---- a HUI ----------------------------------------------------------------------------
    prefs.midi.surfaces = vec![unit(SurfaceKind::Hui, &hui, 0, vec![])];
    s.set_prefs(prefs.clone());
    run(&mut s, 1200);
    let st = s.control_surfaces();
    check(&format!("the HUI is pinged and answers ({} pings, {st:?})", hui.m().pings), hui.m().pings >= 1 && st.len() == 1 && st[0].online);
    check(
        &format!("its scribble strips name the tracks ({:?})", String::from_utf8_lossy(&hui.m().lcd)),
        hui.m().scribble(0) == "Ch01" && hui.m().scribble(1) == "BssG" && hui.m().scribble(8) == "Pan ",
    );
    check(&format!("its faders show the levels ({:?})", hui.m().faders), (0..8).all(|i| near(hui.m().faders[i], pos_of(vol(&s, i)))));
    // A fader, touched and moved.
    hui.send(&[0xB0, 0x0F, 0x01]);
    hui.send(&[0xB0, 0x2F, 0x40]);
    let n = pos_of(-3.0);
    hui.send(&[0xB0, 0x01, (n >> 7) as u8]);
    hui.send(&[0xB0, 0x21, (n & 0x7F) as u8]);
    run(&mut s, 60);
    check(&format!("a HUI fader sets the volume ({} dB)", vol(&s, 1)), (vol(&s, 1) + 3.0).abs() < 0.05);
    let before = hui.m().fader_msgs;
    s.update_track_linked(tracks[1], TrackPatch { volume_db: Some(-15.0), ..Default::default() });
    run(&mut s, 200);
    check("a touched HUI fader isn't moved", hui.m().fader_msgs == before);
    hui.send(&[0xB0, 0x0F, 0x01]);
    hui.send(&[0xB0, 0x2F, 0x00]);
    run(&mut s, 60);
    check(&format!("let go, it goes to the level ({:?})", hui.m().faders[1]), near(hui.m().faders[1], pos_of(-15.0)));
    // Switches and their lights: Mute 1, Play.
    hui.press_hui(0x00, 2);
    run(&mut s, 60);
    check("a HUI Mute switch mutes and lights up", t(&s, 0).mute && hui.m().leds.get(&(0, 2)) == Some(&true));
    hui.press_hui(0x0E, 4);
    run(&mut s, 60);
    check("Play plays and lights up", s.is_playing() && hui.m().leds.get(&(0x0E, 4)) == Some(&true));
    hui.press_hui(0x0E, 3);
    run(&mut s, 60);
    check("Stop stops", !s.is_playing() && hui.m().leds.get(&(0x0E, 4)) == Some(&false));
    // Bank right by its 8 strips.
    hui.press_hui(0x0A, 3);
    run(&mut s, 60);
    check(&format!("Bank Right shows tracks 9–16 ({:?})", hui.m().scribble(0)), hui.m().scribble(0) == "Ch09" && near(hui.m().faders[0], pos_of(vol(&s, 8))));
    let time = hui.m().hui_time.clone();
    check(&format!("the time display is sent ({time:?})"), time.as_ref().is_some_and(|t| t.len() == 8));
    // A HUI that stops answering is offline; answering again, it gets everything.
    hui.m().answer = false;
    run(&mut s, 4000);
    check("a HUI that stops answering pings is offline", !s.control_surfaces()[0].online);
    {
        let mut m = hui.m();
        m.answer = true;
        m.lcd.fill(b' ');
    }
    run(&mut s, 1200);
    check(
        &format!("answering again, it is online and gets everything ({:?})", hui.m().scribble(0)),
        s.control_surfaces()[0].online && hui.m().scribble(0) == "Ch09",
    );

    // No surfaces: their ports play instruments again.
    prefs.midi.surfaces.clear();
    s.set_prefs(prefs);
    run(&mut s, 60);
    check("with the surfaces removed, none is listed", s.control_surfaces().is_empty());

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
