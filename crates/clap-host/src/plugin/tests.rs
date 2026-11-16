//! The two halves around a fake plug-in that counts what it's asked, for what needs no real one:
//! the plug-in's requests, the values the controller keeps, and which thread tears it down.

use std::sync::atomic::{AtomicBool, AtomicU32};

use clap_sys::ext::latency::CLAP_EXT_LATENCY;
use clap_sys::ext::params::*;
use clap_sys::ext::thread_check::{clap_host_thread_check, CLAP_EXT_THREAD_CHECK};
use clap_sys::host::clap_host;
use clap_sys::process::{clap_process, clap_process_status, CLAP_PROCESS_CONTINUE};
use plugin_api::{Controller, PluginFormat, Processor, TransportInfo};

use super::*;
use crate::host::mark_main_thread;

/// What the fake plug-in was asked.
#[derive(Default)]
struct Counts {
    processed: AtomicU32,
    flushed: AtomicU32,
    /// `params.get_info` calls.
    infos: AtomicU32,
    /// Main-thread calls made on another thread.
    off_main: AtomicU32,
    latency: AtomicU32,
    destroyed: AtomicBool,
    destroyed_on_main: AtomicBool,
}

struct Fake {
    plugin: clap_plugin,
    host: *const clap_host,
    counts: Counts,
}

unsafe fn fake<'a>(p: *const clap_plugin) -> &'a Fake {
    &*((*p).plugin_data as *const Fake)
}

/// The host's own answer to "is this the main thread", as a plug-in that checks would ask it.
unsafe fn on_main(f: &Fake) -> bool {
    let ext = (*f.host).get_extension.unwrap()(f.host, CLAP_EXT_THREAD_CHECK.as_ptr()) as *const clap_host_thread_check;
    (*ext).is_main_thread.unwrap()(f.host)
}

