//! The Linux implementation of the editor example (X11 embedding).
use std::time::{Duration, Instant};

use crate::poll_loop::{run_once, PollLoop};
use vst3_host::editor::set_event_loop;
use vst3_host::scan::{default_search_paths, find_bundles, scan_bundle};

pub fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "ZamComp".into());
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    set_event_loop(Box::new(PollLoop));
    // An exact name wins over a substring ("Surge XT" vs "Surge XT Effects").
    let all: Vec<_> = find_bundles(&default_search_paths()).iter().flat_map(|b| scan_bundle(b).unwrap_or_default()).collect();
    let desc = all.iter().find(|d| d.name == name).or_else(|| all.iter().find(|d| d.name.contains(&name))).expect("plugin not found").clone();
    println!("opening {}", desc.name);
    let (_p, mut c) = vst3_host::instantiate(&desc, 48000.0, 512).unwrap();
    c.open_editor(&desc.name).unwrap();
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end && c.editor_open() {
        run_once();
        c.idle();
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("editor ran for the test duration, open={}", c.editor_open());
    c.close_editor();
    println!("closed cleanly");
}
