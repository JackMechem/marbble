//! Control surfaces on the session side (the protocols and the pure parts are in
//! `crate::control::surface`): the units set up in Settings › MIDI › Control Surfaces (here), what
//! their faders, V-Pots and buttons do (`input.rs`), and what they show (`display.rs`).
//!
//! A surface's moves go through the controller assignments' edit paths
//! (`session/control/controllers/`): a fader is a Volume target, a V-Pot a Pan, Send or Selected
//! Track plug-in parameter target, the strip buttons switches, so undo steps, groups and
//! automation writing work as they do from the mouse. A touched fader holds its automation
//! parameter (an automation touch) until it is let go, rather than for half a second after it
//! stops. Buttons with no session edit behind them fire key commands, as a controller assigned
//! to one does (#221): the main window runs them.
//!
//! What a surface shows is read every idle tick (30 ms, main thread) from where the session
//! keeps it, as controller feedback does (`session/control/feedback.rs`): faders and V-Pot rings are
//! `ctl_shown`, so they follow the mouse, automation, undo and opening a project alike; a
//! fader's [`Throttle`] limits its rate and keeps it still while it is touched or moved. Only
//! what differs from what a unit shows is sent: a fader, a light, a ring, the LCD characters
//! that changed, the digits that changed. A unit that comes online (it answers the device
//! query or a ping, or a port is connected anew) is sent everything. The meters are the levels
//! the tick reads for the interface.

mod display;
mod input;

use std::cell::RefCell;

use vst3_host::ParamInfo;

use crate::control::controllers::{self as ctl, port_key, ButtonAct, CtlTarget, SelectedParam, TransportAction};
use crate::control::feedback::{Msgs, Shown, Throttle};
use crate::control::surface::{self as sf, hui, mcu, Button, Digit, Input, RingMode, SurfaceConfig, SurfaceKind, MASTER, STRIPS};
use crate::session::*;

/// How long a strip shows its fader's level after it moved or was let go, or its V-Pot's
/// value after it turned (with Name/Value showing names).
const SHOW_VALUE: Duration = Duration::from_millis(1500);
/// How often a Mackie Control is asked who it is until it answers.
const QUERY_EVERY: Duration = Duration::from_secs(2);
/// How often a HUI is pinged, and how long it may leave pings unanswered before it is offline.
const PING_EVERY: Duration = Duration::from_secs(1);
const PING_TIMEOUT: Duration = Duration::from_millis(3500);
/// How often a meter with signal is sent again: the surface lets it fall by itself.
const METER_EVERY: Duration = Duration::from_millis(100);
/// A V-Pot step on its parameter's travel: pan moves by one of its 128 steps.
const POT_STEP: f32 = 1.0 / 128.0;
/// A Mackie Control's meter mode: signal light and peak hold, not the LCD's level meter.
const METER_MODE: u8 = 0x03;
/// Each strip's width on a Mackie Control's LCD: six characters and a space.
const CELL: usize = 7;

/// What the V-Pots drive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PotMode {
    #[default]
    Pan,
    /// A send slot's level.
    Send(u8),
    /// The selected track's focused plug-in's parameters, from `Surfaces::page`.
    Plugin,
}

/// What the time display shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TimeMode {
    #[default]
    Bars,
    Time,
}

/// A unit's state, for Settings › MIDI › Control Surfaces.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceStatus {
    /// Its input and output ports are connected.
    pub input: bool,
    pub output: bool,
    /// It answered (the device query, a ping, or anything sent).
    pub online: bool,
    /// The tracks its strips show, as "Tracks 1–8", or why none.
    pub showing: String,
}

