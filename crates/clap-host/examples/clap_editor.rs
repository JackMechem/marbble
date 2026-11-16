//! Opens one CLAP plug-in's editor with a tiny run loop of its own (no GTK or Tauri), for debugging
//! editor embedding, as `vst3-host`'s `editor` example does for VST3s:
//! `cargo run -p clap-host --example clap_editor -- <name> [seconds] [reopen] [quiet]`.
//!
//! `reopen` closes the window halfway through and opens it again. `quiet` treats the plug-in as
//! stuck for the second quarter of the run, as the app does when a thread is left in it. At the
//! end it says how many timers and file descriptors were on the loop and how often each kind
//! fired (the file descriptors include the window's own X connection). Linux only, on a display of
//! its own (`Xvfb :91 &`, `DISPLAY=:91`), never your session's.

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the CLAP editor example runs on Linux only");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use clap_host::scan::{default_search_paths, find_files, scan_file};
    use plugin_api::Controller;
    use plugin_editor::poll_loop::{run_once, PollLoop};
    use plugin_editor::{set_event_loop, EventLoop, FdEvents};

    static TIMERS: AtomicU64 = AtomicU64::new(0);
    static TIMER_FIRES: AtomicU64 = AtomicU64::new(0);
    static FDS: AtomicU64 = AtomicU64::new(0);
    static FD_FIRES: AtomicU64 = AtomicU64::new(0);

    /// The poll loop, counting what goes on it and what fires.
    struct Counting;

    impl EventLoop for Counting {
        fn add_fd(&self, fd: i32, watch: FdEvents, mut cb: Box<dyn FnMut(FdEvents)>) -> Option<u64> {
            FDS.fetch_add(1, Ordering::Relaxed);
            PollLoop.add_fd(
                fd,
                watch,
                Box::new(move |ready| {
                    FD_FIRES.fetch_add(1, Ordering::Relaxed);
                    cb(ready)
                }),
            )
        }

        fn add_timer(&self, ms: u64, mut cb: Box<dyn FnMut()>) -> Option<u64> {
            TIMERS.fetch_add(1, Ordering::Relaxed);
            PollLoop.add_timer(
                ms,
                Box::new(move || {
                    TIMER_FIRES.fetch_add(1, Ordering::Relaxed);
                    cb()
                }),
            )
        }

        fn remove(&self, id: u64) {
            PollLoop.remove(id)
        }
    }

    /// Runs the loop and the controller's idle for `secs`, or until the window is closed.
    fn run(c: &mut dyn Controller, secs: f64) {
        let end = Instant::now() + Duration::from_secs_f64(secs);
        while Instant::now() < end && c.editor_open() {
            run_once();
            c.idle();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn main() {
        let mut args = std::env::args().skip(1);
        let name = args.next().unwrap_or_else(|| "ZamComp".into());
        let secs: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4.0);
        let steps: Vec<String> = args.collect();
        let (reopen, quiet) = (steps.iter().any(|s| s == "reopen"), steps.iter().any(|s| s == "quiet"));
        set_event_loop(Box::new(Counting));
        // An exact name wins over a substring.
        let all: Vec<_> = find_files(&default_search_paths()).iter().flat_map(|f| scan_file(f).unwrap_or_default()).collect();
        let desc = all.iter().find(|d| d.name == name).or_else(|| all.iter().find(|d| d.name.contains(&name))).expect("plugin not found").clone();
        println!("opening {} ({})", desc.name, desc.id);
        let (_p, mut c) = clap_host::instantiate(&desc, 48000.0, 512).unwrap();
        if !c.has_editor() {
            println!("{} has no window that embeds", desc.name);
            return;
        }
        c.open_editor(&desc.name).unwrap();
        run(&mut c, secs / 4.0);
        if quiet {
            c.set_unresponsive(true);
            run(&mut c, secs / 4.0);
            println!("quiet for {:.1} s", secs / 4.0);
            c.set_unresponsive(false);
        } else {
            run(&mut c, secs / 4.0);
        }
        if reopen {
            c.close_editor();
            println!("closed; open={}", c.editor_open());
            c.open_editor(&desc.name).unwrap();
            println!("opened again");
        }
        run(&mut c, secs / 2.0);
        println!("editor ran for the test duration, open={}", c.editor_open());
        c.close_editor();
        println!(
            "timers: {} registered, {} fires; file descriptors: {} registered, {} fires",
            TIMERS.load(Ordering::Relaxed),
            TIMER_FIRES.load(Ordering::Relaxed),
            FDS.load(Ordering::Relaxed),
            FD_FIRES.load(Ordering::Relaxed)
        );
        drop(c);
        println!("closed cleanly");
    }
}
