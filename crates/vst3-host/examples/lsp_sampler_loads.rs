//! How often LSP's Multi-Sampler loses a sample while loading (issue #37), with this crate alone:
//! no engine, no session. Each try makes a new instance, loads two sines into its first two
//! instruments through its state, as a project would, and plays a note on each until both are
//! heard, or gives up after a few seconds and counts the try as a loss.
//!
//! The loss is LSP's (docs/DEVELOPING.md › Headless checks): its task queue reports a task it
//! failed to queue as queued, and the sampler then waits for it for ever. `VST3_PATH` set to a
//! folder with another `lsp-plugins.vst3` (a patched build) tries that one instead.
//!
//! usage: lsp_sampler_loads <scratch dir> [tries (300)] [flat|paced (flat)] [instances (1)]
//! `flat` calls `process` as fast as it returns, `paced` once per 512 frames at 48 kHz. Several
//! instances load at once, each played on a thread of its own, as on the engine's worker threads.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use vst3_host::scan::{default_search_paths, find_bundles, scan_bundle};
use vst3_host::{instantiate, MidiEvent, PluginController, PluginDescriptor, PluginProcessor, TransportInfo};

const PLUGIN: &str = "Multi-Sampler x12 DirectOut";
const RATE: u32 = 48_000;
const BLOCK: usize = 512;
/// The first instrument's default note (octave 4, A), and the note the second is moved to.
const NOTES: [u8; 2] = [57, 59];
/// Longer than any load takes when it works: it takes milliseconds.
const PATIENCE: Duration = Duration::from_secs(3);

fn main() {
    let mut args = std::env::args().skip(1);
    let scratch = PathBuf::from(args.next().expect("usage: lsp_sampler_loads <scratch dir> [tries] [flat|paced]"));
    let tries: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(300);
    let paced = args.next().as_deref() == Some("paced");
    let instances: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let Some(desc) = find_plugin() else {
        println!("SKIP {PLUGIN} (lsp-plugins) isn't installed");
        return;
    };
    println!("{} from {}", desc.name, desc.path.display());
    std::fs::create_dir_all(&scratch).unwrap();
    let files = [scratch.join("one.wav"), scratch.join("two.wav")];
    write_sine(&files[0], 1000.0);
    write_sine(&files[1], 2000.0);
    let mut lost = 0;
    for i in 0..tries {
        let loaded: Vec<_> = (0..instances).map(|_| load(&desc, &files)).collect();
        let played: Vec<_> = std::thread::scope(|s| {
            let threads: Vec<_> = loaded.into_iter().map(|(p, c)| s.spawn(move || (listen(p, paced), c))).collect();
            threads.into_iter().map(|t| t.join().unwrap().0).collect()
        });
        for which in played.into_iter().flatten() {
            lost += 1;
            println!("  try {i}: instrument {which} never played");
        }
    }
    println!("{lost} of {} loads lost a sample ({}, {instances} at a time)", tries * instances, if paced { "paced" } else { "flat out" });
}

fn find_plugin() -> Option<PluginDescriptor> {
    find_bundles(&default_search_paths())
        .iter()
        .filter(|b| b.to_string_lossy().contains("lsp-plugins"))
        .find_map(|b| scan_bundle(b).ok()?.into_iter().find(|d| d.name == PLUGIN))
}

/// A new instance with the two sines in its state, loaded as a project loads it.
fn load(desc: &PluginDescriptor, files: &[PathBuf; 2]) -> (PluginProcessor, PluginController) {
    let (p, c) = instantiate(desc, RATE as f64, BLOCK).unwrap();
    let mut state = c.save_state().unwrap();
    set_string(&mut state.component, "sf_0_0", files[0].to_str().unwrap());
    set_string(&mut state.component, "sf_1_0", files[1].to_str().unwrap());
    set_float(&mut state.component, "note_1", 11.0);
    c.load_state(&state);
    (p, c)
}

/// Plays the instance until both instruments are heard: None, or the one that never was.
fn listen(mut p: PluginProcessor, paced: bool) -> Option<usize> {
    let t = TransportInfo { sample_rate: RATE as f64, tempo: 120.0, sig_num: 4, sig_den: 4, ..Default::default() };
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut heard = [false; 2];
    let start = Instant::now();
    // A note every 8 blocks, the two instruments in turn, each with a note-off before the next.
    for block in 0usize.. {
        if heard == [true, true] {
            return None;
        }
        if start.elapsed() > PATIENCE {
            return heard.iter().position(|h| !h);
        }
        let which = (block / 8) % 2;
        let midi: Vec<MidiEvent> = match block % 8 {
            0 => vec![MidiEvent { offset: 0, data: [0x90, NOTES[which], 127] }],
            7 => vec![MidiEvent { offset: 0, data: [0x80, NOTES[which], 0] }],
            _ => vec![],
        };
        l.fill(0.0);
        r.fill(0.0);
        p.process(&mut l, &mut r, &midi, &t);
        if l.iter().chain(&r).any(|s| s.abs() > 0.01) {
            heard[which] = true;
        }
        if paced {
            std::thread::sleep(Duration::from_secs_f64(BLOCK as f64 / RATE as f64));
        }
    }
    unreachable!()
}

/// LSP's state is a list of named ports: a length, the name, a type letter, the value. Replaces
/// a port's value, keeping the rest.
fn set_port(state: &mut Vec<u8>, name: &str, kind: u8, old_len: usize, value: &[u8]) {
    let mut key = vec![name.len() as u8];
    key.extend_from_slice(name.as_bytes());
    key.push(kind);
    let at = state.windows(key.len()).position(|w| w == key).unwrap_or_else(|| panic!("no port {name} in the plug-in's state")) + key.len();
    state.splice(at..at + old_len, value.iter().copied());
}

fn set_float(state: &mut Vec<u8>, name: &str, v: f32) {
    set_port(state, name, b'f', 4, &v.to_le_bytes());
}

/// A string port, empty in a fresh instance's state (one zero byte): its length as a
/// variable-length integer, then the bytes.
fn set_string(state: &mut Vec<u8>, name: &str, v: &str) {
    let mut value = Vec::new();
    let mut n = v.len();
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        value.push(if n > 0 { b | 0x80 } else { b });
        if n == 0 {
            break;
        }
    }
    value.extend_from_slice(v.as_bytes());
    set_port(state, name, b's', 1, &value);
}

/// 0.2 s of a sine at half scale, as a mono 32-bit float WAV.
fn write_sine(path: &Path, hz: f32) {
    let samples: Vec<f32> = (0..RATE / 5).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin()).collect();
    let data = (samples.len() * 4) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    // A 16-byte format chunk: IEEE float, one channel, the rate, bytes per second and per frame,
    // and bits per sample.
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}
