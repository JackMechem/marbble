//! What sits in a strip's instrument and insert slots: loading and bypassing plug-ins (`slots.rs`),
//! built-in effects and instruments, Marbble Sampler, reordering inserts, presets, side-chains, and
//! for VST3s their saved state and the undo of edits made in their own editor.

pub mod builtin;
pub mod editor_undo;
pub mod insert_order;
pub mod presets;
pub mod sampler;
pub mod sidechain;
pub mod slots;
pub mod states;
