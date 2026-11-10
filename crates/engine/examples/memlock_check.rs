//! Settings › Audio › Lock audio memory (`memlock.rs`): writes a drum session (a kick and an
//! overhead track with four takes each, packed into take folders, beside the all-features project's
//! instruments and effects, `common/all_features.rs`), opens it in a fresh process the way the app
//! does, plays it, and reports what the process holds: mapped, resident, locked and swapped, and
//! how much of it is the clips. Then it locks the memory as the app does at startup and checks the
//! outcome against this process's RLIMIT_MEMLOCK: unlimited locks (VmLck holds what is resident,
//! and audio imported and played after it still works); a finite limit locks nothing and says so.
//! Switching the setting off lets go of the lock, and on again takes it back. Elsewhere than Linux
//! the switch does nothing.
//! usage: memlock_check <scratch dir> [minutes per take] [--without-onfault]
//!   --without-onfault  lock with plain mlockall(MCL_CURRENT | MCL_FUTURE), to compare what that costs
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use daw_engine::audio_io::memlock::{MemoryLock, ProcessMemory};
use daw_engine::audio_io::AudioConfig;
use daw_engine::model::TrackKind;
use daw_engine::Session;

#[path = "common/all_features.rs"]
mod all_features;
use all_features::*;

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn config() -> AudioConfig {
    AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() }
}

fn mb(kb: u64) -> String {
    format!("{:.0} MB", kb as f64 / 1024.0)
}

fn report(when: &str) -> ProcessMemory {
    let m = ProcessMemory::read();
    println!("  {when}: mapped {}, resident {}, locked {}, swapped {}", mb(m.mapped_kb), mb(m.resident_kb), mb(m.locked_kb), mb(m.swapped_kb));
    m
}

/// A mono take: decaying hits on every beat over noise, so no page of it is zero.
fn write_take(path: &Path, minutes: f64, seed: u32) {
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let mut n = seed;
    let beat = RATE as usize / 2;
    for i in 0..(minutes * 60.0 * RATE as f64) as usize {
        n = n.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((n >> 9) as f32 / (1 << 23) as f32 - 1.0) * 0.01;
        let t = (i % beat) as f32 / RATE as f32;
        w.write_sample(noise + (t * 60.0 * std::f32::consts::TAU).sin() * (-t * 12.0).exp() * 0.5).unwrap();
    }
    w.finalize().unwrap();
}

/// Writes the drum session into `scratch` and returns its folder.
fn build(scratch: &Path, minutes: f64) -> PathBuf {
    let mut s = Session::new(config(), Vec::new());
    let dir = scratch.join("Drums.marbble");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Audio Files")).unwrap();
    s.dir = dir.clone();
    let mut w = base(&mut s, scratch);
    for (_, step) in steps() {
        step(&mut s, &mut w);
    }
    for (plugin, preset) in MODELS {
        add_model(&mut s, plugin, preset);
    }
    add_tre(&mut s);
    for (k, name) in ["Kick", "Overhead"].into_iter().enumerate() {
        let t = s.add_track(TrackKind::Audio, daw_engine::model::InputSel::Mono(k as u16), None, None).unwrap();
        let mut ids = Vec::new();
        for take in 0..4 {
            let file = dir.join("Audio Files").join(format!("{name}#{:02}.wav", take + 1));
            write_take(&file, minutes, 7 + take * 31 + k as u32);
            ids.push(s.import_audio(&file, t, 0).unwrap());
        }
        s.pack_take_folder(&ids).unwrap();
    }
    s.save(Some(dir.clone())).unwrap();
    dir
}

