//! A CLAP plug-in's editor: its `clap.gui`, embedded in a window of the host's
//! (`plugin_editor::Editor`) through the platform's window API, X11 on Linux and Cocoa on macOS.
//! Floating windows aren't offered: a plug-in whose GUI only floats has no editor here. Every call
//! is a main-thread one.

use std::cell::Cell;
use std::ffi::CStr;

use clap_sys::ext::gui::*;
use clap_sys::plugin::clap_plugin;
use plugin_editor::{Frame, Parent, View};

#[cfg(target_os = "linux")]
const API: &CStr = CLAP_WINDOW_API_X11;
#[cfg(target_os = "macos")]
const API: &CStr = CLAP_WINDOW_API_COCOA;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const API: &CStr = CLAP_WINDOW_API_WIN32;

/// Whether the plug-in's GUI can be embedded in a window of the host's on this platform.
pub(crate) unsafe fn embeds(plugin: *const clap_plugin, gui: *const clap_plugin_gui) -> bool {
    gui.as_ref().and_then(|g| g.is_api_supported).is_some_and(|f| f(plugin, API.as_ptr(), false))
}

/// A plug-in's GUI, created for embedding and destroyed when it leaves its window (or, if it never
/// got there, when it's dropped). The plug-in must outlive it.
pub(crate) struct ClapGui {
    plugin: *const clap_plugin,
    gui: &'static clap_plugin_gui,
    name: String,
    destroyed: Cell<bool>,
}

impl ClapGui {
    /// Creates the plug-in's GUI (`gui.create`), which `embeds` said it can make here.
    pub unsafe fn create(plugin: *const clap_plugin, gui: *const clap_plugin_gui, name: &str) -> Result<ClapGui, String> {
        let gui = gui.as_ref().ok_or_else(|| format!("{name} has no window of its own"))?;
        if !gui.create.is_some_and(|f| f(plugin, API.as_ptr(), false)) {
            return Err(format!("{name} couldn't make its window"));
        }
        Ok(ClapGui { plugin, gui, name: name.to_string(), destroyed: Cell::new(false) })
    }

    fn destroy(&self) {
        if !self.destroyed.replace(true) {
            if let Some(f) = self.gui.destroy {
                unsafe { f(self.plugin) };
            }
        }
    }
}

impl Drop for ClapGui {
    fn drop(&mut self) {
        self.destroy();
    }
}

impl View for ClapGui {
    fn set_scale(&self, scale: f64) -> bool {
        self.gui.set_scale.is_some_and(|f| unsafe { f(self.plugin, scale) })
    }

    fn size(&self) -> Option<(u32, u32)> {
        let (mut w, mut h) = (0, 0);
        self.gui.get_size.is_some_and(|f| unsafe { f(self.plugin, &mut w, &mut h) }).then_some((w, h))
    }

    fn can_resize(&self) -> bool {
        self.gui.can_resize.is_some_and(|f| unsafe { f(self.plugin) })
    }

    fn constrain(&self, (mut w, mut h): (u32, u32)) -> Option<(u32, u32)> {
        self.gui.adjust_size.is_some_and(|f| unsafe { f(self.plugin, &mut w, &mut h) }).then_some((w, h))
    }

    fn set_size(&self, (w, h): (u32, u32)) {
        if let Some(f) = self.gui.set_size {
            unsafe { f(self.plugin, w, h) };
        }
    }

    /// `set_parent`, then `show`. A GUI that only now takes the scale it was told (DPF's) says its
    /// new size when asked again, and the window takes it. Its later requests to resize come
    /// through the host (`GuiRequests`), so the frame isn't kept.
    fn attach(&self, parent: Parent, frame: Frame) -> Result<(), String> {
        let specific = match parent {
            Parent::X11(id) => clap_window_handle { x11: id as clap_xwnd },
            Parent::NsView(view) => clap_window_handle { cocoa: view },
        };
        let window = clap_window { api: API.as_ptr(), specific };
        if !self.gui.set_parent.is_some_and(|f| unsafe { f(self.plugin, &window) }) {
            return Err(format!("{} refused to attach its window", self.name));
        }
        if let Some(f) = self.gui.show {
            unsafe { f(self.plugin) };
        }
        if let Some(size) = self.size() {
            frame.fit(size);
        }
        Ok(())
    }

    fn detach(&self) {
        if let Some(f) = self.gui.hide {
            unsafe { f(self.plugin) };
        }
        self.destroy();
    }
}