/// One unit and what it shows.
struct Unit {
    cfg: SurfaceConfig,
    online: bool,
    /// When it was last asked who it is (or pinged) and last heard from.
    asked: Option<Instant>,
    heard: Option<Instant>,
    hui: hui::Decoder,
    /// The faders (the strips', then the master's).
    faders: [Throttle; STRIPS + 1],
    touched: [bool; STRIPS + 1],
    /// The track whose volume a touched fader holds (MASTER_ID for the master).
    holding: [Option<Id>; STRIPS + 1],
    leds: HashMap<Button, bool>,
    rings: [Option<u8>; STRIPS],
    /// The LCD's 112 characters (a HUI: its nine scribble strips' 36).
    lcd: Option<Vec<u8>>,
    /// The digits, rightmost first: a Mackie Control's ten and the assignment display's two, a
    /// HUI's eight.
    digits: Vec<Option<Digit>>,
    /// Each strip's meter level as sent, and when.
    meters: [(u8, Option<Instant>); STRIPS],
    meter_mode: bool,
}

impl Unit {
    fn new(cfg: SurfaceConfig) -> Unit {
        Unit {
            cfg,
            online: false,
            asked: None,
            heard: None,
            hui: Default::default(),
            faders: Default::default(),
            touched: [false; STRIPS + 1],
            holding: [None; STRIPS + 1],
            leds: HashMap::new(),
            rings: [None; STRIPS],
            lcd: None,
            digits: Vec::new(),
            meters: [(0, None); STRIPS],
            meter_mode: false,
        }
    }

    /// What it shows is unknown: everything is sent again.
    fn forget(&mut self) {
        self.faders.iter_mut().for_each(Throttle::forget);
        self.leds.clear();
        self.rings = [None; STRIPS];
        self.lcd = None;
        self.digits.clear();
        self.meters = [(0, None); STRIPS];
        self.meter_mode = false;
    }

    /// It is there: everything is sent to it if it wasn't.
    fn come_online(&mut self) {
        if !self.online {
            self.online = true;
            self.forget();
        }
    }

    fn device(&self) -> u8 {
        if self.cfg.kind == SurfaceKind::McuExtender {
            mcu::EXTENDER
        } else {
            mcu::MAIN
        }
    }

    fn hui(&self) -> bool {
        self.cfg.kind == SurfaceKind::Hui
    }

    /// A fader's position as its messages.
    fn fader(&self, strip: u8, v: f32) -> Msgs {
        if self.hui() {
            hui::fader(strip, v).to_vec()
        } else {
            vec![mcu::fader(strip, v)]
        }
    }

    /// Whether it has a master fader.
    fn has_master(&self) -> bool {
        self.cfg.kind == SurfaceKind::Mcu
    }
}

/// A plug-in's parameters and when they were read.
type ParamList = (Id, Instant, Arc<[ParamInfo]>);

#[derive(Default)]
pub(crate) struct Surfaces {
    units: Vec<Unit>,
    /// The bank's first track, in the visible tracks.
    bank: usize,
    pot: PotMode,
    /// Plug-in mode: the parameter the first strip shows.
    page: usize,
    time: TimeMode,
    /// Name/Value: the bottom line names what the V-Pots drive instead of showing its value.
    names: bool,
    nudge: bool,
    zoom: bool,
    scrub: bool,
    /// Buttons held down, on any unit.
    held: Vec<Button>,
    /// Marker was held and used with Rewind or Forward: letting go of it makes no marker.
    marker_used: bool,
    /// When each track's fader last moved or was let go (MASTER_ID for the master).
    fader_moved: HashMap<Id, Instant>,
    /// When each strip's V-Pot last turned (by the strip's place from the bank's start).
    pot_moved: HashMap<usize, Instant>,
    /// Tracks whose volume a touched fader holds, and whether its automation touch began.
    touches: Vec<(Id, bool)>,
    /// Each track's latest meter peak (linear), from the tick.
    peaks: HashMap<Id, f32>,
    /// Plug-ins' parameters for Plug-in mode, and when each list was read (a VST3 describes
    /// every parameter to give one).
    params: RefCell<Vec<ParamList>>,
    /// Messages to send at the end of the idle tick: (output key, message).
    out: Vec<(String, Vec<u8>)>,
}

