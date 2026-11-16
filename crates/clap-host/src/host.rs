//! The host's side of the API: one `clap_host` per instance, its extensions, and which thread is
//! which. The plug-in may call the thread-safe requests from any thread, the audio thread too, so
//! they only set flags (`Requests`) for the controller's `idle`; those about its editor window are
//! in `gui.rs`. Its timers and file descriptors go on the app's main loop on Linux (`run_loop.rs`).

mod gui;
#[cfg(target_os = "linux")]
mod run_loop;

pub(crate) use gui::GuiRequests;

use std::cell::Cell;
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use clap_sys::ext::audio_ports::{clap_host_audio_ports, CLAP_EXT_AUDIO_PORTS};
use clap_sys::ext::gui::CLAP_EXT_GUI;
use clap_sys::ext::latency::{clap_host_latency, CLAP_EXT_LATENCY};
use clap_sys::ext::log::*;
use clap_sys::ext::note_ports::{clap_host_note_ports, clap_note_dialect, CLAP_EXT_NOTE_PORTS, CLAP_NOTE_DIALECT_CLAP, CLAP_NOTE_DIALECT_MIDI};
use clap_sys::ext::params::{
    clap_host_params, clap_param_clear_flags, clap_param_rescan_flags, CLAP_EXT_PARAMS, CLAP_PARAM_RESCAN_ALL, CLAP_PARAM_RESCAN_INFO, CLAP_PARAM_RESCAN_VALUES,
};
use clap_sys::ext::state::{clap_host_state, CLAP_EXT_STATE};
use clap_sys::ext::tail::{clap_host_tail, CLAP_EXT_TAIL};
use clap_sys::ext::thread_check::{clap_host_thread_check, CLAP_EXT_THREAD_CHECK};
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::clap_plugin;
use clap_sys::version::CLAP_VERSION;

thread_local! {
    /// This thread instantiated plug-ins: the main thread.
    static MAIN: Cell<bool> = const { Cell::new(false) };
    /// This thread is in a processor call (see `AudioRole`).
    static AUDIO: Cell<bool> = const { Cell::new(false) };
}

/// Marks the calling thread as the main thread, which every controller call comes from.
pub(crate) fn mark_main_thread() {
    MAIN.with(|m| m.set(true));
}

/// While held, the thread counts as the audio thread: around every call the processor makes into
/// the plug-in, wherever it runs (the device's callback, a pool worker, a render, or the main
/// thread stopping a processor it is about to drop). Allocates nothing.
pub(crate) struct AudioRole(bool);

impl AudioRole {
    pub fn enter() -> AudioRole {
        AudioRole(AUDIO.with(|a| a.replace(true)))
    }
}

impl Drop for AudioRole {
    fn drop(&mut self) {
        AUDIO.with(|a| a.set(self.0));
    }
}

fn in_audio() -> bool {
    AUDIO.with(|a| a.get())
}

/// Whether this is the main thread, outside a processor call: where the plug-in's main-thread
/// calls may be made.
pub(crate) fn is_main() -> bool {
    !in_audio() && MAIN.with(|m| m.get())
}

/// What the plug-in asked of the host, for the controller's `idle` to answer.
#[derive(Default)]
pub(crate) struct Requests {
    /// `request_callback`: call `on_main_thread`.
    pub callback: AtomicBool,
    /// `request_restart`: deactivate and activate again.
    pub restart: AtomicBool,
    /// `request_process` or `params.request_flush`: run it, or flush its parameters.
    pub process: AtomicBool,
    /// `params.rescan` with values or infos, or `state.mark_dirty`: its parameters or state changed
    /// on their own.
    pub changed: AtomicBool,
    /// `latency.changed`.
    pub latency: AtomicBool,
    /// What it asked of its editor window.
    pub gui: GuiRequests,
}

/// One instance's `clap_host`. Boxed, so the address the plug-in keeps stays valid; `host_data`
/// points at it.
pub(crate) struct Host {
    raw: clap_host,
    /// The plug-in's name, for its log messages.
    name: String,
    pub requests: Requests,
    /// The plug-in, once created (`set_plugin`), for its timers and file descriptors.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    plugin: AtomicPtr<clap_plugin>,
    /// The plug-in isn't responding: its timers wait.
    pub quiet: AtomicBool,
    /// Its timers and file descriptors on the main loop.
    #[cfg(target_os = "linux")]
    pub sources: run_loop::Sources,
}

unsafe impl Send for Host {}
unsafe impl Sync for Host {}