unsafe fn main_only(p: *const clap_plugin) {
    let f = fake(p);
    if !on_main(f) {
        f.counts.off_main.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe extern "C" fn yes(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn activate(_p: *const clap_plugin, _rate: f64, _min: u32, _max: u32) -> bool {
    true
}
unsafe extern "C" fn nothing(_p: *const clap_plugin) {}
unsafe extern "C" fn destroy(p: *const clap_plugin) {
    let f = fake(p);
    f.counts.destroyed_on_main.store(on_main(f), Ordering::Relaxed);
    f.counts.destroyed.store(true, Ordering::Relaxed);
}
unsafe extern "C" fn process(p: *const clap_plugin, _process: *const clap_process) -> clap_process_status {
    fake(p).counts.processed.fetch_add(1, Ordering::Relaxed);
    CLAP_PROCESS_CONTINUE
}
unsafe extern "C" fn get_extension(_p: *const clap_plugin, id: *const std::ffi::c_char) -> *const std::ffi::c_void {
    let id = std::ffi::CStr::from_ptr(id);
    if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as _
    } else if id == CLAP_EXT_LATENCY {
        &LATENCY as *const _ as _
    } else {
        std::ptr::null()
    }
}

static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(param_count),
    get_info: Some(param_info),
    get_value: Some(param_value),
    value_to_text: None,
    text_to_value: None,
    flush: Some(flush),
};

unsafe extern "C" fn param_count(p: *const clap_plugin) -> u32 {
    main_only(p);
    1
}
unsafe extern "C" fn param_info(p: *const clap_plugin, _index: u32, info: *mut clap_param_info) -> bool {
    main_only(p);
    fake(p).counts.infos.fetch_add(1, Ordering::Relaxed);
    let i = &mut *info;
    i.id = 7;
    i.flags = CLAP_PARAM_IS_AUTOMATABLE;
    i.min_value = 0.0;
    i.max_value = 10.0;
    i.default_value = 5.0;
    true
}
unsafe extern "C" fn param_value(p: *const clap_plugin, _id: u32, out: *mut f64) -> bool {
    main_only(p);
    *out = 2.5;
    true
}
unsafe extern "C" fn flush(p: *const clap_plugin, _in: *const clap_sys::events::clap_input_events, _out: *const clap_sys::events::clap_output_events) {
    fake(p).counts.flushed.fetch_add(1, Ordering::Relaxed);
}

static LATENCY: clap_sys::ext::latency::clap_plugin_latency = clap_sys::ext::latency::clap_plugin_latency { get: Some(latency) };

unsafe extern "C" fn latency(p: *const clap_plugin) -> u32 {
    main_only(p);
    fake(p).counts.latency.load(Ordering::Relaxed)
}

/// The fake plug-in's halves, made on this thread, and what it counts. The fake itself is leaked:
/// it must outlive its halves, wherever they end.
fn halves(latency: u32) -> (ClapProcessor, ClapController, &'static Counts) {
    mark_main_thread();
    let host = Host::new("Fake");
    let f = Box::leak(Box::new(Fake {
        plugin: clap_plugin {
            desc: std::ptr::null(),
            plugin_data: std::ptr::null_mut(),
            init: Some(yes),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(nothing),
            start_processing: Some(yes),
            stop_processing: Some(nothing),
            reset: Some(nothing),
            process: Some(process),
            get_extension: Some(get_extension),
            on_main_thread: Some(nothing),
        },
        host: host.raw(),
        counts: Counts::default(),
    }));
    f.plugin.plugin_data = f as *mut Fake as *mut std::ffi::c_void;
    f.counts.latency.store(latency, Ordering::Relaxed);
    let desc = PluginDescriptor {
        id: "/fake.clap|fake".into(),
        name: "Fake".into(),
        vendor: String::new(),
        version: String::new(),
        path: "/fake.clap".into(),
        cid: "fake".into(),
        subcategories: "Fx".into(),
        is_instrument: false,
        format: PluginFormat::Clap,
    };
    let (p, c) = unsafe { instantiate::start(&desc, &f.plugin, host, None, 48_000.0, 64) }.unwrap();
    (p, c, &f.counts)
}

fn block(p: &mut ClapProcessor) {
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    p.process(&mut l, &mut r, &[], &TransportInfo { sample_rate: 48_000.0, ..Default::default() });
}

#[test]
fn a_request_to_be_run_is_answered_by_the_next_block_or_a_flush() {
    let (mut p, _c, counts) = halves(0);
    assert!(!p.wants_to_run());
    p.shared.requests().process.store(true, Ordering::Release);
    assert!(p.has_edits() && p.wants_to_run());
    block(&mut p);
    assert!(!p.has_edits() && !p.wants_to_run(), "a block answers it, so the strip may sleep again");
    assert_eq!(counts.processed.load(Ordering::Relaxed), 1);
    p.shared.requests().process.store(true, Ordering::Release);
    p.pass_edits(&TransportInfo::default());
    assert!(!p.has_edits());
    assert_eq!(counts.flushed.load(Ordering::Relaxed), 1);
    p.pass_edits(&TransportInfo::default());
    assert_eq!(counts.flushed.load(Ordering::Relaxed), 1, "nothing waits, so nothing is flushed");
}

#[test]
fn off_the_main_thread_the_controller_answers_from_what_it_read_last() {
    let (_p, mut c, counts) = halves(64);
    counts.latency.store(128, Ordering::Relaxed);
    let before = counts.infos.load(Ordering::Relaxed);
    let c2 = std::thread::spawn(move || {
        let seen = (c.latency(), c.param_value(7), c.save_state().is_none(), c.parameters().is_empty());
        c.idle();
        (c, seen)
    });
    let (moved, seen) = c2.join().unwrap();
    c = moved;
    assert_eq!(seen, (64, None, true, true), "a render's thread gets the latency read at load, and no calls");
    assert_eq!(counts.off_main.load(Ordering::Relaxed), 0, "no main-thread call was made off it");
    assert!(!c.take_latency_changed());
    c.idle();
    assert!(c.take_latency_changed() && c.latency() == 128, "idle reads it again");
    assert_eq!(counts.infos.load(Ordering::Relaxed), before);
    for _ in 0..10 {
        assert_eq!(c.param_value(7), Some(0.25));
    }
    assert_eq!(counts.infos.load(Ordering::Relaxed), before, "a value needs no parameter infos");
    c.shared.requests().changed.store(true, Ordering::Release);
    c.idle();
    assert!(c.take_params_restarted());
    assert!(counts.infos.load(Ordering::Relaxed) > before, "the ranges are read again when it says they changed");
}

#[test]
fn a_plug_in_dropped_off_the_main_thread_is_destroyed_on_it() {
    let (p, c, counts) = halves(0);
    std::thread::spawn(move || drop((p, c))).join().unwrap();
    assert!(!counts.destroyed.load(Ordering::Relaxed), "it waits for the main thread");
    destroy_deferred();
    assert!(counts.destroyed.load(Ordering::Relaxed) && counts.destroyed_on_main.load(Ordering::Relaxed));

    let (p, c, counts) = halves(0);
    drop((p, c));
    assert!(counts.destroyed.load(Ordering::Relaxed) && counts.destroyed_on_main.load(Ordering::Relaxed), "on the main thread, at once");
}
