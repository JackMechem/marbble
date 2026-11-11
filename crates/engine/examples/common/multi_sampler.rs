//! LSP's Multi-Sampler x12 DirectOut, the multi-output instrument the checks measure with
//! (`multiout_check`, `multiout_freeze_check`): finding it, giving its first two instruments a
//! 1 kHz and a 2 kHz sine through its state (it has no parameter for a sample's file), and waiting
//! until it has loaded them, which it does on a thread of its own and now and then fails to do
//! (issue #37).
// Each check uses only some of these.
#![allow(dead_code)]

use std::path::Path;
use std::time::{Duration, Instant};

use base64::Engine as _;
use daw_engine::model::Id;
use daw_engine::Session;
use vst3_host::PluginDescriptor;

pub const PLUGIN: &str = "Multi-Sampler x12 DirectOut";
/// The first instrument's default note in the plug-in: octave 4, A, which it counts as 57. The
/// second plays two semitones up.
pub const NOTE_ONE: u8 = 57;

pub fn find_plugin() -> Option<PluginDescriptor> {
    for b in vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths()) {
        if b.to_string_lossy().contains("lsp-plugins") {
            if let Some(d) = vst3_host::scan::scan_bundle(&b).ok()?.into_iter().find(|d| d.name == PLUGIN) {
                return Some(d);
            }
        }
    }
    None
}

/// The loudest sample of `a` over `range`.
pub fn peak(a: &[Vec<f32>], range: (i64, i64)) -> f32 {
    a.iter().flat_map(|c| &c[range.0 as usize..(range.1 as usize).min(c.len())]).fold(0.0, |m, x| m.max(x.abs()))
}

/// LSP's state is a list of named ports: a length, the name, a type letter, the value. Replaces
/// a port's value, keeping the rest.
fn set_port(state: &mut Vec<u8>, name: &str, kind: u8, old_len: impl Fn(&[u8]) -> usize, value: &[u8]) {
    let mut key = vec![name.len() as u8];
    key.extend_from_slice(name.as_bytes());
    key.push(kind);
    let at = state.windows(key.len()).position(|w| w == key).unwrap_or_else(|| panic!("no port {name} in the plug-in's state")) + key.len();
    let n = old_len(&state[at..]);
    state.splice(at..at + n, value.iter().copied());
}

fn set_float(state: &mut Vec<u8>, name: &str, v: f32) {
    set_port(state, name, b'f', |_| 4, &v.to_le_bytes());
}

/// A string port: its length as a variable-length integer, then the bytes.
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
    // An empty string is one zero byte.
    set_port(state, name, b's', |_| 1, &value);
}

/// Writes 0.2 s of a stereo sine at half scale.
pub fn sine(path: &Path, hz: f32, rate: u32) {
    // Shorter than the time between the notes: the sampler plays the whole of it.
    let x: Vec<f32> = (0..rate / 5).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin()).collect();
    daw_engine::audio_file::decode::write_wav(path, rate, &[x.clone(), x]).unwrap();
}

/// Loads the sines into instruments 1 and 2, the first on its default note and the second two
/// semitones up, without touching the rest.
pub fn load_samples(s: &mut Session, inst: Id, one: &Path, two: &Path) {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut copy = s.copy_plugin(inst, usize::MAX).unwrap();
    let saved = copy.state.clone().unwrap();
    let (component, controller) = saved.split_once(':').unwrap();
    let mut state = b64.decode(component).unwrap();
    set_string(&mut state, "sf_0_0", one.to_str().unwrap());
    set_string(&mut state, "sf_1_0", two.to_str().unwrap());
    set_float(&mut state, "note_1", 11.0);
    copy.state = Some(format!("{}:{controller}", b64.encode(&state)).into());
    s.paste_plugin(&[inst], usize::MAX, &copy).unwrap();
}

/// Waits until a render of track `id` alone (through its fader) sounds within `window`.
pub fn wait_for_samples(s: &mut Session, id: Id, window: (i64, i64)) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        let iso = s.isolate_track(id, true, true);
        if peak(&s.render(0, window.1, 0, Some(iso), &mut |_| {}).unwrap(), window) > 0.01 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}
