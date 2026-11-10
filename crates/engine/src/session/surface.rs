//! Control surfaces on the session side (the protocols and the pure parts are in
//! `crate::surface`): the units set up in Settings › MIDI › Control Surfaces, what their
//! faders, V-Pots and buttons do, and what they show.
//!
//! A surface's moves go through the controller assignments' edit paths
//! (`session/controllers.rs`): a fader is a Volume target, a V-Pot a Pan, Send or Selected
//! Track plug-in parameter target, the strip buttons switches, so undo steps, groups and
//! automation writing work as they do from the mouse. A touched fader holds its automation
//! parameter (an automation touch) until it is let go, rather than for half a second after it
//! stops. Buttons with no session edit behind them fire key commands, as a controller assigned
//! to one does (#221): the main window runs them.
//!
//! What a surface shows is read every idle tick (30 ms, main thread) from where the session
//! keeps it, as controller feedback does (`session/feedback.rs`): faders and V-Pot rings are
//! `ctl_shown`, so they follow the mouse, automation, undo and opening a project alike; a
//! fader's [`Throttle`] limits its rate and keeps it still while it is touched or moved. Only
//! what differs from what a unit shows is sent: a fader, a light, a ring, the LCD characters
//! that changed, the digits that changed. A unit that comes online (it answers the device
//! query or a ping, or a port is connected anew) is sent everything. The meters are the levels
//! the tick reads for the interface.

use std::cell::RefCell;

use vst3_host::ParamInfo;

use super::*;
use crate::controllers::{self as ctl, port_key, ButtonAct, CtlTarget, SelectedParam, TransportAction};
use crate::feedback::{Msgs, Shown, Throttle};
use crate::surface::{self as sf, hui, mcu, Button, Digit, Input, RingMode, SurfaceConfig, SurfaceKind, MASTER, STRIPS};

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
pub(super) struct Surfaces {
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
    pub(super) fn surface_meters(&mut self, meters: &[(Id, f32, f32)]) {
        if self.surfaces.units.is_empty() {
            return;
        }
        self.surfaces.peaks = meters.iter().map(|&(id, l, r)| (id, l.max(r))).collect();
    }

