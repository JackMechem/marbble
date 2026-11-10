//! Audio files: reading any common format into the project's sample rate (`decode/`), writing
//! bounces and exports as WAV, AIFF or FLAC (`encode/`), the decoded audio a project plays from
//! (`clip.rs`), and repairing recordings a crash cut short (`wav_repair.rs`).

pub mod clip;
pub mod decode;
pub mod encode;
pub mod wav_repair;