const VERSION: &CStr = match CStr::from_bytes_with_nul(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes()) {
    Ok(v) => v,
    Err(_) => c"",
};

impl Host {
    pub fn new(name: &str) -> Box<Host> {
        let mut h = Box::new(Host {
            raw: clap_host {
                clap_version: CLAP_VERSION,
                host_data: std::ptr::null_mut(),
                name: c"Marbble".as_ptr(),
                vendor: c"Marbble".as_ptr(),
                url: c"https://github.com/JackMechem/marbble".as_ptr(),
                version: VERSION.as_ptr(),
                get_extension: Some(get_extension),
                request_restart: Some(request_restart),
                request_process: Some(request_process),
                request_callback: Some(request_callback),
            },
            name: name.to_string(),
            requests: Requests::default(),
            plugin: AtomicPtr::new(std::ptr::null_mut()),
            quiet: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            sources: Default::default(),
        });
        h.raw.host_data = &*h as *const Host as *mut c_void;
        h
    }

    pub fn raw(&self) -> *const clap_host {
        &self.raw
    }

    /// The plug-in made with this host, before it's initialized.
    pub fn set_plugin(&self, plugin: *const clap_plugin) {
        self.plugin.store(plugin as *mut clap_plugin, Ordering::Release);
    }

    /// Takes the timers and file descriptors the plug-in left registered off the main loop, once
    /// it's destroyed.
    pub fn forget_sources(&self) {
        #[cfg(target_os = "linux")]
        self.sources.forget();
    }
}

unsafe fn host<'a>(h: *const clap_host) -> Option<&'a Host> {
    h.as_ref().and_then(|h| (h.host_data as *const Host).as_ref())
}

unsafe fn flag(h: *const clap_host, pick: impl Fn(&Requests) -> &AtomicBool) {
    if let Some(h) = host(h) {
        pick(&h.requests).store(true, Ordering::Release);
    }
}

unsafe extern "C" fn request_restart(h: *const clap_host) {
    flag(h, |r| &r.restart);
}

unsafe extern "C" fn request_process(h: *const clap_host) {
    flag(h, |r| &r.process);
}

unsafe extern "C" fn request_callback(h: *const clap_host) {
    flag(h, |r| &r.callback);
}

unsafe extern "C" fn get_extension(_h: *const clap_host, id: *const c_char) -> *const c_void {
    if id.is_null() {
        return std::ptr::null();
    }
    let id = CStr::from_ptr(id);
    let ext: *const c_void = if id == CLAP_EXT_THREAD_CHECK {
        &THREAD_CHECK as *const _ as _
    } else if id == CLAP_EXT_LOG {
        &LOG as *const _ as _
    } else if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as _
    } else if id == CLAP_EXT_STATE {
        &STATE as *const _ as _
    } else if id == CLAP_EXT_LATENCY {
        &LATENCY as *const _ as _
    } else if id == CLAP_EXT_TAIL {
        &TAIL as *const _ as _
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &AUDIO_PORTS as *const _ as _
    } else if id == CLAP_EXT_NOTE_PORTS {
        &NOTE_PORTS as *const _ as _
    } else if id == CLAP_EXT_GUI {
        &gui::GUI as *const _ as _
    } else {
        run_loop_extension(id)
    };
    ext
}

/// `timer-support` and `posix-fd-support`, which only Linux has the main loop for.
#[cfg(target_os = "linux")]
fn run_loop_extension(id: &CStr) -> *const c_void {
    use clap_sys::ext::posix_fd_support::CLAP_EXT_POSIX_FD_SUPPORT;
    use clap_sys::ext::timer_support::CLAP_EXT_TIMER_SUPPORT;
    if id == CLAP_EXT_TIMER_SUPPORT {
        &run_loop::TIMER_SUPPORT as *const _ as _
    } else if id == CLAP_EXT_POSIX_FD_SUPPORT {
        &run_loop::POSIX_FD_SUPPORT as *const _ as _
    } else {
        std::ptr::null()
    }
}

#[cfg(not(target_os = "linux"))]
fn run_loop_extension(_id: &CStr) -> *const c_void {
    std::ptr::null()
}

static THREAD_CHECK: clap_host_thread_check = clap_host_thread_check { is_main_thread: Some(is_main_thread), is_audio_thread: Some(is_audio_thread) };

unsafe extern "C" fn is_main_thread(_h: *const clap_host) -> bool {
    is_main()
}

