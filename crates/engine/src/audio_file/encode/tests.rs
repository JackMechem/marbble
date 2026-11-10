//! Writing WAV, AIFF and FLAC: dither, headers, sample rates, and round trips through the decoder.

use super::*;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("marbble-audio-file-{}-{name}", std::process::id()))
}

/// A second of stereo: a sine on the left, a quieter one with noise on the right, and a
/// few samples past full scale (which must clip, not wrap).
fn signal(n: usize) -> Vec<Vec<f32>> {
    let mut seed = 1u32;
    let mut noise = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
    };
    let l: Vec<f32> = (0..n).map(|i| 0.8 * (i as f32 * 0.031).sin()).collect();
    let mut r: Vec<f32> = (0..n).map(|i| 0.3 * (i as f32 * 0.007).sin() + 0.05 * noise()).collect();
    r[10] = 1.5;
    r[11] = -1.5;
    vec![l, r]
}

/// What the integer formats should hold: the quantized samples, per channel.
fn quantized(audio: &[Vec<f32>], depth: BitDepth) -> Vec<Vec<i32>> {
    let mut q = Quantizer::new(depth);
    let mut out = vec![Vec::new(); audio.len()];
    for i in 0..frames(audio) {
        for (c, ch) in audio.iter().enumerate() {
            out[c].push(q.int(ch[i]));
        }
    }
    out
}

/// Decodes `path` and scales it back to integers of `bits`.
fn decoded_ints(path: &Path, bits: u16) -> (u32, Vec<Vec<i32>>) {
    let d = crate::audio_file::decode::decode(path).unwrap();
    let scale = (1i64 << (bits - 1)) as f64;
    let ints = d.channels.iter().map(|c| c.iter().map(|&s| (s as f64 * scale) as i32).collect()).collect();
    (d.sample_rate, ints)
}

