//! Writing bounced and exported audio as WAV, AIFF or FLAC, as Logic's Bounce dialog offers.
//!
//! Every format shares one [`Quantizer`], so a 16-bit WAV, AIFF and FLAC of the same render hold
//! exactly the same samples (dither included). Reading these files back is `decode`'s job.

use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

mod aiff;
mod flac;
mod wav;

/// File format of a bounce or export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    #[default]
    Wav,
    /// AIFF for integer samples; AIFF-C (`fl32`) for 32-bit float.
    Aiff,
    /// Lossless FLAC, 16 or 24-bit.
    Flac,
}

impl AudioFormat {
    /// The file name extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Wav => "wav",
            AudioFormat::Aiff => "aif",
            AudioFormat::Flac => "flac",
        }
    }

    /// FLAC stores integer samples only.
    pub fn supports(self, depth: BitDepth) -> bool {
        !(self == AudioFormat::Flac && depth == BitDepth::Float32)
    }
}

/// Sample format of a bounced file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    /// 16-bit integer, with TPDF dither.
    Int16,
    Int24,
    Float32,
}

impl BitDepth {
    pub fn bits(self) -> u16 {
        match self {
            BitDepth::Int16 => 16,
            BitDepth::Int24 => 24,
            BitDepth::Float32 => 32,
        }
    }
}

/// Turns float samples into a file's integer samples, clipping at full scale. 16-bit gets TPDF
/// dither (±1 LSB triangular noise) so quiet passages fade into noise rather than distortion.
/// Feed it samples in file order (frame by frame, channels interleaved): the dither noise is a
/// fixed sequence, which keeps renders repeatable and every format bit-identical.
pub struct Quantizer {
    depth: BitDepth,
    seed: u32,
}

impl Quantizer {
    pub fn new(depth: BitDepth) -> Self {
        Quantizer { depth, seed: 0x9e37_79b9 }
    }

    /// xorshift32: plenty for dither noise.
    fn uniform(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    /// The integer sample for `s` (full scale ±1.0). Float32 has none; it gives the 24-bit value.
    pub fn int(&mut self, s: f32) -> i32 {
        match self.depth {
            BitDepth::Int16 => {
                let v = s * 32767.0 + (self.uniform() - self.uniform());
                v.round().clamp(-32768.0, 32767.0) as i32
            }
            BitDepth::Int24 | BitDepth::Float32 => (s as f64 * 8_388_607.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32,
        }
    }
}

fn frames(audio: &[Vec<f32>]) -> usize {
    audio.first().map(|c| c.len()).unwrap_or(0)
}

/// Writes `audio` (one Vec per channel, all the same length) to `path` in the given format,
/// creating the folder if needed.
pub fn write(path: &Path, format: AudioFormat, rate: u32, audio: &[Vec<f32>], depth: BitDepth) -> Result<()> {
    if !format.supports(depth) {
        return Err(anyhow!("FLAC files can't hold 32-bit float audio: choose 16 or 24-bit"));
    }
    if audio.is_empty() {
        return Err(anyhow!("no audio to write"));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match format {
        AudioFormat::Wav => wav::write(path, rate, audio, depth),
        AudioFormat::Aiff => aiff::write(path, rate, audio, depth),
        AudioFormat::Flac => flac::write(path, rate, audio, depth),
    }
}

#[cfg(test)]
mod tests;
