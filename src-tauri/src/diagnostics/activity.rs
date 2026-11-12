//! The activity log: what the user does and what the engine's watch sees, always in the log the
//! Diagnostics window shows (`log_ring.rs`) and, with Record Everything on (the Diagnostics window,
//! kept in `settings.json` so it is on from launch), with everything else in a file in `logs/` in
//! the config folder, a line each, flushed as it goes so a crash loses nothing:
//!
//! - every command the interface sends the engine, with its arguments ([`command`], hooked into
//!   Tauri's invoke handler in main.rs), but those the windows send on a timer ([`POLLED`]);
//! - menu items, key commands and MIDI-fired commands, and windows coming to the front or closing
//!   ([`note`]; the frontend's through `log_activity`);
//! - every log line (`logger.rs`);
//! - what the engine's counters, device, threads and transport did, from [`Session::watch`] four
//!   times a second from launch ([`start_watch`]): gaps, late buffers, overloads, stream errors,
//!   restarts, play and stop.
//!
//! A thread of its own writes the file, so nothing waits for the disk, and nothing here runs on the
//! audio thread. A run of one command (a fader drag sends one per move) is one line with a count
//! and the last arguments. A file past [`FILE_BYTES`] gives way to a new one, and the oldest go once
//! the folder holds more than [`FOLDER_BYTES`].

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use daw_engine::library::plugin_db::config_dir;
use daw_engine::session::Watch;
use daw_engine::Session;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::ipc::InvokeBody;

/// A file this big gives way to a new one.
pub const FILE_BYTES: u64 = 50 << 20;
/// The oldest files go once the folder holds more than this.
pub const FOLDER_BYTES: u64 = 200 << 20;
/// A command's arguments are cut here (a whole region's notes, say), with "…".
const ARGS_CHARS: usize = 600;
const WATCH_EVERY: Duration = Duration::from_millis(250);
/// The longest a look waits for the session, which edits hold for moments; past it, the next.
const SESSION_WAIT: Duration = Duration::from_millis(10);
/// A run of one command is written once it has been quiet this long, or something else comes, or
/// it has gone on this long (a drag held for minutes isn't left unwritten).
const RUN_QUIET: Duration = Duration::from_secs(1);
const RUN_LONGEST: Duration = Duration::from_secs(5);

/// Commands the interface sends by itself rather than for something the user did: on a timer (the
/// Diagnostics window's refresh, the lists in Settings › Audio and MIDI, the sampler's recording
/// level, this log's own), or to follow what changed (the menu bar's state, waveforms to draw).
pub const POLLED: &[&str] = &[
    "diagnostics",
    "activity_log",
    "log_activity",
    "input_level",
    "control_surfaces",
    "midi_ports",
    "midi_outputs",
    "midi_monitor",
    "sampler_record_status",
    "set_menu_state",
    "set_project_menus",
    "clip_waveform",
    "project_files",
];

/// A line on its way to the file: when, in UTC and since launch, and for a command, its name, as a
/// run of the same one folds into a line.
struct Entry {
    at: SystemTime,
    since: Duration,
    kind: &'static str,
    text: String,
    run: Option<String>,
}

struct Log {
    on: AtomicBool,
    /// Counts the times it was switched on, so the watch notes how things stand in each new file.
    generation: AtomicU64,
    tx: Mutex<Option<Sender<Entry>>>,
    /// The file being written, which the writer moves on when it fills.
    file: Mutex<Option<PathBuf>>,
}

fn state() -> &'static Log {
    static LOG: OnceLock<Log> = OnceLock::new();
    LOG.get_or_init(|| Log { on: AtomicBool::new(false), generation: AtomicU64::new(0), tx: Mutex::new(None), file: Mutex::new(None) })
}

/// Where the files go.
pub fn folder() -> PathBuf {
    config_dir().join("logs")
}

pub fn is_on() -> bool {
    state().on.load(Ordering::Relaxed)
}

/// Adds a line to the window's log, and to the file when it's on. Never logs (the logger comes
/// here), and never waits.
pub fn note(kind: &'static str, text: impl Into<String>) {
    let text = text.into();
    crate::diagnostics::logger::ring().push_activity(since_ms(), kind, text.clone());
    send(kind, text, None);
}

fn since_ms() -> u64 {
    crate::diagnostics::logger::since_start().as_millis() as u64
}

fn send(kind: &'static str, text: String, run: Option<String>) {
    if !is_on() {
        return;
    }
    if let Some(tx) = state().tx.lock().as_ref() {
        let since = crate::diagnostics::logger::since_start();
        let _ = tx.send(Entry { at: SystemTime::now(), since, kind, text, run });
    }
}

