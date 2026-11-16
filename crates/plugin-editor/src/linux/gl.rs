//! Keeps the app's OpenGL context and plug-ins' GLX contexts apart on the main thread.

use std::sync::OnceLock;

use parking_lot::Mutex;

use super::libs::loaded_libraries;

static GL_RELEASE: OnceLock<fn()> = OnceLock::new();

/// Keeps the application's OpenGL context and plug-ins' GLX contexts apart on the main thread.
/// GTK running natively on Wayland leaves its EGL context current there, and libglvnd refuses
/// `glXMakeCurrent` on a thread where another API's context is current (BadAccess), so OpenGL
/// editors (DPF and pugl ones such as Zam and Dragonfly) fail to open. `release` must make the
/// application's context not current in a way its toolkit knows about
/// (`gdk_gl_context_clear_current`). Once set, every call into an editor on the main thread
/// (`gl_isolated`) starts with no context current and ends by releasing any GLX context the
/// plug-in left current, so the toolkit's next `eglMakeCurrent` succeeds. Unset (GTK on X11),
/// nothing changes.
pub fn set_gl_isolation(release: fn()) {
    let _ = GL_RELEASE.set(release);
}

/// Whether `set_gl_isolation` was called.
pub(super) fn isolating() -> bool {
    GL_RELEASE.get().is_some()
}

thread_local! {
    static GL_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Runs `f`, a call into plug-in code, isolated as `set_gl_isolation` describes. Nested calls
/// (a plug-in's `resizeView` leading to `onSize`) are left alone: only the outermost releases.
pub fn gl_isolated<R>(f: impl FnOnce() -> R) -> R {
    let Some(release) = GL_RELEASE.get() else { return f() };
    let outer = GL_DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get() == 1
    });
    if outer {
        release();
    }
    struct Leave(bool);
    impl Drop for Leave {
        fn drop(&mut self) {
            GL_DEPTH.with(|d| d.set(d.get() - 1));
            if self.0 {
                release_glx();
            }
        }
    }
    let _leave = Leave(outer);
    f()
}

type GlxGet = unsafe extern "C" fn() -> *mut std::ffi::c_void;
type GlxMake = unsafe extern "C" fn(*mut std::ffi::c_void, std::ffi::c_ulong, *mut std::ffi::c_void) -> std::ffi::c_int;

/// `glXGetCurrentContext`, `glXGetCurrentDisplay` and `glXMakeCurrent` of each GLX library
/// plug-ins have loaded (NixOS can have several copies). Found when an editor opens; each
/// library is kept loaded so the pointers stay valid.
static GLX: Mutex<Vec<(GlxGet, GlxGet, GlxMake)>> = Mutex::new(Vec::new());

/// Finds the GLX libraries loaded so far: call after the plug-in is loaded, which brings in its GL
/// library.
pub(super) fn find_glx() {
    use std::ffi::c_void;
    let mut glx = GLX.lock();
    for lib in loaded_libraries(&[b"libGLX.so", b"libGL.so"]) {
        unsafe {
            let h = libc::dlopen(lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_NOLOAD);
            if h.is_null() {
                continue;
            }
            let get = libc::dlsym(h, c"glXGetCurrentContext".as_ptr());
            let display = libc::dlsym(h, c"glXGetCurrentDisplay".as_ptr());
            let make = libc::dlsym(h, c"glXMakeCurrent".as_ptr());
            if get.is_null() || display.is_null() || make.is_null() || glx.iter().any(|(g, _, _)| *g as *mut c_void == get) {
                libc::dlclose(h);
                continue;
            }
            // `h` stays open: the library is never unloaded under the pointers.
            glx.push((
                std::mem::transmute::<*mut c_void, GlxGet>(get),
                std::mem::transmute::<*mut c_void, GlxGet>(display),
                std::mem::transmute::<*mut c_void, GlxMake>(make),
            ));
        }
    }
}

fn release_glx() {
    for (get, display, make) in GLX.lock().iter() {
        unsafe {
            if !get().is_null() {
                make(display(), 0, std::ptr::null_mut());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static RELEASES: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn gl_isolation_releases_once_per_outermost_call() {
        assert_eq!(gl_isolated(|| 1), 1, "unset, it only runs the call");
        set_gl_isolation(|| {
            RELEASES.fetch_add(1, Ordering::Relaxed);
        });
        let r = gl_isolated(|| gl_isolated(|| gl_isolated(|| 7)) + 1);
        assert_eq!((r, RELEASES.load(Ordering::Relaxed)), (8, 1));
        gl_isolated(|| ());
        assert_eq!(RELEASES.load(Ordering::Relaxed), 2);
        // A panic inside still leaves the depth balanced.
        let _ = std::panic::catch_unwind(|| gl_isolated(|| panic!("plug-in")));
        gl_isolated(|| ());
        assert_eq!(RELEASES.load(Ordering::Relaxed), 4);
        GL_DEPTH.with(|d| assert_eq!(d.get(), 0));
    }
}
