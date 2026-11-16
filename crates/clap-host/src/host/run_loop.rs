//! The host's `clap.timer-support` and `clap.posix-fd-support` on Linux: a plug-in's timers and file
//! descriptors run on the app's main loop (`plugin_editor::EventLoop`, GTK's in the app), where
//! many plug-ins draw their GUI and read their X connection. Without that loop (headless checks)
//! registering fails, as the API allows. While the plug-in isn't responding its timers wait and its
//! file descriptors are off the loop, so the main thread never waits on a lock its stuck thread
//! holds. Registering and unregistering are main-thread calls: elsewhere they fail.

use std::cell::Cell;
use std::ffi::CStr;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use clap_sys::ext::posix_fd_support::*;
use clap_sys::ext::timer_support::*;
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::clap_plugin;
use parking_lot::Mutex;
use plugin_editor::{event_loop, gl_isolated, FdEvents};

use super::{host, is_main, Host};

/// A timer or file descriptor the plug-in registered.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Timer(clap_id),
    /// With the flags it's watched for.
    Fd(i32, clap_posix_fd_flags),
}

impl Source {
    fn is(&self, other: &Source) -> bool {
        match (self, other) {
            (Source::Fd(a, _), Source::Fd(b, _)) => a == b,
            _ => self == other,
        }
    }
}

/// A registered source and the main loop's id for it: None for a file descriptor taken off the
/// loop while the plug-in isn't responding (`Sources::pause`).
struct Entry {
    source: Source,
    on_loop: Option<u64>,
}

/// One plug-in's registered sources.
#[derive(Default)]
pub(crate) struct Sources {
    list: Mutex<Vec<Entry>>,
    next_timer: AtomicU32,
    /// Xlib's error handlers were installed for this plug-in's first source.
    guarded: AtomicBool,
}

impl Sources {
    fn find(&self, s: Source) -> bool {
        self.list.lock().iter().any(|e| e.source.is(&s))
    }

    /// Takes `s` off the main loop and the list; false if it wasn't registered.
    fn remove(&self, s: Source) -> bool {
        let found = {
            let mut list = self.list.lock();
            list.iter().position(|e| e.source.is(&s)).map(|i| list.remove(i).on_loop)
        };
        if let (Some(Some(id)), Some(l)) = (found, event_loop()) {
            l.remove(id);
        }
        found.is_some()
    }

    /// Forgets the source the main loop dropped (a file descriptor that hung up), so the plug-in
    /// may register it again.
    fn dropped(&self, loop_id: u64) {
        self.list.lock().retain(|e| e.on_loop != Some(loop_id));
    }

    /// Takes what the plug-in left registered off the main loop: once it's destroyed, on the main
    /// thread.
    pub fn forget(&self) {
        let list = std::mem::take(&mut *self.list.lock());
        if let Some(l) = event_loop() {
            for id in list.iter().filter_map(|e| e.on_loop) {
                l.remove(id);
            }
        }
    }

    /// Takes the plug-in's file descriptors off the main loop while it isn't responding, and puts
    /// them back when it is: one that is readable would otherwise be reported on every pass of the
    /// loop, unread. On the main thread.
    pub fn pause(&self, host: &Host, on: bool) {
        let Some(l) = event_loop() else { return };
        let mut list = self.list.lock();
        for e in list.iter_mut() {
            let Source::Fd(fd, flags) = e.source else { continue };
            match (on, e.on_loop) {
                (true, Some(id)) => {
                    l.remove(id);
                    e.on_loop = None;
                }
                (false, None) => e.on_loop = unsafe { watch_fd(host, fd, flags) },
                _ => {}
            }
        }
    }

    /// Before the plug-in's first source: its GUI code may run from it.
    fn guard_x_errors(&self) {
        if !self.guarded.swap(true, Ordering::Relaxed) {
            plugin_editor::install_x_error_handlers();
        }
    }
}

/// The plug-in's own side of an extension, which it has once it's initialized.
unsafe fn extension<'a, T>(plugin: *const clap_plugin, id: &CStr) -> Option<&'a T> {
    let p = plugin.as_ref()?;
    (p.get_extension?(plugin, id.as_ptr()) as *const T).as_ref()
}

/// The host and its plug-in, for a call the API makes on the main thread; None elsewhere.
unsafe fn on_main<'a>(h: *const clap_host) -> Option<(&'a Host, *const clap_plugin)> {
    let h = host(h)?;
    let plugin = h.plugin.load(Ordering::Acquire) as *const clap_plugin;
    (is_main() && !plugin.is_null()).then_some((h, plugin))
}

pub(super) static TIMER_SUPPORT: clap_host_timer_support =
    clap_host_timer_support { register_timer: Some(register_timer), unregister_timer: Some(unregister_timer) };

