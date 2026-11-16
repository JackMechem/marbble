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

fn sources() -> std::sync::MutexGuard<'static, Option<HashMap<u64, SourceId>>> {
    SOURCES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Destroys a source, on the main thread.
fn remove_now(id: u64) {
    let src = sources().as_mut().and_then(|m| m.remove(&id));
    if let Some(src) = src {
        src.remove();
    }
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

    fn on_main(&self) -> bool {
        std::thread::current().id() == self.main
    }

    /// Adds a source made by `add`, given its id, on the main thread, which must own the default
    /// main context to (GTK's loop owns it while it runs; before it does, a project opened at
    /// launch, it's taken for the call). None on another thread.
    fn adding(&self, add: impl FnOnce(u64) -> SourceId) -> Option<u64> {
        if !self.on_main() {
            return None;
        }
        let context = glib::MainContext::default();
        let _owned = if context.is_owner() { None } else { Some(context.acquire().ok()?) };
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let src = add(id);
        sources().get_or_insert_with(HashMap::new).insert(id, src);
        Some(id)
    }
}

impl plugin_editor::EventLoop for GlibLoop {
    /// A file descriptor that hangs up, fails or is closed would be reported again on every pass,
    /// keeping the loop busy: it's passed on once (`error`), then dropped from the loop.
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
        self.adding(|id| {
            glib::source::unix_fd_add_local(fd, condition, move |_, ready| {
                let ended = ready.intersects(IOCondition::ERR | IOCondition::HUP | IOCondition::NVAL);
                cb(FdEvents { read: ready.intersects(IOCondition::IN | IOCondition::PRI), write: ready.contains(IOCondition::OUT), error: ended });
                if !ended {
                    return ControlFlow::Continue;
                }
                // Glib destroys it on Break; a later `remove` finds nothing left to remove.
                sources().as_mut().and_then(|m| m.remove(&id));
                ControlFlow::Break
            })
        })
    }

    fn add_timer(&self, interval_ms: u64, mut cb: Box<dyn FnMut()>) -> Option<u64> {
        self.adding(|_| {
            glib::timeout_add_local(Duration::from_millis(interval_ms), move || {
                cb();
                ControlFlow::Continue
            })
        })
    }

    /// Off the main thread the removal is handed to it: destroying a source drops its callback,
    /// which belongs to the main thread (glib panics on another). The callback may run once more
    /// meanwhile.
    fn remove(&self, id: u64) {
        if self.on_main() {
            remove_now(id);
        } else {
            glib::MainContext::default().invoke(move || remove_now(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::rc::Rc;

    use plugin_editor::EventLoop;

    use super::*;

    #[test]
    fn a_hung_up_fd_is_reported_once_and_dropped() {
        let l = GlibLoop::on_this_thread();
        let context = glib::MainContext::default();
        let _owned = context.acquire().unwrap();
        let (reader, mut writer) = std::io::pipe().unwrap();
        let seen = Rc::new(Cell::new((0, false)));
        let s = seen.clone();
        let id = l.add_fd(reader.as_raw_fd(), FdEvents::READ, Box::new(move |r| s.set((s.get().0 + 1, s.get().1 | r.error)))).unwrap();
        writer.write_all(b"x").unwrap();
        context.iteration(false);
        assert_eq!(seen.get(), (1, false), "readable");
        drop(writer);
        for _ in 0..10 {
            context.iteration(false);
        }
        assert_eq!(seen.get(), (2, true), "hung up: told once, not again");
        assert!(sources().as_ref().is_none_or(|m| !m.contains_key(&id)));
        l.remove(id);
    }
}