#[test]
fn sixteen_bit_dither_is_small_and_unbiased() {
    let dir = scratch("dither");
    let p = dir.join("d.wav");
    let quiet = vec![vec![0.25f32; 48000]];
    write(&p, AudioFormat::Wav, 48000, &quiet, BitDepth::Int16).unwrap();
    let mut r = hound::WavReader::open(&p).unwrap();
    assert_eq!(r.spec().bits_per_sample, 16);
    let s: Vec<i16> = r.samples::<i16>().map(|x| x.unwrap()).collect();
    let mean = s.iter().map(|&v| v as f64).sum::<f64>() / s.len() as f64;
    assert!((mean - 0.25 * 32767.0).abs() < 0.1, "mean {mean}");
    assert!(s.iter().all(|&v| (v as f32 - 0.25 * 32767.0).abs() <= 1.5));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn aiff_round_trips_through_the_decoder() {
    let dir = scratch("aiff");
    let audio = signal(9001);
    for depth in [BitDepth::Int16, BitDepth::Int24] {
        let p = dir.join(format!("t{}.aif", depth.bits()));
        write(&p, AudioFormat::Aiff, 44100, &audio, depth).unwrap();
        let (rate, got) = decoded_ints(&p, depth.bits());
        assert_eq!(rate, 44100);
        assert_eq!(got, quantized(&audio, depth), "{depth:?}");
    }
    let p = dir.join("f.aif");
    write(&p, AudioFormat::Aiff, 96000, &audio, BitDepth::Float32).unwrap();
    let d = crate::audio_file::decode::decode(&p).unwrap();
    assert_eq!(d.sample_rate, 96000);
    assert_eq!(d.channels, audio, "float AIFF-C is lossless, overs included");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn aiff_headers_are_valid() {
    let dir = scratch("aiff-head");
    // 24-bit mono, odd frame count: 9 data bytes, so the sound chunk needs a pad byte.
    let p = dir.join("odd.aif");
    write(&p, AudioFormat::Aiff, 48000, &[vec![0.5, -0.5, 0.25]], BitDepth::Int24).unwrap();
    let b = std::fs::read(&p).unwrap();
    let be32 = |at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    assert_eq!(&b[0..4], b"FORM");
    assert_eq!(&b[8..12], b"AIFF");
    assert_eq!(be32(4) as usize, b.len() - 8);
    assert_eq!(b.len() % 2, 0);
    assert_eq!(&b[12..16], b"COMM");
    assert_eq!(be32(16), 18);
    assert_eq!(i16::from_be_bytes([b[20], b[21]]), 1, "channels");
    assert_eq!(be32(22), 3, "frames");
    assert_eq!(i16::from_be_bytes([b[26], b[27]]), 24, "bits");
    assert_eq!(&b[28..38], &aiff::extended(48000));
    assert_eq!(&b[38..42], b"SSND");
    assert_eq!(be32(42), 8 + 9);
    assert_eq!(&b[54..57], &[0x40, 0x00, 0x00], "0.5 at 24 bits, big-endian");
    assert_eq!(b.len(), 54 + 9 + 1);
    let d = crate::audio_file::decode::decode(&p).unwrap();
    assert_eq!(d.channels[0].len(), 3);

    let p = dir.join("float.aif");
    write(&p, AudioFormat::Aiff, 48000, &[vec![0.5; 4], vec![-0.5; 4]], BitDepth::Float32).unwrap();
    let b = std::fs::read(&p).unwrap();
    assert_eq!(&b[8..12], b"AIFC");
    assert_eq!(&b[12..16], b"FVER");
    assert_eq!(u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize, b.len() - 8);
    let comm = b.windows(4).position(|w| w == b"COMM").unwrap();
    assert_eq!(&b[comm + 26..comm + 30], b"fl32");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sample_rates_as_80_bit_extended() {
    assert_eq!(aiff::extended(44100), [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
    assert_eq!(aiff::extended(48000), [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
    assert_eq!(aiff::extended(96000), [0x40, 0x0F, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
    assert_eq!(aiff::extended(1), [0x3F, 0xFF, 0x80, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn flac_decodes_back_bit_exact() {
    let dir = scratch("flac");
    let audio = signal(20000);
    for depth in [BitDepth::Int16, BitDepth::Int24] {
        let p = dir.join(format!("t{}.flac", depth.bits()));
        write(&p, AudioFormat::Flac, 48000, &audio, depth).unwrap();
        let b = std::fs::read(&p).unwrap();
        assert_eq!(&b[0..4], b"fLaC");
        let (rate, got) = decoded_ints(&p, depth.bits());
        assert_eq!(rate, 48000);
        assert_eq!(got, quantized(&audio, depth), "{depth:?}");
        // Compressed: smaller than the same samples as WAV, even with a noisy channel.
        let w = dir.join(format!("t{}.wav", depth.bits()));
        write(&w, AudioFormat::Wav, 48000, &audio, depth).unwrap();
        let (fl, wl) = (b.len(), std::fs::metadata(&w).unwrap().len() as usize);
        assert!(fl < wl, "{depth:?}: FLAC {fl} bytes, WAV {wl}");
    }
    // Mono and very short files too.
    let p = dir.join("short.flac");
    write(&p, AudioFormat::Flac, 44100, &[vec![0.1, 0.2, -0.3]], BitDepth::Int24).unwrap();
    assert_eq!(decoded_ints(&p, 24).1, quantized(&[vec![0.1, 0.2, -0.3]], BitDepth::Int24));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn flac_refuses_float() {
    let p = scratch("flac-float").join("f.flac");
    assert!(write(&p, AudioFormat::Flac, 48000, &[vec![0.0; 10]], BitDepth::Float32).is_err());
    assert!(!p.exists());
}

#[test]
fn every_format_holds_the_same_samples() {
    let dir = scratch("same");
    let audio = signal(5000);
    let mut all = Vec::new();
    for f in [AudioFormat::Wav, AudioFormat::Aiff, AudioFormat::Flac] {
        let p = dir.join(format!("t.{}", f.extension()));
        write(&p, f, 48000, &audio, BitDepth::Int16).unwrap();
        all.push(decoded_ints(&p, 16).1);
    }
    assert_eq!(all[0], all[1]);
    assert_eq!(all[0], all[2]);
    let _ = std::fs::remove_dir_all(&dir);
}
