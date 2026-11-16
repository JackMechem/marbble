//! What an editor window asks of a plug-in's GUI, whatever its format.

use std::ffi::c_void;

use crate::Frame;

/// The window a GUI is embedded in.
#[derive(Clone, Copy, Debug)]
pub enum Parent {
    /// An X11 window's id (Linux).
    X11(u32),
    /// An `NSView` (macOS).
    NsView(*mut c_void),
}

/// A plug-in's GUI, as an editor window drives it: a VST3's `IPlugView`, a CLAP plug-in's
/// `clap.gui`. Sizes are in pixels on Linux and in points on macOS. Calls come from the main thread,
/// except `set_size` when a plug-in asks for a size from another (`Frame::resize`).
pub trait View {
    /// Tells the GUI the display's scale, before its size is asked for (Linux only). False if it
    /// can't be told: it may read the display's scale itself.
    fn set_scale(&self, scale: f64) -> bool;

    /// Its size, or None if it won't say.
    fn size(&self) -> Option<(u32, u32)>;

    /// Whether the user may resize its window.
    fn can_resize(&self) -> bool;

    /// The size nearest `size` that it can take, or None if it won't say.
    fn constrain(&self, size: (u32, u32)) -> Option<(u32, u32)>;

    /// Tells it its window's size is now `size`.
    fn set_size(&self, size: (u32, u32));

    /// Puts it in `parent`, the window made for it. `frame` is how it asks for a new size later.
    fn attach(&self, parent: Parent, frame: Frame) -> Result<(), String>;

    /// Takes it out of its window, before the window goes. Called once, after `attach` succeeded.
    fn detach(&self);
}
