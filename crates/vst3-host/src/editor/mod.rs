//! Plug-in editor windows, with one implementation per platform behind the same interface: an X11
//! window on Linux (`linux.rs`, drawn at the scale `x11_scale.rs` chooses), an NSWindow on macOS
//! (`macos.rs`), and a placeholder elsewhere (`other.rs`).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "linux")]
mod x11_scale;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod other;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use other::*;
