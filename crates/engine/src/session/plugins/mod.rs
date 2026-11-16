//! What sits in a strip's instrument and insert slots: loading and bypassing plug-ins (`slots.rs`),
//! built-in effects and instruments, Marbble Sampler, reordering inserts, presets, pasting one
//! plug-in's settings into another (`settings.rs`), side-chains, and for VST3 and CLAP plug-ins
//! (`hosted.rs`) their saved state, the undo of edits made in their own editor and Compare
//! (`compare.rs`).

pub mod builtin;
pub mod compare;
pub mod editor_undo;
pub mod hosted;
pub mod insert_order;
pub mod presets;
pub mod sampler;
pub mod settings;
pub mod sidechain;
pub mod slots;
pub mod states;
