//! Writes a showcase project for screenshots of the app: a 24-bar song in A minor with a dozen
//! tracks: synthesized drums, percussion and a plucked guitar on audio tracks, all six synth models
//! and Marbble Sampler on instrument tracks, effects, two aux returns with sends, a VCA, arrangement
//! markers, an automation lane and an audio take folder with a swiped comp. Only built-in plug-ins, so it
//! looks the same everywhere.
//! `cargo run -p daw-engine --example showcase_project -- <out.marbble>`.
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{AutoParam, AutoPoint, Id, InputSel, Note, TrackKind, TrackSend, MASTER_ID};
use daw_engine::presets::CHANNEL_EQ;
use daw_engine::session::{MidiContent, TrackPatch};
use daw_engine::Session;

const SR: u32 = 48_000;
/// A bar at 120 bpm in 4/4.
const BAR: i64 = 96_000;
const BEAT: i64 = BAR / 4;

fn write_wav(path: &Path, ch: &[Vec<f32>]) {
    let spec = hound::WavSpec { channels: ch.len() as u16, sample_rate: SR, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..ch[0].len() {
        for c in ch {
            w.write_sample(c[i]).unwrap();
        }
    }
    w.finalize().unwrap();
}

struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 9) as f32 / (1 << 23) as f32 * 2.0 - 1.0
    }
}

/// Kick, snare and hats, `bars` bars, with a fill in the last bar.
fn drums(bars: usize) -> Vec<Vec<f32>> {
    let step = BEAT as usize / 4;
    let len = BAR as usize * bars;
    let (mut l, mut r) = (vec![0.0f32; len], vec![0.0f32; len]);
    let mut n = Noise(12345);
    for s in 0..bars * 16 {
        let at = s * step;
        let (bar, pos) = (s / 16, s % 16);
        let fill = bar == bars - 1 && pos >= 12;
        if [0, 6, 10].contains(&pos) && !fill {
            for i in 0..(SR as usize / 4).min(len - at) {
                let t = i as f32 / SR as f32;
                let f = 48.0 + 110.0 * (-t * 35.0).exp();
                let x = (TAU * f * t).sin() * (-t * 8.0).exp() * 0.9;
                l[at + i] += x;
                r[at + i] += x;
            }
        }
        if pos == 4 || pos == 12 || fill {
            let amp = if fill { 0.35 + 0.1 * (pos - 12) as f32 } else { 0.6 };
            for i in 0..(SR as usize / 5).min(len - at) {
                let t = i as f32 / SR as f32;
                let x = (n.next() * 0.65 + (TAU * 185.0 * t).sin() * 0.35) * (-t * 16.0).exp() * amp;
                l[at + i] += x;
                r[at + i] += x * 0.9;
            }
        }
        let hat = if pos % 2 == 0 { 0.16 } else { 0.08 };
        for i in 0..(SR as usize / 25).min(len - at) {
            let t = i as f32 / SR as f32;
            let x = n.next() * (-t * 110.0).exp() * hat;
            l[at + i] += x * 0.8;
            r[at + i] += x;
        }
    }
    vec![l, r]
}

/// A shaker on sixteenths, louder on the off-beats.
fn shaker(bars: usize) -> Vec<Vec<f32>> {
    let step = BEAT as usize / 4;
    let len = BAR as usize * bars;
    let mut out = vec![0.0f32; len];
    let mut n = Noise(777);
    for s in 0..bars * 16 {
        let at = s * step;
        let amp = if s % 4 == 2 { 0.22 } else { 0.1 };
        for i in 0..(step * 3 / 4).min(len - at) {
            let t = i as f32 / SR as f32;
            let env = (t * 300.0).min(1.0) * (-t * 40.0).exp();
            out[at + i] += n.next() * env * amp;
        }
    }
    vec![out]
}

