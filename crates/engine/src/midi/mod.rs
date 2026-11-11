//! MIDI to and from the outside world: the hardware input ports (`input.rs`), the output ports
//! that controller feedback and control surfaces send through (`output.rs`), and Standard MIDI
//! Files (`smf.rs`). What MIDI controls once it arrives is in `control/`; editing the notes of a
//! region is in `edit/midi_content.rs`.

pub mod input;
pub mod output;
pub mod smf;

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the system's MIDI ports are left alone. Always in the engine's unit tests, which
/// have no MIDI hardware to talk to and feed MIDI in by hand (`MidiIo::inject_from`): every test
/// session would otherwise open each of the system's ports, and on Windows, opening the system's
/// MIDI output in session after session, several at a time, now and then crashed the whole test
/// process inside the system's MIDI driver (`wdmaud.drv`, under `midiOutOpen`; issue #387).
static NO_SYSTEM_PORTS: AtomicBool = AtomicBool::new(cfg!(test));

/// Whether the system's MIDI ports are listed and opened.
pub(crate) fn system_ports() -> bool {
    !NO_SYSTEM_PORTS.load(Ordering::Relaxed)
}

/// From now on, this process lists and opens none of the system's MIDI ports: for a headless
/// check that feeds MIDI in by hand. Otherwise whatever else plays on the system (a keyboard,
/// another check's virtual ports) reaches its instrument too (issue #38). Call it before making
/// a session.
pub fn leave_system_ports_alone() {
    NO_SYSTEM_PORTS.store(true, Ordering::Relaxed);
}