/// Opens the session in this process, plays it, then locks and unlocks its memory.
fn open_and_lock(project: &Path, onfault: bool) {
    let mut s = Session::new(config(), Vec::new());
    let start = report("a new session");
    s.open(project).unwrap();
    let clip_kb: u64 = s.project.clips.iter().filter_map(|c| s.clip_audio(c.id)).map(|c| (c.frames() * c.channels.len() * 4) as u64 / 1024).sum();
    let opened = report("the project open");
    println!(
        "  {} clips hold {} of samples; opening it added {} resident",
        s.project.clips.len(),
        mb(clip_kb),
        mb(opened.resident_kb.saturating_sub(start.resident_kb))
    );
    // The null device has no clock: a cycle keeps the playhead on the audio while it races.
    s.set_cycle(true, 0, LEN);
    s.play();
    std::thread::sleep(Duration::from_secs(1));
    let played = report("after playing");

    let t0 = Instant::now();
    if onfault {
        s.apply_memory_lock();
    } else {
        #[cfg(target_os = "linux")]
        {
            let ok = unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) } == 0;
            s.memory_lock = if ok { MemoryLock::Locked } else { MemoryLock::Failed { error: std::io::Error::last_os_error().to_string() } };
        }
    }
    let took = t0.elapsed();
    let locked = report(&format!("locked ({:?}, in {} ms)", s.memory_lock, took.as_millis()));
    println!("  locking made {} more resident", mb(locked.resident_kb.saturating_sub(played.resident_kb)));

    #[cfg(target_os = "linux")]
    {
        let limit = daw_engine::audio_io::memlock::memlock_limit();
        match limit {
            None => {
                check("with RLIMIT_MEMLOCK unlimited the memory is locked", s.memory_lock == MemoryLock::Locked);
                // VmLck counts the locked mappings whole, touched or not.
                check(
                    &format!("the mappings holding what is resident are locked (VmLck {}, resident {})", mb(locked.locked_kb), mb(played.resident_kb)),
                    locked.locked_kb >= played.resident_kb,
                );
                // MCL_FUTURE: what's allocated afterwards is locked too, and allocating still works.
                let more = project.join("Audio Files").join("Later.wav");
                write_take(&more, 0.5, 99);
                let t = s.add_track(TrackKind::Audio, daw_engine::model::InputSel::Mono(0), None, None).unwrap();
                check("audio imported after locking loads", s.import_audio(&more, t, 0).is_ok());
                let out = s.render(0, LEN, 0, None, &mut |_| {}).unwrap();
                check("and the project renders with it", out[0].iter().any(|x| x.abs() > 0.01));
                let after = report("after importing 30 s more");
                check("the new clip is locked too", after.locked_kb >= locked.locked_kb + 5 * 1024);
            }
            Some(limit) => {
                check(&format!("RLIMIT_MEMLOCK is {limit} bytes, so nothing is locked"), s.memory_lock == MemoryLock::LimitTooSmall { limit });
                check("VmLck stays as it was", locked.locked_kb <= played.locked_kb);
            }
        }
        // Switching it off in Settings › Audio lets go at once.
        s.set_audio_config(AudioConfig { lock_memory: false, ..s.audio_config.clone() });
        let off = report("switched off");
        check("switching it off unlocks", s.memory_lock == MemoryLock::Off && off.locked_kb <= played.locked_kb);
        s.set_audio_config(AudioConfig { lock_memory: true, ..s.audio_config.clone() });
        let on = report("switched on again");
        check("switching it on locks again", s.memory_lock != MemoryLock::Off && (limit.is_some() || on.locked_kb >= played.resident_kb));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (project, played, locked);
        check("elsewhere the switch does nothing", s.memory_lock == MemoryLock::Unsupported);
    }
    s.stop();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let onfault = !args.iter().any(|a| a == "--without-onfault");
    if args.first().map(String::as_str) == Some("--open") {
        open_and_lock(Path::new(&args[1]), onfault);
    } else {
        let scratch = PathBuf::from(args.first().expect("usage: memlock_check <scratch dir> [minutes per take]"));
        let minutes: f64 = args.get(1).and_then(|m| m.parse().ok()).unwrap_or(0.5);
        let project = build(&scratch, minutes);
        println!("wrote {} ({minutes} minutes per take)", project.display());
        // A fresh process, so the measurement is what opening the project costs.
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.arg("--open").arg(&project);
        if !onfault {
            child.arg("--without-onfault");
        }
        let status = child.status().unwrap();
        FAILED.fetch_or(!status.success(), Ordering::Relaxed);
    }
    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}
