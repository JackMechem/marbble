//! The app's main loop (GTK's in the Tauri app), which runs the plug-ins' file descriptors and
//! timers: a VST3 editor's `IRunLoop`, a CLAP plug-in's `posix-fd-support` and `timer-support`, and
//! each editor window's own X connection.

use std::sync::OnceLock;

/// What a file descriptor is watched for, or what it is ready for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FdEvents {
    pub read: bool,
    pub write: bool,
    /// An error or a hang-up; reported whether asked for or not.
    pub error: bool,
}

impl FdEvents {
    pub const READ: FdEvents = FdEvents { read: true, write: false, error: false };
}

/// Integration point with the application's main loop, whose thread runs the callbacks. Adding a
/// source on another thread (a plug-in loaded for a render, registering a timer) gives None.
pub trait EventLoop: Send + Sync {
    /// Calls `cb` with what `fd` is ready for, whenever it is ready for something in `watch`.
    fn add_fd(&self, fd: i32, watch: FdEvents, cb: Box<dyn FnMut(FdEvents)>) -> Option<u64>;
    fn add_timer(&self, interval_ms: u64, cb: Box<dyn FnMut()>) -> Option<u64>;
    fn remove(&self, id: u64);
}

static EVENT_LOOP: OnceLock<Box<dyn EventLoop>> = OnceLock::new();

/// Sets the main loop, once, before any editor opens or plug-in loads.
pub fn set_event_loop(l: Box<dyn EventLoop>) {
    let _ = EVENT_LOOP.set(l);
}

/// The main loop, if the app set one (headless checks don't).
pub fn event_loop() -> Option<&'static dyn EventLoop> {
    EVENT_LOOP.get().map(|b| b.as_ref())
}
