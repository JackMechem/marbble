//! Plug-in editor windows, whatever the plug-in's format: a window of the host's that the plug-in's
//! own GUI is embedded in, and everything around it (the display's scale, resizes both ways, a
//! plug-in that mustn't be called for now, keys the plug-in doesn't use). Each format's host crate
//! gives its GUI as a [`View`]: a VST3 `IPlugView` in `vst3-host`, a CLAP `clap.gui` in
//! `clap-host`.
//!
//! One implementation per platform behind the same interface: an X11 window on Linux (`linux.rs`,
//! drawn at the scale `linux/x11_scale.rs` chooses, following changes that `linux/scale_watch.rs`
//! hears of, with the plug-ins' file descriptors and timers on the app's main loop through
//! [`EventLoop`]), an NSWindow on macOS (`macos.rs`), and a placeholder elsewhere (`other.rs`).
//! Editors are main-thread objects, as are the calls into views.

pub mod keys;
mod stranded;
mod view;

pub use stranded::{close_stranded, strand};
pub use view::{Parent, View};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod other;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use other::*;

/// Runs `f`, a call into plug-in code on the main thread. On Linux it keeps the app's OpenGL
/// context and the plug-in's apart (`set_gl_isolation`); elsewhere it only runs `f`.
#[cfg(not(target_os = "linux"))]
pub fn gl_isolated<R>(f: impl FnOnce() -> R) -> R {
    f()
}