unsafe extern "C" fn is_audio_thread(_h: *const clap_host) -> bool {
    in_audio()
}

static LOG: clap_host_log = clap_host_log { log: Some(log) };

/// Passes the plug-in's messages to the app's log, except from the audio thread, where logging
/// would allocate and lock: those are dropped.
unsafe extern "C" fn log(h: *const clap_host, severity: clap_log_severity, msg: *const c_char) {
    if in_audio() || msg.is_null() {
        return;
    }
    let name = host(h).map_or("CLAP plug-in", |h| h.name.as_str());
    let msg = CStr::from_ptr(msg).to_string_lossy();
    match severity {
        CLAP_LOG_DEBUG => log::debug!("{name}: {msg}"),
        CLAP_LOG_INFO => log::info!("{name}: {msg}"),
        CLAP_LOG_WARNING => log::warn!("{name}: {msg}"),
        CLAP_LOG_HOST_MISBEHAVING => log::warn!("{name} says the host misbehaved: {msg}"),
        CLAP_LOG_PLUGIN_MISBEHAVING => log::warn!("{name} misbehaved: {msg}"),
        _ => log::error!("{name}: {msg}"),
    }
}

static PARAMS: clap_host_params = clap_host_params { rescan: Some(params_rescan), clear: Some(params_clear), request_flush: Some(request_process) };

unsafe extern "C" fn params_rescan(h: *const clap_host, flags: clap_param_rescan_flags) {
    if flags & (CLAP_PARAM_RESCAN_VALUES | CLAP_PARAM_RESCAN_INFO | CLAP_PARAM_RESCAN_ALL) != 0 {
        flag(h, |r| &r.changed);
    }
}

unsafe extern "C" fn params_clear(_h: *const clap_host, _id: clap_id, _flags: clap_param_clear_flags) {}

static STATE: clap_host_state = clap_host_state { mark_dirty: Some(mark_dirty) };

unsafe extern "C" fn mark_dirty(h: *const clap_host) {
    flag(h, |r| &r.changed);
}

static LATENCY: clap_host_latency = clap_host_latency { changed: Some(latency_changed) };

unsafe extern "C" fn latency_changed(h: *const clap_host) {
    flag(h, |r| &r.latency);
}

static TAIL: clap_host_tail = clap_host_tail { changed: Some(nothing) };

unsafe extern "C" fn nothing(_h: *const clap_host) {}

static AUDIO_PORTS: clap_host_audio_ports = clap_host_audio_ports { is_rescan_flag_supported: Some(no_rescan_flags), rescan: Some(ignore_rescan) };

unsafe extern "C" fn no_rescan_flags(_h: *const clap_host, _flag: u32) -> bool {
    false
}

unsafe extern "C" fn ignore_rescan(_h: *const clap_host, _flags: u32) {}

static NOTE_PORTS: clap_host_note_ports = clap_host_note_ports { supported_dialects: Some(note_dialects), rescan: Some(ignore_rescan) };

unsafe extern "C" fn note_dialects(_h: *const clap_host) -> clap_note_dialect {
    CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_is_the_audio_thread_only_inside_the_processor() {
        mark_main_thread();
        let h = Host::new("Test");
        unsafe {
            assert!(is_main_thread(h.raw()) && !is_audio_thread(h.raw()));
            {
                let _a = AudioRole::enter();
                let _b = AudioRole::enter();
                drop(_b);
                assert!(is_audio_thread(h.raw()) && !is_main_thread(h.raw()), "nested roles keep the outer one");
            }
            assert!(is_main_thread(h.raw()) && !is_audio_thread(h.raw()));
            assert!(std::thread::spawn(|| !is_main_thread(std::ptr::null())).join().unwrap());
        }
    }

    #[test]
    fn requests_set_flags_and_extensions_are_found() {
        let h = Host::new("Test");
        unsafe {
            let raw = &*h.raw();
            raw.request_callback.unwrap()(h.raw());
            assert!(h.requests.callback.load(Ordering::Acquire));
            assert!(!h.requests.restart.load(Ordering::Acquire));
            let params = get_extension(h.raw(), CLAP_EXT_PARAMS.as_ptr()) as *const clap_host_params;
            (*params).rescan.unwrap()(h.raw(), CLAP_PARAM_RESCAN_VALUES);
            assert!(h.requests.changed.load(Ordering::Acquire));
            assert!(get_extension(h.raw(), c"clap.unknown".as_ptr()).is_null());
        }
    }
}