/// Karplus-Strong plucks: (time in beats, MIDI pitch).
fn pluck(notes: &[(f32, u8)], beats: f32) -> Vec<Vec<f32>> {
    let len = (beats * BEAT as f32) as usize;
    let mut out = vec![0.0f32; len];
    let mut n = Noise(99);
    for &(at, pitch) in notes {
        let freq = 440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0);
        let period = (SR as f32 / freq) as usize;
        let mut buf: Vec<f32> = (0..period).map(|_| n.next()).collect();
        let start = (at * BEAT as f32) as usize;
        for i in 0..(SR as usize * 2) {
            if start + i >= len {
                break;
            }
            let k = i % period;
            let next = buf[(k + 1) % period];
            buf[k] = 0.995 * 0.5 * (buf[k] + next);
            out[start + i] += buf[k] * 0.3;
        }
    }
    vec![out]
}

/// A bell-like tone for the sampler: a few inharmonic partials, decaying.
fn bell(pitch: u8, loud: bool) -> Vec<Vec<f32>> {
    let freq = 440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0);
    let len = SR as usize * 2;
    let amp = if loud { 0.7 } else { 0.35 };
    let partials: [(f32, f32, f32); 4] = [(1.0, 1.0, 2.5), (2.76, 0.45, 4.0), (5.4, 0.25, 7.0), (8.93, if loud { 0.18 } else { 0.05 }, 11.0)];
    let x: Vec<f32> = (0..len)
        .map(|i| {
            let t = i as f32 / SR as f32;
            let attack = (t * 400.0).min(1.0);
            partials.iter().map(|&(m, a, d)| (TAU * freq * m * t).sin() * a * (-t * d).exp()).sum::<f32>() * amp * attack * 0.5
        })
        .collect();
    vec![x.clone(), x]
}

fn note(start: i64, length: i64, pitch: u8, velocity: u8) -> Note {
    Note { start, length, pitch, velocity, muted: false }
}

/// Am, F, C/E, G: a chord a bar.
const CHORDS: [[u8; 3]; 4] = [[57, 60, 64], [53, 57, 60], [52, 55, 60], [55, 59, 62]];
const ROOTS: [u8; 4] = [33, 29, 28, 31];

struct Song {
    s: Session,
}

impl Song {
    fn track(&mut self, kind: TrackKind, plugin: Option<&str>, name: &str, color: &str, volume_db: f32, pan: f32) -> Id {
        let input = if kind == TrackKind::Audio { InputSel::Mono(0) } else { InputSel::None };
        let t = self.s.add_track(kind, input, plugin.map(Into::into), None).unwrap();
        self.s.update_track(
            t,
            TrackPatch { name: Some(name.into()), color: Some(color.into()), volume_db: Some(volume_db), pan: Some(pan), ..Default::default() },
        );
        t
    }

    fn preset(&mut self, t: Id, plugin: &str, preset: &str) {
        self.s.load_preset(t, usize::MAX, plugin, preset, true).unwrap();
    }