/// A command the interface sent (every one goes through here first; see main.rs).
pub fn command(name: &str, payload: &InvokeBody) {
    if POLLED.contains(&name) {
        return;
    }
    let args = match payload {
        InvokeBody::Json(serde_json::Value::Object(m)) if m.is_empty() => String::new(),
        InvokeBody::Json(v) => format!(" {}", cut(v.to_string(), ARGS_CHARS)),
        InvokeBody::Raw(b) => format!(" <{} bytes>", b.len()),
    };
    let text = format!("{name}{args}");
    crate::diagnostics::logger::ring().push_command(since_ms(), name, text.clone());
    send("command", text, Some(name.to_string()));
}

/// A log line, from `logger.rs`.
pub fn log_line(level: log::Level, target: &str, message: &str) {
    if is_on() {
        send("log", format!("{:<5} {target}: {message}", level.as_str()), None);
    }
}

/// Switches it on or off: from the Diagnostics window, and at launch from the settings. Switching
/// it on starts a new file.
pub fn set(on: bool) -> Result<(), String> {
    let log = state();
    if on == is_on() {
        return Ok(());
    }
    if !on {
        note("activity", "recording stopped");
        log.on.store(false, Ordering::Relaxed);
        // The writer drains what's queued, then ends.
        *log.tx.lock() = None;
        return Ok(());
    }
    let out = Out::open().map_err(|e| format!("Couldn't start a log file in {}: {e}", folder().display()))?;
    *log.file.lock() = Some(out.path.clone());
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new().name("activity-log".into()).spawn(move || write_loop(rx, out)).map_err(|e| e.to_string())?;
    *log.tx.lock() = Some(tx);
    log.on.store(true, Ordering::Relaxed);
    note("activity", format!("Marbble {} on {} {}, recording everything", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH));
    // The watch notes how things stand at the start of the file.
    log.generation.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Starts the watch, for the rest of the app's life: four times a second it looks at the session
/// and notes what changed, and how things stand at launch and when a recording starts.
pub fn start_watch(session: Arc<Mutex<Session>>) {
    let spawned = std::thread::Builder::new().name("activity-watch".into()).spawn(move || watch_loop(&session));
    if let Err(e) = spawned {
        log::warn!("no activity watch: {e}");
    }
}

fn watch_loop(session: &Mutex<Session>) {
    let mut before: Option<Watch> = None;
    // The recording the state was last noted for.
    let mut noted = u64::MAX;
    loop {
        if let Some(now) = session.try_lock_for(SESSION_WAIT).map(|s| s.watch()) {
            let generation = state().generation.load(Ordering::Relaxed);
            if let Some(b) = &before {
                for (kind, text) in now.changes(b) {
                    note(kind.name(), text);
                }
            }
            if before.is_none() || (generation != noted && is_on()) {
                note("device", first_look(&now));
                noted = generation;
            }
            before = Some(now);
        }
        std::thread::sleep(WATCH_EVERY);
    }
}

/// How things stand when recording starts.
fn first_look(w: &Watch) -> String {
    let a = &w.audio;
    let device = if a.running {
        format!(
            "{}: {} → {}, {} Hz, {} frames",
            a.host,
            a.input.as_deref().unwrap_or("no input"),
            a.output.as_deref().unwrap_or("no output"),
            a.sample_rate,
            a.buffer_size
        )
    } else {
        format!("not running{}", a.error.as_deref().map(|e| format!(": {e}")).unwrap_or_default())
    };
    let threads = format!("{} processing threads (setting {})", w.threads.resolved, w.threads.setting);
    format!("{device}; {threads}; {}", if w.playing { format!("playing at {}", w.at()) } else { format!("stopped at {}", w.at()) })
}

/// A run of one command, written as one line once something else comes or it goes quiet.
struct Run {
    name: String,
    first: SystemTime,
    since: Duration,
    last: SystemTime,
    text: String,
    count: u64,
}

fn write_loop(rx: Receiver<Entry>, mut out: Out) {
    let mut run: Option<Run> = None;
    loop {
        match rx.recv_timeout(RUN_QUIET) {
            Ok(e) => {
                if let (Some(name), Some(r)) = (&e.run, run.as_mut()) {
                    if *name == r.name && e.at.duration_since(r.first).unwrap_or_default() < RUN_LONGEST {
                        r.count += 1;
                        r.last = e.at;
                        r.text = e.text;
                        continue;
                    }
                }
                if let Some(r) = run.take() {
                    out.run(&r);
                }
                match e.run {
                    Some(name) => run = Some(Run { name, first: e.at, since: e.since, last: e.at, text: e.text, count: 1 }),
                    None => out.line(e.at, e.since, e.kind, &e.text),
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(r) = run.take() {
                    out.run(&r);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if let Some(r) = run.take() {
                    out.run(&r);
                }
                out.flush();
                return;
            }
        }
        out.flush();
    }
}

/// The file being written.
struct Out {
    path: PathBuf,
    file: BufWriter<File>,
    bytes: u64,
}

impl Out {
    /// A new file, named for the time (UTC), after the oldest made room for it.
    fn open() -> std::io::Result<Out> {
        let dir = folder();
        std::fs::create_dir_all(&dir)?;
        let ms = now_ms(SystemTime::now());
        let mut path = dir.join(format!("activity-{}.log", file_stamp(ms)));
        // Two in one second (switched off and on again): the second gets the milliseconds too.
        if path.exists() {
            path = dir.join(format!("activity-{}-{:03}.log", file_stamp(ms), ms % 1000));
        }
        let out = Out::create(path)?;
        prune(&dir, &out.path, FOLDER_BYTES);
        Ok(out)
    }

    fn create(path: PathBuf) -> std::io::Result<Out> {
        let file = BufWriter::new(File::create(&path)?);
        Ok(Out { path, file, bytes: 0 })
    }

    fn line(&mut self, at: SystemTime, since: Duration, kind: &str, text: &str) {
        let line = format!("{}  +{:>10.3}  {kind:<9} {text}\n", line_stamp(now_ms(at)), since.as_secs_f64());
        if let Err(e) = self.file.write_all(line.as_bytes()) {
            // Not log::: the logger writes here.
            eprintln!("activity log {}: {e}", self.path.display());
            return;
        }
        self.bytes += line.len() as u64;
        if self.bytes > FILE_BYTES {
            self.next_file();
        }
    }

    fn run(&mut self, r: &Run) {
        if r.count == 1 {
            return self.line(r.first, r.since, "command", &r.text);
        }
        let secs = r.last.duration_since(r.first).unwrap_or_default().as_secs_f64();
        self.line(r.first, r.since, "command", &format!("{} ×{} over {secs:.1} s, the last: {}", r.name, r.count, r.text));
    }

    fn flush(&mut self) {
        let _ = self.file.flush();
    }

    fn next_file(&mut self) {
        self.flush();
        match Out::open() {
            Ok(next) => {
                *self = next;
                *state().file.lock() = Some(self.path.clone());
                self.line(SystemTime::now(), crate::diagnostics::logger::since_start(), "activity", "continued from the file before, which was full");
            }
            Err(e) => eprintln!("activity log: no new file in {}: {e}", folder().display()),
        }
    }
}

/// Deletes the oldest activity files in `dir` (but `keep`) until they hold `limit` bytes at most.
fn prune(dir: &Path, keep: &Path, limit: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(PathBuf, u64)> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("activity-") && e.file_name().to_string_lossy().ends_with(".log"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.len())))
        .collect();
    // The names start with the time, so they sort oldest first.
    files.sort();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    for (path, len) in files {
        if total <= limit {
            break;
        }
        if path != keep && std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

fn now_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Year, month and day of a day counted from 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `2026-11-11 23:20:01.123Z`.
fn line_stamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (y, mo, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02}.{:03}Z", t / 3600, t / 60 % 60, t % 60, ms % 1000)
}

/// `2026-11-11T232001Z`, for a file's name.
fn file_stamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (y, mo, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02}T{:02}{:02}{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

/// `s` cut to `max` characters, with "…" after.
fn cut(s: String, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}

/// What the Diagnostics window shows of it.
#[derive(Serialize)]
pub struct ActivityStatus {
    pub on: bool,
    /// The file being written, while it's on.
    pub file: Option<String>,
    pub folder: String,
}

fn status() -> ActivityStatus {
    let file = if is_on() { state().file.lock().as_ref().map(|p| p.display().to_string()) } else { None };
    ActivityStatus { on: is_on(), file, folder: folder().display().to_string() }
}

#[tauri::command]
pub fn activity_log() -> ActivityStatus {
    status()
}

/// Record Everything, switched in the Diagnostics window: kept in the settings for the next
/// launch.
#[tauri::command]
pub fn set_activity_log(on: bool) -> Result<ActivityStatus, String> {
    set(on)?;
    let mut all = crate::settings::load();
    all.diagnostics.record_activity = on;
    crate::settings::save(&all);
    Ok(status())
}

/// A key command, a command a MIDI controller fired, or something else the interface did that sends
/// no command of its own. `kind` is one of "key", "midi" or "ui".
#[tauri::command]
pub fn log_activity(kind: String, what: String) {
    let kind = match kind.as_str() {
        "key" => "key",
        "midi" => "midi",
        _ => "ui",
    };
    note(kind, cut(what, ARGS_CHARS));
}

/// Opens the folder of log files in the file manager.
#[tauri::command]
pub fn show_activity_logs() -> Result<(), String> {
    let dir = folder();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    let r = {
        use gio::prelude::FileExt;
        gio::AppInfo::launch_default_for_uri(&gio::File::for_path(&dir).uri(), None::<&gio::AppLaunchContext>).map_err(|e| e.to_string())
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&dir).spawn().map(|_| ()).map_err(|e| e.to_string());
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(&dir).spawn().map(|_| ()).map_err(|e| e.to_string());
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let r: Result<(), String> = Err("no file manager".into());
    r.map_err(|e| format!("Couldn't open {}: {e}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_read_as_utc() {
        // 2026-11-11 23:20:01.123 UTC.
        let ms = 1_794_439_201_123;
        assert_eq!(line_stamp(ms), "2026-11-11 23:20:01.123Z");
        assert_eq!(file_stamp(ms), "2026-11-11T232001Z");
        assert_eq!(line_stamp(0), "1970-01-01 00:00:00.000Z");
        // A leap day.
        assert_eq!(file_stamp(1_709_164_800_000), "2024-02-29T000000Z");
    }

    #[test]
    fn long_arguments_are_cut() {
        assert_eq!(cut("abcdef".into(), 3), "abc…");
        assert_eq!(cut("abc".into(), 3), "abc");
        assert_eq!(cut("ééé".into(), 2), "éé…");
    }

    /// A run of one command is one line with a count and the last arguments; anything else ends it.
    #[test]
    fn a_run_of_one_command_is_one_line() {
        let path = std::env::temp_dir().join(format!("marbble-activity-run-{}.log", std::process::id()));
        let out = Out::create(path.clone()).unwrap();
        let (tx, rx) = mpsc::channel();
        let t0 = UNIX_EPOCH + Duration::from_secs(1_794_439_201);
        let entry = |ms: u64, kind: &'static str, text: &str, run: Option<&str>| Entry {
            at: t0 + Duration::from_millis(ms),
            since: Duration::from_millis(ms),
            kind,
            text: text.into(),
            run: run.map(String::from),
        };
        for (i, db) in [-3, -6, -9].into_iter().enumerate() {
            tx.send(entry(i as u64 * 100, "command", &format!("update_track {{\"volume_db\":{db}}}"), Some("update_track"))).unwrap();
        }
        tx.send(entry(300, "key", "play", None)).unwrap();
        tx.send(entry(400, "command", "toggle_play", Some("toggle_play"))).unwrap();
        // Past the longest run: a second line for the same command.
        tx.send(entry(400 + RUN_LONGEST.as_millis() as u64, "command", "toggle_play", Some("toggle_play"))).unwrap();
        drop(tx);
        write_loop(rx, out);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "{text}");
        assert!(
            lines[0].starts_with("2026-11-11 23:20:01.000Z  +     0.000  command   update_track ×3 over 0.2 s, the last: update_track {\"volume_db\":-9}"),
            "{}",
            lines[0]
        );
        assert!(lines[1].ends_with("key       play"), "{}", lines[1]);
        assert!(lines[2].ends_with("command   toggle_play") && lines[3].ends_with("command   toggle_play"), "{text}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_oldest_files_go_first_and_never_the_one_being_written() {
        let dir = std::env::temp_dir().join(format!("marbble-activity-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str, len: usize| {
            let p = dir.join(name);
            std::fs::write(&p, vec![b'x'; len]).unwrap();
            p
        };
        let old = make("activity-2026-01-01T000000Z.log", 40);
        let mid = make("activity-2026-02-01T000000Z.log", 40);
        let other = make("notes.txt", 500);
        let new = make("activity-2026-03-01T000000Z.log", 40);
        prune(&dir, &old, 80);
        assert!(old.exists(), "the one being written stays, even as the oldest");
        assert!(!mid.exists(), "the next oldest goes");
        assert!(new.exists() && other.exists(), "the newest stays, and other files aren't touched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