    /// The surfaces' idle tick: reads what they sent and applies it, keeps them online, and
    /// sends what changed. `fresh`: the outputs connected since the last tick. Returns whether
    /// the project changed.
    pub(super) fn surface_idle(&mut self, fresh: &[String]) -> bool {
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

    // ---- what the surfaces did ------------------------------------------------------------

    /// One thing unit `ui` did. Returns whether the project changed.
    fn surface_input(&mut self, ui: usize, ev: Input, now: Instant) -> bool {
        let u = &mut self.surfaces.units[ui];
        match ev {
            Input::Hello { serial, challenge } => {
                // It started (again): answer its challenge and send it everything.
                self.surfaces.out.push((u.cfg.output.clone(), mcu::connection_reply(u.device(), serial, challenge)));
                u.online = false;
                u.come_online();
                return false;
            }
            Input::Alive => {
                u.come_online();
                return false;
            }
            _ => u.come_online(),
        }
        let (offset, has_master) = (u.cfg.offset as usize, u.has_master());
        let tracks = self.surface_tracks();
        let bank = self.surface_bank(tracks.len());
        let track_at = |strip: u8| tracks.get(bank + offset + strip as usize).copied();
        match ev {
            Input::Fader { strip, value } => {
                if strip > MASTER {
                    return false;
                }
                let target =
                    if strip == MASTER { has_master.then_some(CtlTarget::MasterVolume) } else { track_at(strip).map(|track| CtlTarget::Volume { track }) };
                let u = &mut self.surfaces.units[ui];
                let shown = u.fader(strip, value);
                u.faders[strip as usize].moved(shown, now);
                let Some(target) = target else { return false };
                self.surfaces.fader_moved.insert(target.track().unwrap_or(MASTER_ID), now);
                self.ctl_set(&target, value);
                true
            }
            Input::Touch { strip, on } => {
                let track = if strip == MASTER { has_master.then_some(MASTER_ID) } else { track_at(strip) };
                self.surface_touch(ui, strip, on, track, now);
                false
            }
            Input::VPot { strip, steps } => {
                let place = offset + strip as usize;
                let Some(target) = self.surface_pot(place, track_at(strip)).and_then(|t| self.ctl_resolve(&t)) else { return false };
                let Some(current) = self.ctl_value(&target) else { return false };
                // A stepped plug-in parameter moves a step at a time.
                let step = match (&target, self.surfaces.pot) {
                    (CtlTarget::Plugin { param, .. }, PotMode::Plugin) => {
                        let steps = self.surface_params().and_then(|ps| ps.iter().find(|p| p.id == *param).map(|p| p.steps)).unwrap_or(0);
                        if steps > 0 {
                            1.0 / steps as f32
                        } else {
                            POT_STEP
                        }
                    }
                    _ => POT_STEP,
                };
                self.surfaces.pot_moved.insert(place, now);
                self.ctl_set(&target, (current + steps as f32 * step).clamp(0.0, 1.0));
                true
            }
            Input::Jog { steps } => {
                self.surface_jog(steps);
                true
            }
            Input::Button { button, down } => {
                let track = button.strip().and_then(track_at);
                self.surface_button(ui, button, down, track, offset)
            }
            Input::Hello { .. } | Input::Alive => false,
        }
    }

    /// A fader was touched or let go. While touched it holds its track's volume (an automation
    /// touch while automation is being written) and nothing moves it.
    fn surface_touch(&mut self, ui: usize, strip: u8, on: bool, track: Option<Id>, now: Instant) {
        let s = strip as usize;
        if s > STRIPS {
            return;
        }
        let u = &mut self.surfaces.units[ui];
        u.touched[s] = on;
        let held = std::mem::take(&mut u.holding[s]);
        if on {
            u.holding[s] = track;
        } else {
            // Let go: it is sent where its fader is, whatever it last sent.
            u.faders[s].button(true, now);
        }
        if let Some(id) = held {
            self.surface_release(id);
        }
        if let Some(id) = track {
            self.surfaces.fader_moved.insert(id, now);
            if on {
                self.surfaces.touches.push((id, false));
                self.surface_holds(id, &AutoParam::Volume);
            }
        }
    }

    /// A touched fader on `track` was let go: so is its automation parameter.
    fn surface_release(&mut self, track: Id) {
        let Some(i) = self.surfaces.touches.iter().position(|t| t.0 == track) else { return };
        let (_, began) = self.surfaces.touches.remove(i);
        if began && !self.surfaces.touches.iter().any(|t| t.0 == track) {
            self.automation_touch(track, AutoParam::Volume, false);
        }
    }

    /// Whether a touched fader holds `param` of `track`; begins its automation touch when
    /// automation is being written (`ctl_hold` asks, and holds it for half a second otherwise).
    pub(super) fn surface_holds(&mut self, track: Id, param: &AutoParam) -> bool {
        if !matches!(param, AutoParam::Volume) {
            return false;
        }
        let Some(i) = self.surfaces.touches.iter().position(|t| t.0 == track) else { return false };
        if !self.surfaces.touches[i].1 && self.automation_writing(track) {
            self.surfaces.touches[i].1 = true;
            self.automation_touch(track, AutoParam::Volume, true);
        }
        true
    }

    /// The jog wheel moves the playhead a beat a step (a sixteenth of one with Scrub on).
    fn surface_jog(&mut self, steps: i32) {
        let map = self.tempo_map();
        let pos = self.position() as f64;
        let beat = 4.0 / map.sig_at(pos).1.max(1) as f64;
        let step = if self.surfaces.scrub { beat / 4.0 } else { beat };
        let q = (map.beats_at(pos) + steps as f64 * step).max(0.0);
        self.locate(map.sample_at(q).round() as i64);
    }

    /// Moves the bank (or Plug-in mode's parameters) by `by` strips.
    fn surface_bank_by(&mut self, by: i64) {
        let strips = sf::strips_of(&self.prefs.midi.surfaces);
        if self.surfaces.pot == PotMode::Plugin {
            let n = self.surface_params().map(|p| p.len()).unwrap_or(0);
            self.surfaces.page = sf::clamp_bank(self.surfaces.page as i64 + by, n, strips);
        } else {
            let n = self.surface_tracks().len();
            let bank = self.surface_bank(n) as i64;
            self.surfaces.bank = sf::clamp_bank(bank + by, n, strips);
        }
    }

    /// The selected track's automation mode, set as the track header's menu does.
    fn surface_auto_mode(&mut self, mode: impl Fn(AutoMode) -> AutoMode) {
        let Some(track) = self.selected.filter(|id| self.project.track(*id).is_some()) else { return };
        let Some(now) = self.auto_mode_of(track) else { return };
        let to = mode(now);
        if to != now {
            self.checkpoint("Automation Mode", None, false);
            self.set_auto_mode(track, to);
        }
    }

    /// A button pressed (`down`) or let go on unit `ui`; `track` is a strip button's track.
    /// Returns whether the project changed.
    fn surface_button(&mut self, ui: usize, b: Button, down: bool, track: Option<Id>, offset: usize) -> bool {
        let sfs = &mut self.surfaces;
        if !down {
            sfs.held.retain(|h| *h != b);
            // Marker on its own makes a marker; with Rewind or Forward it went to one.
            if b == Button::Marker && !std::mem::take(&mut sfs.marker_used) {
                self.ctl_command("create-marker");
            }
            return false;
        }
        if !sfs.held.contains(&b) {
            sfs.held.push(b);
        }
        let shift = sfs.held.contains(&Button::Shift);
        let marker = sfs.held.contains(&Button::Marker);
        let (zoom, nudge) = (sfs.zoom, sfs.nudge);
        let strips = sf::strips_of(&self.prefs.midi.surfaces) as i64;
        let switch = |t: Option<CtlTarget>| t.map(|t| (t, ButtonAct::Flip));
        let flip = match b {
            Button::Rec(_) => switch(track.map(|track| CtlTarget::Arm { track })),
            Button::Solo(_) => switch(track.map(|track| CtlTarget::Solo { track })),
            Button::Mute(_) => switch(track.map(|track| CtlTarget::Mute { track })),
            Button::Cycle => switch(Some(CtlTarget::Transport { action: TransportAction::Cycle })),
            Button::Record => switch(Some(CtlTarget::Transport { action: TransportAction::Record })),
            _ => None,
        };
        if let Some((t, act)) = flip {
            self.ctl_switch(&t, act);
            return true;
        }
        let command = match b {
            Button::F(n) => {
                let id = self.surfaces.units[ui].cfg.functions.get(n as usize).filter(|id| !id.is_empty()).cloned();
                if let Some(id) = id {
                    self.ctl_command(&id);
                }
                return false;
            }
            Button::Save => "save",
            Button::Undo if shift => "redo",
            Button::Undo => "undo",
            Button::Drop => "autopunch",
            Button::Replace => "replace-mode",
            Button::ToStart => "to-start",
            Button::Rewind | Button::Forward if marker => {
                self.surfaces.marker_used = true;
                if b == Button::Rewind {
                    "previous-marker"
                } else {
                    "next-marker"
                }
            }
            Button::Rewind | Button::Left if nudge => "nudge-left",
            Button::Forward | Button::Right if nudge => "nudge-right",
            Button::Left if zoom => "zoom-out",
            Button::Right if zoom => "zoom-in",
            Button::Rewind | Button::Left => "rewind",
            Button::Forward | Button::Right => "forward",
            Button::Up if zoom => "shorter-tracks",
            Button::Down if zoom => "taller-tracks",
            Button::Up => "select-previous",
            Button::Down => "select-next",
            _ => "",
        };
        if !command.is_empty() {
            self.ctl_command(command);
            return false;
        }
        let sfs = &mut self.surfaces;
        match b {
            Button::Select(_) => match track {
                Some(t) => self.select_track(Some(t)),
                None => return false,
            },
            Button::VPot(s) => {
                // Back to its default: pan centre, a send at 0 dB, a plug-in's own default.
                let place = offset + s as usize;
                let Some(target) = self.surface_pot(place, track).and_then(|t| self.ctl_resolve(&t)) else { return false };
                let default = match &target {
                    CtlTarget::Pan { .. } => 0.5,
                    CtlTarget::Send { .. } => ctl::db_to_fader(0.0),
                    CtlTarget::Plugin { param, .. } => {
                        let Some(d) = self.surface_params().and_then(|ps| ps.iter().find(|p| p.id == *param).map(|p| p.default as f32)) else { return false };
                        d
                    }
                    _ => return false,
                };
                self.surfaces.pot_moved.insert(place, Instant::now());
                self.ctl_set(&target, default);
            }
            Button::AssignPan => sfs.pot = PotMode::Pan,
            Button::AssignSend => {
                sfs.pot = match sfs.pot {
                    PotMode::Send(s) => PotMode::Send((s + 1) % SEND_SLOTS as u8),
                    _ => PotMode::Send(0),
                }
            }
            Button::AssignPlugin => {
                if sfs.pot != PotMode::Plugin {
                    sfs.pot = PotMode::Plugin;
                    sfs.page = 0;
                }
            }
            Button::BankLeft => self.surface_bank_by(-strips),
            Button::BankRight => self.surface_bank_by(strips),
            Button::ChannelLeft => self.surface_bank_by(-1),
            Button::ChannelRight => self.surface_bank_by(1),
            Button::NameValue => sfs.names = !sfs.names,
            Button::SmpteBeats => sfs.time = if sfs.time == TimeMode::Bars { TimeMode::Time } else { TimeMode::Bars },
            Button::Marker => sfs.marker_used = false,
            Button::Nudge => {
                sfs.nudge = !sfs.nudge;
                sfs.zoom &= !sfs.nudge;
            }
            Button::Zoom => {
                sfs.zoom = !sfs.zoom;
                sfs.nudge &= !sfs.zoom;
            }
            Button::Scrub => sfs.scrub = !sfs.scrub,
            Button::Read => self.surface_auto_mode(|m| if m == AutoMode::Read { AutoMode::Off } else { AutoMode::Read }),
            Button::Write => self.surface_auto_mode(|_| AutoMode::Write),
            Button::Touch => self.surface_auto_mode(|_| AutoMode::Touch),
            Button::Latch => self.surface_auto_mode(|_| AutoMode::Latch),
            Button::Click => {
                let on = !self.project.metronome;
                self.set_metronome(on);
            }
            Button::SoloClear => {
                let soloed: Vec<Id> = self.project.tracks.iter().filter(|t| t.solo).map(|t| t.id).collect();
                for id in soloed {
                    self.update_track(id, TrackPatch { solo: Some(false), ..Default::default() });
                }
            }
            Button::Stop => self.stop(),
            Button::Play => self.play(),
            _ => return false,
        }
        true
    }

    // ---- what the surfaces show -----------------------------------------------------------

    /// Sends each unit what changed in what it should show.
    fn surface_show(&mut self, now: Instant) {
        let tracks = self.surface_tracks();
        let bank = self.surface_bank(tracks.len());
        self.surfaces.bank = bank;
        let outputs = self.midi_out.keys();
        let frames: Vec<Option<Frame>> =
            self.surfaces.units.iter().map(|u| outputs.contains(&u.cfg.output).then(|| self.surface_frame(u, &tracks, bank, now))).collect();
        let Surfaces { units, out, .. } = &mut self.surfaces;
        for (u, frame) in units.iter_mut().zip(frames) {
            let Some(f) = frame else { continue };
            let port = u.cfg.output.clone();
            let mut send = |m: Vec<u8>| out.push((port.clone(), m));
            if !u.hui() && !u.meter_mode {
                u.meter_mode = true;
                (0..STRIPS as u8).for_each(|s| send(mcu::meter_mode(u.device(), s, METER_MODE)));
            }
            for (s, v) in f.faders.iter().enumerate() {
                let Some(v) = *v else { continue };
                if u.touched[s] {
                    continue;
                }
                let msgs = u.fader(s as u8, v);
                if let Some(ms) = u.faders[s].due(msgs, now) {
                    ms.into_iter().for_each(|m| send(m.to_vec()));
                }
            }
            for (b, on) in f.leds {
                if u.leds.get(&b) == Some(&on) {
                    continue;
                }
                u.leds.insert(b, on);
                if u.hui() {
                    hui::led(b, on).into_iter().flatten().for_each(|m| send(m.to_vec()));
                } else if let Some(m) = mcu::led(b, on) {
                    send(m.to_vec());
                }
            }
            for (s, v) in f.rings.iter().enumerate() {
                if u.rings[s] != Some(*v) {
                    u.rings[s] = Some(*v);
                    send(if u.hui() { hui::ring(s as u8, *v) } else { mcu::ring(s as u8, *v) }.to_vec());
                }
            }
            if u.lcd.as_ref() != Some(&f.lcd) {
                if u.hui() {
                    for (s, cell) in f.lcd.chunks(4).enumerate() {
                        if u.lcd.as_ref().is_none_or(|old| old.get(s * 4..s * 4 + 4) != Some(cell)) {
                            send(hui::scribble(s as u8, cell));
                        }
                    }
                } else {
                    for (offset, text) in lcd_changes(u.lcd.as_deref(), &f.lcd) {
                        send(mcu::lcd(u.device(), offset as u8, text));
                    }
                }
                u.lcd = Some(f.lcd);
            }
            // The digits, rightmost first as both protocols count them.
            let digits: Vec<Digit> = f.digits.iter().rev().copied().collect();
            if u.hui() {
                if !digits.is_empty() && (u.digits.len() != digits.len() || u.digits.iter().zip(&digits).any(|(a, b)| *a != Some(*b))) {
                    send(hui::timecode(&f.digits));
                    u.digits = digits.into_iter().map(Some).collect();
                }
            } else {
                u.digits.resize(digits.len(), None);
                for (i, d) in digits.iter().enumerate() {
                    if u.digits[i] != Some(*d) {
                        u.digits[i] = Some(*d);
                        // The assignment display's two come first in `digits`' last two.
                        send(mcu::digit(i as u8, *d).to_vec());
                    }
                }
            }
            for (s, level) in f.meters.iter().enumerate() {
                let (last, at) = u.meters[s];
                let again = *level > 0 && at.is_none_or(|t| now.saturating_duration_since(t) >= METER_EVERY);
                if *level != last || again || at.is_none() {
                    u.meters[s] = (*level, Some(now));
                    if u.hui() {
                        hui::meter(s as u8, *level).into_iter().for_each(|m| send(m.to_vec()));
                    } else {
                        send(mcu::meter(s as u8, *level).to_vec());
                    }
                }
            }
        }
    }

    /// What unit `u` should show, with the bank at `bank` of `tracks`.
    fn surface_frame(&self, u: &Unit, tracks: &[Id], bank: usize, now: Instant) -> Frame {
        let sfs = &self.surfaces;
        let travel = |t: &CtlTarget| match self.ctl_shown(t) {
            Some(Shown::Travel(v)) => Some(v),
            Some(Shown::Switch(on)) => Some(on as u8 as f32),
            None => None,
        };
        let recent = |at: Option<&Instant>| at.is_some_and(|t| now.saturating_duration_since(*t) < SHOW_VALUE);
        let params = if sfs.pot == PotMode::Plugin { self.surface_params() } else { None };
        let width = if u.hui() { 4 } else { CELL - 1 };
        let mut f = Frame { faders: [None; STRIPS + 1], leds: Vec::new(), rings: [0; STRIPS], lcd: Vec::new(), digits: Vec::new(), meters: [0; STRIPS] };
        let (mut top, mut bottom) = (Vec::new(), Vec::new());
        for s in 0..STRIPS {
            let place = u.cfg.offset as usize + s;
            let track = tracks.get(bank + place).copied();
            let t = track.and_then(|id| self.project.track(id));
            f.faders[s] = Some(track.and_then(|track| travel(&CtlTarget::Volume { track })).unwrap_or(0.0));
            let s8 = s as u8;
            f.leds.push((Button::Rec(s8), t.is_some_and(|t| t.armed)));
            f.leds.push((Button::Solo(s8), t.is_some_and(|t| t.solo)));
            f.leds.push((Button::Mute(s8), t.is_some_and(|t| t.mute)));
            f.leds.push((Button::Select(s8), track.is_some() && track == self.selected));
            f.meters[s] = track.and_then(|id| sfs.peaks.get(&id)).map(|p| sf::meter_level(*p)).unwrap_or(0);
            // The V-Pot: its ring, and the strip's two lines.
            let pot = self.surface_pot(place, track);
            let resolved = pot.as_ref().and_then(|p| self.ctl_resolve(p));
            let v = resolved.as_ref().and_then(travel);
            let mode = if sfs.pot == PotMode::Pan { RingMode::Dot } else { RingMode::Wrap };
            f.rings[s] = sf::ring_value(mode, v, false);
            let param = match (&resolved, &params) {
                (Some(CtlTarget::Plugin { param, .. }), Some(ps)) => ps.iter().find(|p| p.id == *param),
                _ => None,
            };
            let name = match (sfs.pot, param) {
                (PotMode::Plugin, Some(p)) => p.name.clone(),
                (PotMode::Plugin, None) => String::new(),
                _ => t.map(|t| t.name.clone()).unwrap_or_default(),
            };
            let value = match (&resolved, v) {
                (Some(CtlTarget::Pan { .. }), Some(v)) => sf::pan_text(v * 2.0 - 1.0),
                (Some(CtlTarget::Send { .. }), Some(v)) => sf::db_text(ctl::fader_to_db(v)),
                (Some(_), Some(v)) => sf::percent_text(v),
                _ => String::new(),
            };
            let label = match (sfs.pot, param) {
                (PotMode::Pan, _) => "Pan".to_string(),
                (PotMode::Send(n), _) => format!("Send{}", n + 1),
                (PotMode::Plugin, p) => p.map(|p| p.name.clone()).unwrap_or_default(),
            };
            let level = track.filter(|id| recent(sfs.fader_moved.get(id)) || u.touched[s]).and_then(|id| travel(&CtlTarget::Volume { track: id }));
            let line2 = if let Some(l) = level {
                sf::db_text(ctl::fader_to_db(l))
            } else if sfs.names && !recent(sfs.pot_moved.get(&place)) && pot.is_some() {
                label
            } else {
                value
            };
            if u.hui() {
                // Four characters: the name, or a value while it moves.
                let moving = level.is_some() || recent(sfs.pot_moved.get(&place));
                let text = if moving { short_value(&line2) } else { sf::fit(&name, width) };
                top.extend(sf::center(&text, width).bytes());
            } else {
                top.extend(sf::fit(&name, width).bytes());
                top.push(b' ');
                bottom.extend(sf::center(&line2, width).bytes());
                bottom.push(b' ');
            }
        }
        if u.has_master() {
            f.faders[MASTER as usize] = travel(&CtlTarget::MasterVolume);
        }
        f.lcd = top;
        if u.hui() {
            let assign = match sfs.pot {
                PotMode::Pan => "Pan".to_string(),
                PotMode::Send(n) => format!("Snd{}", n + 1),
                PotMode::Plugin => "Plug".into(),
            };
            f.lcd.extend(sf::fit(&assign, 4).bytes());
        } else {
            f.lcd.extend(bottom);
        }
        if u.cfg.kind.is_main() {
            self.surface_main(u, &mut f);
        }
        f
    }

    /// The master section: its lights and the time display.
    fn surface_main(&self, u: &Unit, f: &mut Frame) {
        let sfs = &self.surfaces;
        let playing = self.is_playing();
        let mode = self.selected.and_then(|t| self.auto_mode_of(t));
        let any_solo = self.project.tracks.iter().any(|t| t.solo);
        f.leds.extend([
            (Button::Play, playing),
            (Button::Stop, !playing),
            (Button::Record, self.is_recording()),
            (Button::Cycle, self.project.cycle.enabled),
            (Button::Click, self.project.metronome),
            (Button::Replace, self.prefs.recording.replace),
            (Button::Drop, self.project.punch.enabled),
            (Button::AssignPan, sfs.pot == PotMode::Pan),
            (Button::AssignSend, matches!(sfs.pot, PotMode::Send(_))),
            (Button::AssignPlugin, sfs.pot == PotMode::Plugin),
            (Button::Read, mode == Some(AutoMode::Read)),
            (Button::Write, mode == Some(AutoMode::Write)),
            (Button::Touch, mode == Some(AutoMode::Touch)),
            (Button::Latch, mode == Some(AutoMode::Latch)),
            (Button::Marker, sfs.held.contains(&Button::Marker)),
            (Button::Nudge, sfs.nudge),
            (Button::Zoom, sfs.zoom),
            (Button::Scrub, sfs.scrub),
            (Button::NameValue, sfs.names),
            (Button::SmpteLight, sfs.time == TimeMode::Time),
            (Button::BeatsLight, sfs.time == TimeMode::Bars),
            (Button::SoloClear, any_solo),
            (Button::RudeSolo, any_solo),
        ]);
        let pos = self.position().max(0) as f64;
        let secs = pos / self.sample_rate().max(1.0);
        let bbt = || self.tempo_map().bar_beat_at(pos);
        f.digits = match (u.hui(), sfs.time) {
            (true, TimeMode::Bars) => sf::hui_bbt_digits(&bbt()),
            (true, TimeMode::Time) => sf::hui_time_digits(secs),
            (false, TimeMode::Bars) => sf::bbt_digits(&bbt()),
            (false, TimeMode::Time) => sf::time_digits(secs),
        };
        if !u.hui() {
            // The assignment display, left of the time display: what the V-Pots drive.
            let assign = match sfs.pot {
                PotMode::Pan => "PN".to_string(),
                PotMode::Send(n) => format!("S{}", (n + 1) % 10),
                PotMode::Plugin => "PI".into(),
            };
            let mut digits: Vec<Digit> = assign.bytes().map(|ch| Digit { ch, dot: false }).collect();
            digits.append(&mut f.digits);
            f.digits = digits;
        }
    }
}

/// A value in a HUI's four characters: "-12.5" as "-12", "-inf" as "-oo".
fn short_value(text: &str) -> String {
    if text.chars().count() <= 4 {
        return text.to_string();
    }
    match text.parse::<f32>() {
        Ok(v) => format!("{:+.0}", v),
        Err(_) => text.chars().take(4).collect(),
    }
}

/// The stretches of a Mackie Control's LCD that differ from what it shows, one per line:
/// (offset, characters). Everything when what it shows is unknown.
fn lcd_changes<'a>(old: Option<&[u8]>, new: &'a [u8]) -> Vec<(usize, &'a [u8])> {
    let line = new.len() / 2;
    let mut out = Vec::new();
    for start in [0, line] {
        let end = (start + line).min(new.len());
        let differs = |i: usize| old.is_none_or(|o| o.get(i) != Some(&new[i]));
        let Some(first) = (start..end).find(|&i| differs(i)) else { continue };
        let last = (start..end).rev().find(|&i| differs(i)).unwrap_or(first);
        out.push((first, &new[first..=last]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcd_sends_what_changed() {
        let mut a = vec![b' '; 112];
        a[..12].copy_from_slice(b"Kick   Snare");
        a[56..66].copy_from_slice(b"  C      C");
        let all = lcd_changes(None, &a);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, 0);
        assert_eq!(all[1].0, a.len() / 2);
        assert_eq!(lcd_changes(Some(&a), &a), vec![]);
        let mut b = a.clone();
        b[7..12].copy_from_slice(b"Clap ");
        let line = a.len() / 2;
        b[line + 7] = b'L';
        assert_eq!(lcd_changes(Some(&a), &b), vec![(7, &b"Clap "[..]), (line + 7, &b"L"[..])]);
    }

    #[test]
    fn hui_values_fit_four_characters() {
        assert_eq!(short_value("C"), "C");
        assert_eq!(short_value("L32"), "L32");
        assert_eq!(short_value("-12.5"), "-12");
        assert_eq!(short_value("+3.0"), "+3.0");
        assert_eq!(short_value("-inf"), "-inf");
        assert_eq!(short_value("100%"), "100%");
    }
}
