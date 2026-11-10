//! MIDI to and from the outside world: the hardware input ports (`input.rs`), the output ports
//! that controller feedback and control surfaces send through (`output.rs`), and Standard MIDI
//! Files (`smf.rs`). What MIDI controls once it arrives is in `control/`; editing the notes of a
//! region is in `edit/midi_content.rs`.

pub mod input;
pub mod output;
pub mod smf;