/// What a unit should show.
struct Frame {
    /// The faders' positions (None: no fader there).
    faders: [Option<f32>; STRIPS + 1],
    leds: Vec<(Button, bool)>,
    rings: [u8; STRIPS],
    lcd: Vec<u8>,
    /// Leftmost first: the time display (and a Mackie Control's assignment display).
    digits: Vec<Digit>,
    meters: [u8; STRIPS],
}

impl Session {
    /// The control surfaces' units and how each is connected, in Settings' order.
    pub fn control_surfaces(&self) -> Vec<SurfaceStatus> {
        let outputs = self.midi_out.keys();
        let inputs: Vec<String> = self.midi.connected().iter().map(|n| port_key(n).to_string()).collect();
        let tracks = self.surface_tracks().len();
        let bank = self.surface_bank(tracks);
        self.prefs
            .midi
            .surfaces
            .iter()
            .enumerate()
            .map(|(i, cfg)| {
                let first = bank + cfg.offset as usize;
                let last = (first + STRIPS).min(tracks);
                SurfaceStatus {
                    input: inputs.contains(&cfg.input),
                    output: outputs.contains(&cfg.output),
                    online: self.surfaces.units.get(i).is_some_and(|u| u.cfg == *cfg && u.online),
                    showing: if first < last { format!("Tracks {}–{}", first + 1, last) } else { "No tracks".into() },
                }
            })
            .collect()
    }

    /// The latest meter peaks, from the tick, for the surfaces' meters.
    pub(crate) fn surface_meters(&mut self, meters: &[(Id, f32, f32)]) {
        if self.surfaces.units.is_empty() {
            return;
        }
        self.surfaces.peaks = meters.iter().map(|&(id, l, r)| (id, l.max(r))).collect();
    }

    /// The surfaces' idle tick: reads what they sent and applies it, keeps them online, and
    /// sends what changed. `fresh`: the outputs connected since the last tick. Returns whether
    /// the project changed.
    pub(crate) fn surface_idle(&mut self, fresh: &[String]) -> bool {
        self.surface_setup();
        if self.surfaces.units.is_empty() {
            return false;
        }
        let now = Instant::now();
        let mut events = Vec::new();
        for (port, msg) in self.midi.take_surface() {
            for (i, u) in self.surfaces.units.iter_mut().enumerate() {
                if *u.cfg.input != *port {
                    continue;
                }
                u.heard = Some(now);
                let ev = if u.hui() { u.hui.feed(&msg) } else { mcu::decode(&msg) };
                events.extend(ev.map(|e| (i, e)));
            }
        }
        // A fader's latest position in the batch is the one that counts.
        let fader = |e: &(usize, Input)| match e.1 {
            Input::Fader { strip, .. } => Some((e.0, strip)),
            _ => None,
        };
        let mut changed = false;
        for (k, e) in events.iter().enumerate() {
            if fader(e).is_some() && events[k + 1..].iter().any(|later| fader(later) == fader(e)) {
                continue;
            }
            changed |= self.surface_input(e.0, e.1, now);
        }
        if !self.is_playing() {
            // A pass ended: a fader still touched begins a new one when written again.
            self.surfaces.touches.iter_mut().for_each(|t| t.1 = false);
        }
        self.surface_links(fresh, now);
        self.surface_show(now);
        let out = std::mem::take(&mut self.surfaces.out);
        self.midi_out.send_raw(out);
        changed
    }

    /// Keeps the units in step with Settings, and the input threads with their ports.
    fn surface_setup(&mut self) {
        let cfgs = &self.prefs.midi.surfaces;
        if self.surfaces.units.len() == cfgs.len() && self.surfaces.units.iter().zip(cfgs).all(|(u, c)| u.cfg == *c) {
            return;
        }
        let cfgs = cfgs.clone();
        let mut old = std::mem::take(&mut self.surfaces.units);
        self.surfaces.units = cfgs
            .iter()
            .map(|c| match old.iter().position(|u| u.cfg == *c) {
                Some(i) => old.remove(i),
                None => Unit::new(c.clone()),
            })
            .collect();
        // The touches of units that went are let go.
        for u in old {
            for id in u.holding.iter().flatten() {
                self.surface_release(*id);
            }
        }
        self.midi.set_surface_ports(cfgs.iter().map(|c| c.input.clone()).filter(|p| !p.is_empty()).collect());
    }

