//! The host's `clap.timer-support` and `clap.posix-fd-support` on Linux: a plug-in's timers and file
//! descriptors run on the app's main loop (`plugin_editor::EventLoop`, GTK's in the app), where
//! many plug-ins draw their GUI and read their X connection. Without that loop (headless checks)
//! registering fails, as the API allows. Timers wait while the plug-in isn't responding; file
//! descriptors' handlers still run, as their sockets must be read.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU32, Ordering};

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
    Fd(i32),
}

/// One plug-in's registered sources, with the main loop's ids for them.
#[derive(Default)]
pub(crate) struct Sources {
    list: Mutex<Vec<(Source, u64)>>,
    next_timer: AtomicU32,
}

impl Sources {
    fn add(&self, s: Source, loop_id: u64) {
        self.list.lock().push((s, loop_id));
    }

    /// Takes `s` off the main loop; false if it wasn't on it.
    fn remove(&self, s: Source) -> bool {
        let found = {
            let mut list = self.list.lock();
            list.iter().position(|(x, _)| *x == s).map(|i| list.remove(i).1)
        };
        if let (Some(id), Some(l)) = (found, event_loop()) {
            l.remove(id);
        }
        found.is_some()
    }

    /// Takes what the plug-in left registered off the main loop: once it's destroyed, on the main
    /// thread.
    pub fn forget(&self) {
        let list = std::mem::take(&mut *self.list.lock());
        if let Some(l) = event_loop() {
            for (_, id) in list {
                l.remove(id);
            }
        }
    }
}

/// The plug-in's own side of an extension, which it has once it's initialized.
unsafe fn extension<'a, T>(plugin: *const clap_plugin, id: &CStr) -> Option<&'a T> {
    let p = plugin.as_ref()?;
    (p.get_extension?(plugin, id.as_ptr()) as *const T).as_ref()
}

/// The host, its plug-in and the main loop, for a registration made on the main thread.
unsafe fn registering<'a>(h: *const clap_host) -> Option<(&'a Host, *const clap_plugin, &'static dyn plugin_editor::EventLoop)> {
    let h = host(h)?;
    let plugin = h.plugin.load(Ordering::Acquire) as *const clap_plugin;
    (is_main() && !plugin.is_null()).then_some(())?;
    Some((h, plugin, event_loop()?))
}

pub(super) static TIMER_SUPPORT: clap_host_timer_support =
    clap_host_timer_support { register_timer: Some(register_timer), unregister_timer: Some(unregister_timer) };

unsafe extern "C" fn register_timer(h: *const clap_host, period_ms: u32, timer_id: *mut clap_id) -> bool {
    let Some((h, plugin, l)) = registering(h).filter(|_| !timer_id.is_null()) else { return false };
    let Some(on_timer) = extension::<clap_plugin_timer_support>(plugin, CLAP_EXT_TIMER_SUPPORT).and_then(|t| t.on_timer) else { return false };
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
    h.sources.add(Source::Timer(id), loop_id);
    *timer_id = id;
    true
}

unsafe extern "C" fn unregister_timer(h: *const clap_host, timer_id: clap_id) -> bool {
    host(h).is_some_and(|h| h.sources.remove(Source::Timer(timer_id)))
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

unsafe extern "C" fn register_fd(h: *const clap_host, fd: i32, fd_flags: clap_posix_fd_flags) -> bool {
    let Some((h, plugin, l)) = registering(h) else { return false };
    if h.sources.list.lock().iter().any(|(s, _)| *s == Source::Fd(fd)) {
        return false;
    }
    let Some(on_fd) = extension::<clap_plugin_posix_fd_support>(plugin, CLAP_EXT_POSIX_FD_SUPPORT).and_then(|f| f.on_fd) else { return false };
    let Some(loop_id) = l.add_fd(fd, watch(fd_flags), Box::new(move |ready| unsafe { gl_isolated(|| on_fd(plugin, fd, flags(ready))) })) else {
        return false;
    };
    h.sources.add(Source::Fd(fd), loop_id);
    true
}

unsafe extern "C" fn modify_fd(h: *const clap_host, fd: i32, fd_flags: clap_posix_fd_flags) -> bool {
    let Some((hst, ..)) = registering(h) else { return false };
    hst.sources.remove(Source::Fd(fd)) && register_fd(h, fd, fd_flags)
}

unsafe extern "C" fn unregister_fd(h: *const clap_host, fd: i32) -> bool {
    host(h).is_some_and(|h| h.sources.remove(Source::Fd(fd)))
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