unsafe extern "C" fn register_timer(h: *const clap_host, period_ms: u32, timer_id: *mut clap_id) -> bool {
    let (Some((h, plugin)), Some(l)) = (on_main(h), event_loop()) else { return false };
    if timer_id.is_null() {
        return false;
    }
    let Some(on_timer) = extension::<clap_plugin_timer_support>(plugin, CLAP_EXT_TIMER_SUPPORT).and_then(|t| t.on_timer) else { return false };
    h.sources.guard_x_errors();
    let id = h.sources.next_timer.fetch_add(1, Ordering::Relaxed);
    let host = h as *const Host;
    // The source is taken off the loop before the plug-in and its host go (`Sources::forget`).
    let added = l.add_timer(
        period_ms.max(1) as u64,
        Box::new(move || unsafe {
            if !(*host).quiet.load(Ordering::Relaxed) {
                gl_isolated(|| on_timer(plugin, id));
            }
        }),
    );
    let Some(loop_id) = added else { return false };
    h.sources.list.lock().push(Entry { source: Source::Timer(id), on_loop: Some(loop_id) });
    *timer_id = id;
    true
}

unsafe extern "C" fn unregister_timer(h: *const clap_host, timer_id: clap_id) -> bool {
    on_main(h).is_some_and(|(h, _)| h.sources.remove(Source::Timer(timer_id)))
}

pub(super) static POSIX_FD_SUPPORT: clap_host_posix_fd_support =
    clap_host_posix_fd_support { register_fd: Some(register_fd), modify_fd: Some(modify_fd), unregister_fd: Some(unregister_fd) };

/// What CLAP's flags ask to watch.
fn watch(flags: clap_posix_fd_flags) -> FdEvents {
    FdEvents { read: flags & CLAP_POSIX_FD_READ != 0, write: flags & CLAP_POSIX_FD_WRITE != 0, error: flags & CLAP_POSIX_FD_ERROR != 0 }
}

/// CLAP's flags for what a file descriptor is ready for.
fn flags(ready: FdEvents) -> clap_posix_fd_flags {
    (ready.read as u32 * CLAP_POSIX_FD_READ) | (ready.write as u32 * CLAP_POSIX_FD_WRITE) | (ready.error as u32 * CLAP_POSIX_FD_ERROR)
}

/// Puts `fd` on the main loop for the plug-in; the loop's id, or None if it can't.
unsafe fn watch_fd(h: &Host, fd: i32, fd_flags: clap_posix_fd_flags) -> Option<u64> {
    let plugin = h.plugin.load(Ordering::Acquire) as *const clap_plugin;
    let on_fd = extension::<clap_plugin_posix_fd_support>(plugin, CLAP_EXT_POSIX_FD_SUPPORT).and_then(|f| f.on_fd)?;
    let host = h as *const Host;
    // Its own id on the loop, known once it's added, for the loop dropping it.
    let me = Rc::new(Cell::new(0));
    let id = me.clone();
    let loop_id = event_loop()?.add_fd(
        fd,
        watch(fd_flags),
        Box::new(move |ready| unsafe {
            gl_isolated(|| on_fd(plugin, fd, flags(ready)));
            if ready.error {
                (*host).sources.dropped(id.get());
            }
        }),
    )?;
    me.set(loop_id);
    Some(loop_id)
}

unsafe extern "C" fn register_fd(h: *const clap_host, fd: i32, fd_flags: clap_posix_fd_flags) -> bool {
    let Some((h, _)) = on_main(h) else { return false };
    if h.sources.find(Source::Fd(fd, fd_flags)) {
        return false;
    }
    h.sources.guard_x_errors();
    // While it isn't responding, the fd waits off the loop (`Sources::pause`).
    let on_loop = if h.quiet.load(Ordering::Relaxed) {
        None
    } else {
        let Some(id) = watch_fd(h, fd, fd_flags) else { return false };
        Some(id)
    };
    h.sources.list.lock().push(Entry { source: Source::Fd(fd, fd_flags), on_loop });
    true
}

unsafe extern "C" fn modify_fd(h: *const clap_host, fd: i32, fd_flags: clap_posix_fd_flags) -> bool {
    on_main(h).is_some_and(|(hst, _)| hst.sources.remove(Source::Fd(fd, 0))) && register_fd(h, fd, fd_flags)
}

unsafe extern "C" fn unregister_fd(h: *const clap_host, fd: i32) -> bool {
    on_main(h).is_some_and(|(h, _)| h.sources.remove(Source::Fd(fd, 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fd_flags_both_ways() {
        let all = CLAP_POSIX_FD_READ | CLAP_POSIX_FD_WRITE | CLAP_POSIX_FD_ERROR;
        assert_eq!(watch(CLAP_POSIX_FD_READ), FdEvents::READ);
        assert_eq!(flags(watch(all)), all);
        assert_eq!(flags(FdEvents { read: false, write: true, error: true }), CLAP_POSIX_FD_WRITE | CLAP_POSIX_FD_ERROR);
    }
}
