//! Session tests that span several areas, beside the tests each file keeps of its own, and the
//! helpers they share.

mod channel_eq;
mod comping;
mod held_notes;
mod instruments;
mod presets;

use std::path::PathBuf;

use crate::audio_io::AudioConfig;
use crate::session::Session;

/// A session on the null device whose project folder is a scratch folder named after `tag`.
pub(crate) fn scratch_session(tag: &str) -> (Session, PathBuf) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let scratch = std::env::temp_dir().join(format!("marbble-options-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    s.dir = scratch.join("Song.marbble");
    (s, scratch)
}
