//! The shared libraries loaded into the process, of which plug-ins may each bring their own copy
//! (NixOS).

use std::ffi::{c_int, c_void, CStr, CString};

/// The paths of the loaded shared libraries whose file name contains one of `names`.
pub(super) fn loaded_libraries(names: &[&[u8]]) -> Vec<CString> {
    struct Search<'a> {
        names: &'a [&'a [u8]],
        found: Vec<CString>,
    }
    unsafe extern "C" fn visit(info: *mut libc::dl_phdr_info, _: usize, out: *mut c_void) -> c_int {
        let search = &mut *(out as *mut Search);
        let name = (*info).dlpi_name;
        if !name.is_null() {
            let s = CStr::from_ptr(name);
            let file = s.to_bytes().rsplit(|b| *b == b'/').next().unwrap_or_default();
            if search.names.iter().any(|n| file.windows(n.len()).any(|w| w == *n)) {
                search.found.push(s.to_owned());
            }
        }
        0
    }
    let mut search = Search { names, found: Vec::new() };
    unsafe { libc::dl_iterate_phdr(Some(visit), &mut search as *mut Search as *mut c_void) };
    search.found
}

/// Xlib's default error handler calls exit(), so a single bad request from a plugin GUI would
/// take the whole host down. Install a logging handler in every libX11 instance loaded into the
/// process.
pub(super) fn install_x_error_handlers() {
    type Handler = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
    unsafe extern "C" fn on_error(_dpy: *mut c_void, ev: *mut c_void) -> c_int {
        // XErrorEvent: type, display*, resourceid, serial, error_code(u8), request_code(u8), ...
        let bytes = ev as *const u8;
        let off = std::mem::size_of::<c_int>().max(8) + 3 * std::mem::size_of::<usize>();
        let (code, req) = (*bytes.add(off), *bytes.add(off + 1));
        log::warn!("X11 error from a plug-in GUI ignored (error {code}, request {req})");
        0
    }
    unsafe {
        for lib in loaded_libraries(&[b"libX11.so"]) {
            let h = libc::dlopen(lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_NOLOAD);
            if h.is_null() {
                continue;
            }
            let f = libc::dlsym(h, c"XSetErrorHandler".as_ptr());
            if !f.is_null() {
                let set: unsafe extern "C" fn(Option<Handler>) -> Option<Handler> = std::mem::transmute(f);
                set(Some(on_error));
            }
            libc::dlclose(h);
        }
    }
}