    /// A MIDI region from `bar` for `bars` bars, named, with `notes` placed relative to its start.
    fn part(&mut self, t: Id, bar: i64, bars: i64, name: &str, notes: Vec<Note>) -> Id {
        let r = self.s.create_midi_region(t, bar * BAR, bars * BAR).unwrap();
        self.s.set_midi_content(vec![MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
        self.s.rename_region(r, name.into());
        r
    }

    fn insert(&mut self, t: Id, slot: usize, plugin: &str, preset: Option<&str>) {
        self.s.set_plugin(t, slot, plugin).unwrap();
        if let Some(p) = preset {
            let _ = self.s.load_preset(t, slot, plugin, p, true);
        }
    }

    fn send(&mut self, t: Id, slot: usize, bus: Id, level_db: f32) {
        self.s.set_send(t, slot, Some(TrackSend { bus, level_db, pre_fader: false, muted: false })).unwrap();
    }

    /// A Channel EQ with a factory preset at the top of the track's Audio FX, as its EQ
    /// thumbnail puts one.
    fn eq(&mut self, t: Id, preset: &str) {
        self.s.insert_channel_eq(t).unwrap();
        self.s.load_preset(t, 0, CHANNEL_EQ, preset, true).unwrap();
    }
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("usage: showcase_project <out.marbble>"));
    // Audio regions refer to their files where they are, so those go in the project's own
    // Audio Files folder; the sampler copies its files in, so those are written aside and removed.
    let audio = out.join("Audio Files");
    std::fs::create_dir_all(&audio).unwrap();
    let work = out.with_extension("sources");
    std::fs::create_dir_all(&work).unwrap();
    let mut song = Song { s: Session::new(AudioConfig { input_enabled: false, ..Default::default() }, Vec::new()) };
    // The project owns its folder from the start, so imported and sampler files are copied into it.
    song.s.dir = out.clone();
    song.s.set_tempo(120.0);
    song.s.project.key = "A minor".into();

    // Arrangement: Intro 4, Verse 8, Chorus 8, Outro 4.
    for (name, bars) in [("Intro", 4), ("Verse", 8), ("Chorus", 8), ("Outro", 4)] {
        let id = song.s.add_arrangement_marker(Some(name.into()));
        song.s.resize_arrangement_marker(id, bars * BAR);
    }

    // Audio: drums, shaker, guitar.
    let drum_file = audio.join("Drum Loop.wav");
    write_wav(&drum_file, &drums(4));
    let shaker_file = audio.join("Shaker.wav");
    write_wav(&shaker_file, &shaker(4));
    let guitar_file = audio.join("Guitar.wav");
    let mut picks = Vec::new();
    for (bar, chord) in CHORDS.iter().cycle().take(8).enumerate() {
        let b = bar as f32 * 4.0;
        let shape = [chord[0] - 12, chord[1], chord[2], chord[0] + 12, chord[2], chord[1]];
        for (i, &p) in shape.iter().enumerate() {
            picks.push((b + [0.0, 0.5, 1.0, 1.5, 2.5, 3.0][i], p));
        }
    }
    write_wav(&guitar_file, &pluck(&picks, 33.0));

    let drums = song.track(TrackKind::Audio, None, "Drums", "#d9546e", -4.0, 0.0);
    let perc = song.track(TrackKind::Audio, None, "Shaker", "#e0709a", -11.0, 0.35);
    for bar in [4, 8, 12, 16] {
        song.s.import_audio(&drum_file, drums, bar * BAR).unwrap();
    }
    for bar in [12, 16] {
        song.s.import_audio(&shaker_file, perc, bar * BAR).unwrap();
    }

    // Bass: Uno, eighths with octave jumps.
    let bass = song.track(TrackKind::Instrument, Some("builtin:uno"), "Bass", "#f2a63b", -6.0, 0.0);
    song.preset(bass, "builtin:uno", "Round Bottom");
    let mut line = Vec::new();
    for bar in 0..16i64 {
        let root = ROOTS[(bar % 4) as usize] + 12;
        for e in 0..8i64 {
            let p = if e == 3 || e == 7 { root + 12 } else { root };
            let v = if e % 2 == 0 { 108 } else { 84 };
            line.push(note(bar * BAR + e * BEAT / 2, BEAT / 2 - 600, p, v));
        }
    }
    song.part(bass, 4, 16, "Bass", line);

    let guitar = song.track(TrackKind::Audio, None, "Guitar", "#3aa776", -9.0, -0.4);
    song.s.import_audio(&guitar_file, guitar, 4 * BAR).unwrap();
    song.s.import_audio(&guitar_file, guitar, 12 * BAR).unwrap();

    // Keys: Sedici, chords on the off-beats.
    let keys = song.track(TrackKind::Instrument, Some("builtin:sedici"), "Keys", "#3b8fd9", -10.0, 0.2);
    song.preset(keys, "builtin:sedici", "Tine Keys");
    let comp = |bars: i64| {
        let mut n = Vec::new();
        for bar in 0..bars {
            let c = CHORDS[(bar % 4) as usize];
            for (k, &p) in c.iter().enumerate() {
                n.push(note(bar * BAR, BEAT * 3 / 2, p, 82 + k as u8 * 6));
                n.push(note(bar * BAR + BEAT * 2 + BEAT / 2, BEAT, p, 70 + k as u8 * 5));
            }
        }
        n
    };
    song.part(keys, 0, 12, "Keys", comp(12));
    song.part(keys, 12, 12, "Keys Chorus", comp(12));

    // Strings: Sei, held chords.
    let strings = song.track(TrackKind::Instrument, Some("builtin:sei"), "Strings", "#6a5be0", -13.0, -0.15);
    song.preset(strings, "builtin:sei", "Velvet Strings");
    let held = |bars: i64| {
        let mut n = Vec::new();
        for bar in 0..bars {
            for &p in &CHORDS[(bar % 4) as usize] {
                n.push(note(bar * BAR, BAR - 2000, p - 12, 76));
            }
        }
        n
    };
    song.part(strings, 0, 4, "Strings", held(4));
    song.part(strings, 12, 12, "Strings", held(12));

    // Brass stabs: Cinque, in the chorus.
    let brass = song.track(TrackKind::Instrument, Some("builtin:cinque"), "Brass", "#b14fc8", -12.0, 0.25);
    song.preset(brass, "builtin:cinque", "Warm Analog Brass");
    let mut stabs = Vec::new();
    for bar in 0..8i64 {
        for &p in &CHORDS[(bar % 4) as usize] {
            stabs.push(note(bar * BAR + BEAT * 3 / 2, BEAT / 3, p + 12, 112));
            stabs.push(note(bar * BAR + BEAT * 3, BEAT / 2, p + 12, 96));
        }
    }
    song.part(brass, 12, 8, "Stabs", stabs);

    // Arp: Onda, sixteenths through the chord.
    let arp = song.track(TrackKind::Instrument, Some("builtin:onda"), "Arp", "#35a8a0", -15.0, 0.4);
    song.preset(arp, "builtin:onda", "Glass Sync Pluck");
    let mut a = Vec::new();
    for bar in 0..8i64 {
        let c = CHORDS[(bar % 4) as usize];
        let up = [c[0] + 12, c[1] + 12, c[2] + 12, c[0] + 24];
        for s in 0..16i64 {
            a.push(note(bar * BAR + s * BEAT / 4, BEAT / 5, up[(s % 4) as usize], 70 + ((s * 7) % 5) as u8 * 9));
        }
    }
    song.part(arp, 12, 8, "Arp", a);

    // Bells: Marbble Sampler, two velocity layers on five notes.
    let bells = song.track(TrackKind::Instrument, Some("builtin:sampler"), "Bells", "#c9a227", -14.0, -0.3);
    let mut files = Vec::new();
    for (name, pitch) in [("C2", 48u8), ("G2", 55), ("C3", 60), ("G3", 67), ("C4", 72)] {
        for (layer, loud) in [("v1", false), ("v2", true)] {
            let f = work.join(format!("Bell {name} {layer}.wav"));
            write_wav(&f, &bell(pitch, loud));
            files.push(f);
        }
    }
    song.s.sampler_add_files(bells, &files, None).unwrap();
    let chime = [(0, 76u8), (3, 72), (6, 69), (8, 67), (12, 64), (14, 67)];
    let mut b = Vec::new();
    for bar in 0..8i64 {
        for &(s, p) in &chime {
            b.push(note(bar * BAR + s * BEAT / 4, BEAT, p, if s == 0 { 118 } else { 72 }));
        }
    }
    song.part(bells, 12, 8, "Bells", b);

    // Lead: Tre, a melody over the chorus with a lane opening its filter.
    let lead = song.track(TrackKind::Instrument, Some("builtin:tre"), "Lead", "#3aa0d9", -8.0, 0.0);
    song.preset(lead, "builtin:tre", "Singing Lead");
    let phrase: [(i64, i64, u8); 10] =
        [(0, 2, 76), (2, 1, 74), (3, 1, 72), (4, 3, 74), (8, 2, 72), (10, 2, 69), (12, 4, 71), (16, 3, 72), (20, 2, 76), (24, 6, 79)];
    let mut melody = Vec::new();
    for half in 0..2i64 {
        for (i, &(at, len, p)) in phrase.iter().enumerate() {
            let p = p + if half == 1 && i > 6 { 2 } else { 0 };
            melody.push(note(half * 4 * BAR + at * BEAT / 2, len * BEAT / 2 - 1200, p, 90 + (i as u8 * 3) % 20));
        }
    }
    song.part(lead, 12, 8, "Lead", melody);
    {
        use daw_engine::instrument::tre::params::param;
        let uid = song.s.project.track(lead).unwrap().instrument.as_ref().unwrap().uid;
        let lane = AutoParam::Plugin { uid, param: param::CUTOFF, name: "Marbble Tre: Cutoff".into() };
        let pt = |bar: i64, value: f32| AutoPoint { pos: bar * BAR, value, curve: 0.0 };
        song.s.set_automation_lane(lead, lane, vec![pt(12, 0.35), pt(16, 0.7), pt(19, 0.85), pt(20, 0.4)]);
    }

    // Solo: three plucked takes of a line over the chorus, packed into a take folder and comped:
    // Take 1 for two bars, Take 3 for three, Take 2 for the rest.
    let solo = song.track(TrackKind::Audio, None, "Solo", "#e07b39", -10.0, 0.25);
    let line = [(0.0, 76u8), (1.0, 74), (1.5, 72), (2.0, 74), (4.0, 72), (5.0, 69), (6.0, 71), (8.0, 72), (10.0, 76), (12.0, 79), (14.0, 77)];
    let mut regions = Vec::new();
    for (k, shift) in [(1, [0i8, -2, 0]), (2, [0, 0, -3]), (3, [2, 0, 0])] {
        let mut picks = Vec::new();
        for half in 0..2 {
            for (i, &(at, p)) in line.iter().enumerate() {
                let jitter = ((i * 7 + k * 3) % 5) as f32 * 0.03;
                picks.push((half as f32 * 16.0 + at + jitter, (p as i8 + shift[i % 3]) as u8));
            }
        }
        let f = audio.join(format!("Solo Take {k}.wav"));
        write_wav(&f, &pluck(&picks, 32.0));
        song.s.import_audio(&f, solo, 12 * BAR).unwrap();
        regions.push(*song.s.project.track(solo).unwrap().regions.iter().map(|r| &r.id).max().unwrap());
    }
    let folder = song.s.pack_take_folder(&regions).unwrap();
    let takes: Vec<Id> =
        song.s.project.track(solo).unwrap().regions.iter().find(|r| r.id == folder).unwrap().takes.as_ref().unwrap().takes.iter().map(|t| t.id).collect();
    for (take, from, to) in [(takes[0], 0, 2), (takes[2], 2, 5), (takes[1], 5, 8)] {
        song.s.set_comp_range(folder, Some(take), (12 + from) * BAR, (12 + to) * BAR);
    }

    // Aux returns, sends and inserts.
    let reverb = song.s.create_aux(Some("Reverb".into())).unwrap();
    let delay = song.s.create_aux(Some("Delay".into())).unwrap();
    song.insert(reverb, 0, "builtin:reverb", None);
    song.insert(delay, 0, "builtin:delay", None);
    song.insert(drums, 0, "builtin:compressor", None);
    song.insert(bass, 0, "builtin:compressor", None);
    song.insert(guitar, 0, "builtin:chorus", None);
    song.insert(keys, 0, "builtin:chorus", None);
    song.insert(lead, 0, "builtin:overdrive", None);
    song.insert(MASTER_ID, 0, "builtin:limiter", None);
    // Ahead of the plug-ins above.
    song.eq(drums, "Kick Punch");
    song.eq(guitar, "Acoustic Guitar");
    song.eq(bass, "Bass Clarity");
    for (t, rev, del) in [
        (drums, -18.0, None),
        (guitar, -12.0, Some(-16.0)),
        (keys, -14.0, None),
        (strings, -8.0, None),
        (bells, -6.0, Some(-12.0)),
        (lead, -10.0, Some(-9.0)),
        (arp, -14.0, Some(-14.0)),
        (solo, -11.0, Some(-13.0)),
    ] {
        song.send(t, 0, reverb, rev);
        if let Some(d) = del {
            song.send(t, 1, delay, d);
        }
    }
    song.s.create_vca(&[drums, perc]);
    song.s.select_track(Some(lead));

    let path = song.s.save(Some(out.clone())).unwrap();
    std::fs::remove_dir_all(&work).unwrap();
    println!("wrote {}", path.display());
}
