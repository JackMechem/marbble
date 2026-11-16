//! Runs plug-ins' file descriptors and timers on GTK's main loop: their editors' (VST3's
//! `IRunLoop`, each editor window's X connection) and CLAP plug-ins' (`posix-fd-support`,
//! `timer-support`).
//!
//! Main thread only: sources are added with glib's `*_local` functions, which need the default
//! main context's owner (they panic elsewhere, so elsewhere nothing is added: a CLAP plug-in loaded
//! for a render may ask), and their callbacks touch the editor and the plug-in, which live there.
//! The id table is shared rather than thread-local all the same, so an editor dropped on another
//! thread still finds and removes its sources instead of leaving them to call into freed memory.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread::ThreadId;
use std::time::Duration;

use glib::{ControlFlow, IOCondition, SourceId};
use plugin_editor::FdEvents;

static SOURCES: Mutex<Option<HashMap<u64, SourceId>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

fn keep(src: SourceId) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    SOURCES.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id, src);
    id
}

/// GTK's main loop, on the thread it runs on.
pub struct GlibLoop {
    main: ThreadId,
}

impl GlibLoop {
    /// The loop of this thread, the one GTK's main loop runs on.
    pub fn on_this_thread() -> GlibLoop {
        GlibLoop { main: std::thread::current().id() }
    }

    /// Adds a source on the main thread, which must own the default main context to (GTK's loop
    /// owns it while it runs; before it does, a project opened at launch, it's taken for the call).
    /// None on another thread.
    fn adding(&self, add: impl FnOnce() -> SourceId) -> Option<u64> {
        if std::thread::current().id() != self.main {
            return None;
        }
        let context = glib::MainContext::default();
        let _owned = if context.is_owner() { None } else { Some(context.acquire().ok()?) };
        Some(keep(add()))
    }
}

impl plugin_editor::EventLoop for GlibLoop {
    fn add_fd(&self, fd: i32, watch: FdEvents, mut cb: Box<dyn FnMut(FdEvents)>) -> Option<u64> {
        let mut condition = IOCondition::empty();
        if watch.read {
            condition |= IOCondition::IN | IOCondition::PRI;
        }
        if watch.write {
            condition |= IOCondition::OUT;
        }
        if watch.error {
            condition |= IOCondition::ERR | IOCondition::HUP;
        }
        self.adding(|| {
            glib::source::unix_fd_add_local(fd, condition, move |_, ready| {
                cb(FdEvents {
                    read: ready.intersects(IOCondition::IN | IOCondition::PRI),
                    write: ready.contains(IOCondition::OUT),
                    error: ready.intersects(IOCondition::ERR | IOCondition::HUP | IOCondition::NVAL),
                });
                ControlFlow::Continue
            })
        })
    }

    fn add_timer(&self, interval_ms: u64, mut cb: Box<dyn FnMut()>) -> Option<u64> {
        self.adding(|| {
            glib::timeout_add_local(Duration::from_millis(interval_ms), move || {
                cb();
                ControlFlow::Continue
            })
        })
    }

    fn remove(&self, id: u64) {
        let src = SOURCES.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(&id));
        let Some(src) = src else { return };
        if !glib::MainContext::default().is_owner() {
            // GLib lets any thread destroy a source, so it won't be dispatched again, but a
            // callback already running on the main thread can't be stopped: editors must be
            // dropped on the main thread.
            log::error!("plug-in editor source {id} removed off the main thread");
        }
        src.remove();
    }
}