    /// Asks Mackie Controls who they are until they answer, pings HUIs, and forgets what units
    /// show whose output went or was connected anew.
    fn surface_links(&mut self, fresh: &[String], now: Instant) {
        let outputs = self.midi_out.keys();
        let Surfaces { units, out, .. } = &mut self.surfaces;
        for u in units.iter_mut() {
            if !outputs.contains(&u.cfg.output) {
                u.online = false;
                u.asked = None;
                u.forget();
                continue;
            }
            if fresh.contains(&u.cfg.output) {
                u.online = false;
                u.asked = None;
                u.forget();
            }
            let due = |every| u.asked.is_none_or(|t| now.saturating_duration_since(t) >= every);
            if u.hui() {
                if due(PING_EVERY) {
                    out.push((u.cfg.output.clone(), hui::PING.to_vec()));
                    u.asked = Some(now);
                }
                if u.online && u.heard.is_none_or(|t| now.saturating_duration_since(t) > PING_TIMEOUT) {
                    u.online = false;
                }
            } else if !u.online && due(QUERY_EVERY) {
                out.push((u.cfg.output.clone(), mcu::device_query(u.device())));
                u.asked = Some(now);
            }
        }
    }

    /// The tracks the surfaces' strips show, in order: not hidden, not in a collapsed stack.
    fn surface_tracks(&self) -> Vec<Id> {
        let p = &self.project;
        let collapsed = |mut parent: Option<Id>| {
            // (Bounded, in case a damaged project's stacks go round.)
            for _ in 0..64 {
                let Some(t) = parent.and_then(|id| p.track(id)) else { return false };
                if t.collapsed {
                    return true;
                }
                parent = t.parent;
            }
            false
        };
        p.tracks.iter().filter(|t| !t.hidden && !collapsed(t.parent)).map(|t| t.id).collect()
    }

    /// The bank's first track, kept within `tracks`.
    fn surface_bank(&self, tracks: usize) -> usize {
        sf::clamp_bank(self.surfaces.bank as i64, tracks, sf::strips_of(&self.prefs.midi.surfaces))
    }

    /// The plug-in parameters Plug-in mode shows: the selected track's focused plug-in's.
    fn surface_params(&self) -> Option<Arc<[ParamInfo]>> {
        let track = self.selected.filter(|id| self.project.track(*id).is_some())?;
        let uid = self.ctl_focused_plugin(track)?.uid;
        let fresh = |at: &Instant| at.elapsed() < Duration::from_secs(2);
        if let Some((_, _, list)) = self.surfaces.params.borrow().iter().find(|(u, at, _)| *u == uid && fresh(at)) {
            return Some(list.clone());
        }
        let list: Arc<[ParamInfo]> = self.plugin_params(uid).into();
        let mut cache = self.surfaces.params.borrow_mut();
        cache.retain(|(u, at, _)| *u != uid && fresh(at));
        cache.push((uid, Instant::now(), list.clone()));
        Some(list)
    }

    /// What strip `place` (from the bank's start) of a unit's V-Pot drives, with `track` on it.
    fn surface_pot(&self, place: usize, track: Option<Id>) -> Option<CtlTarget> {
        match self.surfaces.pot {
            PotMode::Pan => Some(CtlTarget::Pan { track: track? }),
            PotMode::Send(slot) => {
                let track = track?;
                self.project.track(track)?.sends.get(slot as usize)?.as_ref()?;
                Some(CtlTarget::Send { track, slot })
            }
            PotMode::Plugin => {
                let index = self.surfaces.page + place;
                (index < self.surface_params()?.len())
                    .then_some(CtlTarget::Selected { param: SelectedParam::PluginParam { index: index.min(u16::MAX as usize) as u16 } })
            }
        }
    }
}
