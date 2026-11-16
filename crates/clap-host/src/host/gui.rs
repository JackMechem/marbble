//! The host's `clap.gui`: what the plug-in asks of its editor window. Any thread may ask, so each
//! request only sets a flag (`GuiRequests`) for the controller's `idle`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use clap_sys::ext::gui::clap_host_gui;
use clap_sys::host::clap_host;

use super::host;

/// What the plug-in asked of its editor window.
pub(crate) struct GuiRequests {
    /// `request_resize`: the size it took, width in the high half; `NO_SIZE` when none.
    resize: AtomicU64,
    /// `request_show`, `request_hide`.
    pub show: AtomicBool,
    pub hide: AtomicBool,
    /// `closed`: its GUI went (a floating window it closed, or a GUI it lost), and must be destroyed.
    pub closed: AtomicBool,
    /// `closed` said the GUI was destroyed already: it's only destroyed, not hidden first.
    pub destroyed: AtomicBool,
}

const NO_SIZE: u64 = u64::MAX;

impl Default for GuiRequests {
    fn default() -> Self {
        GuiRequests {
            resize: AtomicU64::new(NO_SIZE),
            show: AtomicBool::new(false),
            hide: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            destroyed: AtomicBool::new(false),
        }
    }
}

impl GuiRequests {
    /// The size it last asked for since the last call, if it asked.
    pub fn take_resize(&self) -> Option<(u32, u32)> {
        let v = self.resize.swap(NO_SIZE, Ordering::AcqRel);
        (v != NO_SIZE).then_some(((v >> 32) as u32, v as u32))
    }

    /// Forgets what was asked of an editor that is gone, before a new one opens.
    pub fn clear(&self) {
        self.take_resize();
        for f in [&self.show, &self.hide, &self.closed, &self.destroyed] {
            f.store(false, Ordering::Release);
        }
    }
}

pub(super) static GUI: clap_host_gui = clap_host_gui {
    resize_hints_changed: Some(resize_hints_changed),
    request_resize: Some(request_resize),
    request_show: Some(request_show),
    request_hide: Some(request_hide),
    closed: Some(closed),
};

/// The window's resizability is read when it opens.
unsafe extern "C" fn resize_hints_changed(_h: *const clap_host) {}

unsafe extern "C" fn request_resize(h: *const clap_host, width: u32, height: u32) -> bool {
    let Some(h) = host(h) else { return false };
    h.requests.gui.resize.store(((width as u64) << 32) | height as u64, Ordering::Release);
    true
}

unsafe extern "C" fn request_show(h: *const clap_host) -> bool {
    host(h).inspect(|h| h.requests.gui.show.store(true, Ordering::Release)).is_some()
}

unsafe extern "C" fn request_hide(h: *const clap_host) -> bool {
    host(h).inspect(|h| h.requests.gui.hide.store(true, Ordering::Release)).is_some()
}

unsafe extern "C" fn closed(h: *const clap_host, was_destroyed: bool) {
    if let Some(h) = host(h) {
        h.requests.gui.destroyed.fetch_or(was_destroyed, Ordering::AcqRel);
        h.requests.gui.closed.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;

    #[test]
    fn requests_wait_for_idle() {
        let h = Host::new("Test");
        unsafe {
            assert!(request_resize(h.raw(), 840, 512));
            assert!(request_resize(h.raw(), 900, 600));
            assert!(request_show(h.raw()));
        }
        let gui = &h.requests.gui;
        assert_eq!(gui.take_resize(), Some((900, 600)), "the last size asked for");
        assert_eq!(gui.take_resize(), None);
        assert!(gui.show.load(Ordering::Acquire));
        unsafe { closed(h.raw(), true) };
        assert!(gui.closed.load(Ordering::Acquire) && gui.destroyed.load(Ordering::Acquire));
        gui.clear();
        assert!(!gui.show.load(Ordering::Acquire) && !gui.closed.load(Ordering::Acquire) && !gui.destroyed.load(Ordering::Acquire));
    }
}
