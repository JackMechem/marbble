//! What opening a plug-in's editor costs the thread that plays it (issue #36). Plays `instances` of
//! the plug-in (each given the state in `state file`, when there is one) on a thread of its own at
//! a device's pace, 512 frames at 48 kHz, opens the first instance's editor after two seconds,
//! closes it and opens it again, and prints each phase's blocks: CPU time per block, the slowest,
//! and the pages the plug-ins faulted in, which is memory first touched on that thread.
//!
//! Needs an X display: run it on a private one (`Xvfb :91 &`, `DISPLAY=:91`), never your own
//! session's. `cpu` keeps the playing thread to that logical CPU, to compare a performance core
//! with an efficiency one. Linux only.
//!
//! usage: editor_load <plug-in name> [instances (1)] [state file, or - for none] [cpu]

#[cfg(target_os = "linux")]
#[path = "editor/poll_loop.rs"]
mod poll_loop;

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("editor_load measures on Linux only");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::poll_loop::{run_once, PollLoop};
    use vst3_host::scan::{default_search_paths, find_bundles, scan_bundle};
    use vst3_host::{PluginController, PluginState, TransportInfo};

    const RATE: f64 = 48_000.0;
    const BLOCK: usize = 512;
    const PHASES: [&str; 5] = ["before", "first open, first second", "open", "second open, first second", "open"];

    /// One block: its thread's CPU time and the pages it faulted in.
    #[derive(Clone, Copy)]
    struct Block {
        phase: usize,
        cpu: Duration,
        faults: i64,
    }

    fn thread_cpu() -> Duration {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: a plain query about this thread.
        unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
        Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
    }

    fn thread_faults() -> i64 {
        // SAFETY: a plain query about this thread.
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut u) };
        u.ru_minflt + u.ru_majflt
    }

    pub fn main() {
        let mut args = std::env::args().skip(1);
        let name = args.next().expect("usage: editor_load <plug-in name> [instances] [state file]");
        let count: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
        let state = args.next().filter(|f| f != "-").map(|f| PluginState { component: std::fs::read(&f).expect("state file"), controller: Vec::new() });
        let cpu: Option<usize> = args.next().and_then(|s| s.parse().ok());
        vst3_host::editor::set_event_loop(Box::new(PollLoop));
        let all: Vec<_> = find_bundles(&default_search_paths()).iter().flat_map(|b| scan_bundle(b).unwrap_or_default()).collect();
        let desc = all.iter().find(|d| d.name == name).or_else(|| all.iter().find(|d| d.name.contains(&name))).expect("plug-in not found").clone();
        println!(
            "{} x{count}{}, played on CPU {}",
            desc.name,
            if state.is_some() { ", with the state given" } else { "" },
            cpu.map_or("any".into(), |c| c.to_string())
        );
        let (mut processors, mut controllers): (Vec<_>, Vec<PluginController>) =
            (0..count).map(|_| vst3_host::instantiate(&desc, RATE, BLOCK).unwrap()).unzip();
        if let Some(st) = &state {
            controllers.iter().for_each(|c| c.load_state(st));
        }
        let phase = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let player = {
            let (phase, stop) = (phase.clone(), stop.clone());
            std::thread::spawn(move || {
                if let Some(c) = cpu {
                    // SAFETY: a plain value struct, and a call about this thread only.
                    unsafe {
                        let mut set: libc::cpu_set_t = std::mem::zeroed();
                        libc::CPU_SET(c, &mut set);
                        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
                    }
                }
                let t = TransportInfo { sample_rate: RATE, tempo: 120.0, sig_num: 4, sig_den: 4, playing: true, ..Default::default() };
                let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
                let period = Duration::from_secs_f64(BLOCK as f64 / RATE);
                let mut blocks = Vec::with_capacity(1 << 16);
                let mut next = Instant::now();
                let mut n = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    let (cpu, faults) = (thread_cpu(), thread_faults());
                    for p in processors.iter_mut() {
                        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                            *a = ((n * BLOCK + i) as f32 * 0.05).sin() * 0.3;
                            *b = *a;
                        }
                        p.process(&mut l, &mut r, &[], &t);
                    }
                    blocks.push(Block { phase: phase.load(Ordering::Relaxed), cpu: thread_cpu() - cpu, faults: thread_faults() - faults });
                    n += 1;
                    next += period;
                    match next.checked_duration_since(Instant::now()) {
                        Some(wait) => std::thread::sleep(wait),
                        None => next = Instant::now(),
                    }
                }
                blocks
            })
        };
        let first = &mut controllers[0];
        let run = |c: &mut PluginController, secs: f64| {
            let end = Instant::now() + Duration::from_secs_f64(secs);
            while Instant::now() < end {
                run_once();
                c.idle();
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        run(first, 2.0);
        for open in [1, 3] {
            phase.store(open, Ordering::Relaxed);
            let at = Instant::now();
            first.open_editor(&desc.name).unwrap();
            println!("open_editor took {:.1} ms", at.elapsed().as_secs_f64() * 1e3);
            run(first, 1.0);
            phase.store(open + 1, Ordering::Relaxed);
            run(first, 2.0);
            first.close_editor();
        }
        stop.store(true, Ordering::Relaxed);
        let blocks = player.join().unwrap();
        println!("{:<28} {:>7} {:>10} {:>10} {:>12}", "phase", "blocks", "mean ms", "max ms", "pages/block");
        for (i, name) in PHASES.iter().enumerate() {
            let b: Vec<_> = blocks.iter().filter(|b| b.phase == i).collect();
            let ms = |d: Duration| d.as_secs_f64() * 1e3;
            let mean = b.iter().map(|b| ms(b.cpu)).sum::<f64>() / b.len().max(1) as f64;
            let max = b.iter().map(|b| ms(b.cpu)).fold(0.0, f64::max);
            let faults = b.iter().map(|b| b.faults).sum::<i64>() as f64 / b.len().max(1) as f64;
            println!("{name:<28} {:>7} {mean:>10.2} {max:>10.2} {faults:>12.1}", b.len());
        }
        println!("(a block lasts {:.1} ms)", BLOCK as f64 / RATE * 1e3);
    }
}
