//! MIDI outputs, for controller feedback (`control/feedback/`, `session/control/feedback.rs`) and control
//! surfaces (`session/control/surface/`). Like the inputs, every output port is connected unless
//! switched off in Settings › MIDI, and ports plugged in later are picked up by the same
//! 3-second refresh.
//!
//! Messages are sent from a thread of their own (`midi-out`), so a slow driver never holds up
//! the main thread, and never from the audio thread. The session hands it batches through a
//! channel; the connections are shared with it behind a lock that the refresh takes only to add
//! or drop one.

use std::sync::mpsc;
use std::sync::Arc;

use midir::{MidiOutput, MidiOutputConnection};
use parking_lot::Mutex;

use crate::control::controllers::{port_key, port_keys};
use crate::midi::input::{listed_port, MidiPort};

type Connections = Arc<Mutex<Vec<(String, MidiOutputConnection)>>>;

/// The connected MIDI outputs and the thread that sends to them.
pub struct MidiOut {
    connections: Connections,
    /// Keys of the ports switched off (as `MidiIo`'s).
    disabled: Vec<String>,
    /// Keys of ports connected since [`MidiOut::take_connected`] was last called.
    fresh: Vec<String>,
    tx: mpsc::Sender<Vec<(String, Vec<u8>)>>,
}

impl Default for MidiOut {
    fn default() -> Self {
        MidiOut::new()
    }
}

impl MidiOut {
    pub fn new() -> MidiOut {
        let connections: Connections = Arc::default();
        let (tx, rx) = mpsc::channel::<Vec<(String, Vec<u8>)>>();
        let conns = connections.clone();
        // Ends when the sender goes, with the session.
        let spawned = std::thread::Builder::new().name("midi-out".into()).spawn(move || {
            while let Ok(batch) = rx.recv() {
                let mut c = conns.lock();
                for (key, msg) in batch {
                    if let Some((name, conn)) = c.iter_mut().find(|(n, _)| port_key(n) == key) {
                        if let Err(e) = conn.send(&msg) {
                            log::warn!("MIDI output {name}: {e}");
                        }
                    }
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("no MIDI output thread: {e}");
        }
        MidiOut { connections, disabled: Vec::new(), fresh: Vec::new(), tx }
    }

    fn port_names() -> Vec<String> {
        if !super::system_ports() {
            return Vec::new();
        }
        let Ok(out) = MidiOutput::new("Marbble") else { return Vec::new() };
        out.ports().iter().filter_map(|p| out.port_name(p).ok()).filter(|n| listed_port(n)).collect()
    }

    /// Every output port the system offers, with its state.
    pub fn ports(&self) -> Vec<MidiPort> {
        let names = Self::port_names();
        let connected = self.connections.lock();
        names
            .into_iter()
            .map(|name| MidiPort {
                key: port_key(&name).to_string(),
                enabled: !self.is_off(&name),
                connected: connected.iter().any(|(n, _)| *n == name),
                auto_output: None,
                name,
            })
            .collect()
    }

    /// The connected outputs' keys (`controllers::port_key`).
    pub fn keys(&self) -> Vec<String> {
        self.connections.lock().iter().map(|(n, _)| port_key(n).to_string()).collect()
    }

    /// Whether any output is connected.
    pub fn any(&self) -> bool {
        !self.connections.lock().is_empty()
    }

    /// Ports to leave unconnected, by name or key; takes effect at once.
    pub fn set_disabled(&mut self, names: Vec<String>) {
        self.disabled = port_keys(&names);
        self.refresh();
    }

    /// Whether the port is switched off in Settings › MIDI.
    fn is_off(&self, name: &str) -> bool {
        let key = port_key(name);
        self.disabled.iter().any(|k| k == key)
    }

    /// The keys of outputs connected since the last call: their controllers need everything
    /// sent again.
    pub fn take_connected(&mut self) -> Vec<String> {
        std::mem::take(&mut self.fresh)
    }

    /// Hands messages to the output thread: (output key, message), in order.
    pub fn send(&self, batch: Vec<(String, [u8; 3])>) {
        self.send_raw(batch.into_iter().map(|(k, m)| (k, m.to_vec())).collect());
    }

    /// [`MidiOut::send`] for messages of any length (a control surface's system exclusive).
    pub fn send_raw(&self, batch: Vec<(String, Vec<u8>)>) {
        if !batch.is_empty() {
            let _ = self.tx.send(batch);
        }
    }

    /// Connects to any ports not already connected and drops ones that disappeared or were
    /// switched off.
    pub fn refresh(&mut self) {
        let names = Self::port_names();
        let disabled = &self.disabled;
        // Dropped outside the lock: closing a connection talks to the driver.
        let gone: Vec<_> = {
            let mut c = self.connections.lock();
            let (keep, gone): (Vec<_>, Vec<_>) =
                std::mem::take(&mut *c).into_iter().partition(|(n, _)| names.contains(n) && !disabled.iter().any(|k| k == port_key(n)));
            *c = keep;
            gone
        };
        drop(gone);
        if !super::system_ports() {
            return;
        }
        let Ok(probe) = MidiOutput::new("Marbble") else { return };
        for port in probe.ports() {
            let Ok(name) = probe.port_name(&port) else { continue };
            if !listed_port(&name) || self.is_off(&name) || self.connections.lock().iter().any(|(n, _)| *n == name) {
                continue;
            }
            let Ok(out) = MidiOutput::new("Marbble") else { continue };
            match out.connect(&port, "marbble-out") {
                Ok(c) => {
                    log::info!("MIDI output connected: {name}");
                    self.fresh.push(port_key(&name).to_string());
                    self.connections.lock().push((name, c));
                }
                Err(e) => log::warn!("MIDI output {name}: {e}"),
            }
        }
    }
}
