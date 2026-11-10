//! Automation on the session side: writing lanes from control moves while playing (`write.rs`),
//! editing them by hand (`edits.rs`), and the lanes beyond a track's own strip and which lanes are
//! shown (`lanes.rs`). Evaluating and
//! splicing curves is in `edit/automation/`.

pub mod edits;
pub mod lanes;
pub mod write;
