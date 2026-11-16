//! The two halves around a fake plug-in that counts what it's asked, for what needs no real one:
//! the plug-in's requests, the values the controller keeps, which thread tears it down, restarts,
//! the side-chain input and extra output, its render mode, tail and thread-pool tasks. Its
//! `process` puts half its main input plus its side-chain input on its main output, and 1.0 on
//! its extra output.

use std::sync::atomic::{AtomicBool, AtomicU32};

use clap_sys::ext::audio_ports::*;
use clap_sys::ext::gui::*;
use clap_sys::ext::latency::CLAP_EXT_LATENCY;
use clap_sys::ext::params::*;
use clap_sys::ext::posix_fd_support::*;
use clap_sys::ext::render::*;
use clap_sys::ext::tail::{clap_plugin_tail, CLAP_EXT_TAIL};
use clap_sys::ext::thread_check::{clap_host_thread_check, CLAP_EXT_THREAD_CHECK};
use clap_sys::ext::thread_pool::*;
use clap_sys::ext::timer_support::*;
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
    /// `timer-support.on_timer` and `posix-fd-support.on_fd` calls.
    timer_fires: AtomicU32,
    fd_fires: AtomicU32,
    /// `on_fd` calls saying the fd hung up or failed.
    fd_errors: AtomicU32,
    /// `gui.hide` and `gui.destroy` calls.
    gui_hides: AtomicU32,
    gui_destroys: AtomicU32,
    activations: AtomicU32,
    deactivations: AtomicU32,
    /// `stop_processing` calls, and calls into it while it was inactive.
    stops: AtomicU32,
    while_inactive: AtomicU32,
    active: AtomicBool,
    /// `activate` fails from now on.
    refuse_activate: AtomicBool,
    /// The render mode it was set to last (-1: never).
    render_mode: std::sync::atomic::AtomicI32,
    tail: AtomicU32,
    /// Thread-pool tasks to ask for in each `process`, and those run.
    tasks: AtomicU32,
    tasks_run: AtomicU32,
    /// Whether the host ran them.
    pool_ran: AtomicBool,
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
unsafe extern "C" fn activate(p: *const clap_plugin, _rate: f64, _min: u32, _max: u32) -> bool {
    main_only(p);
    let c = &fake(p).counts;
    c.activations.fetch_add(1, Ordering::Relaxed);
    let ok = !c.refuse_activate.load(Ordering::Relaxed);
    c.active.store(ok, Ordering::Relaxed);
    ok
}
unsafe extern "C" fn deactivate(p: *const clap_plugin) {
    main_only(p);
    let c = &fake(p).counts;
    c.deactivations.fetch_add(1, Ordering::Relaxed);
    c.active.store(false, Ordering::Relaxed);
}
unsafe extern "C" fn stop_processing(p: *const clap_plugin) {
    let c = &fake(p).counts;
    c.stops.fetch_add(1, Ordering::Relaxed);
}
unsafe extern "C" fn nothing(_p: *const clap_plugin) {}
unsafe extern "C" fn destroy(p: *const clap_plugin) {
    let f = fake(p);
    f.counts.destroyed_on_main.store(on_main(f), Ordering::Relaxed);
    f.counts.destroyed.store(true, Ordering::Relaxed);
}
unsafe extern "C" fn process(p: *const clap_plugin, process: *const clap_process) -> clap_process_status {
    let f = fake(p);
    let c = &f.counts;
    c.processed.fetch_add(1, Ordering::Relaxed);
    if !c.active.load(Ordering::Relaxed) {
        c.while_inactive.fetch_add(1, Ordering::Relaxed);
    }
    let pr = &*process;
    let n = pr.frames_count as usize;
    if pr.audio_inputs_count == 2 && pr.audio_outputs_count == 2 {
        let (ins, outs) = (std::slice::from_raw_parts(pr.audio_inputs, 2), std::slice::from_raw_parts(pr.audio_outputs, 2));
        for ch in 0..2 {
            let (main, key) = (*ins[0].data32.add(ch), *ins[1].data32.add(ch));
            let (out, extra) = (*outs[0].data32.add(ch), *outs[1].data32.add(ch));
            for i in 0..n {
                *out.add(i) = 0.5 * *main.add(i) + *key.add(i);
                *extra.add(i) = 1.0;
            }
        }
    }
    let tasks = c.tasks.load(Ordering::Relaxed);
    if tasks > 0 {
        let ext = (*f.host).get_extension.unwrap()(f.host, CLAP_EXT_THREAD_POOL.as_ptr()) as *const clap_host_thread_pool;
        let ran = (*ext).request_exec.unwrap()(f.host, tasks);
        c.pool_ran.store(ran, Ordering::Relaxed);
    }
    CLAP_PROCESS_CONTINUE
}
unsafe extern "C" fn get_extension(_p: *const clap_plugin, id: *const std::ffi::c_char) -> *const std::ffi::c_void {
    let id = std::ffi::CStr::from_ptr(id);
    if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as _
    } else if id == CLAP_EXT_LATENCY {
        &LATENCY as *const _ as _
    } else if id == CLAP_EXT_GUI {
        &GUI as *const _ as _
    } else if id == CLAP_EXT_TIMER_SUPPORT {
        &TIMER as *const _ as _
    } else if id == CLAP_EXT_POSIX_FD_SUPPORT {
        &FD as *const _ as _
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &PORTS as *const _ as _
    } else if id == CLAP_EXT_RENDER {
        &RENDER as *const _ as _
    } else if id == CLAP_EXT_TAIL {
        &TAIL as *const _ as _
    } else if id == CLAP_EXT_THREAD_POOL {
        &POOL as *const _ as _
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

/// Two stereo ports each way: the main ones, a side-chain input and an extra output.
static PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports { count: Some(port_count), get: Some(port_info) };

unsafe extern "C" fn port_count(_p: *const clap_plugin, _input: bool) -> u32 {
    2
}
unsafe extern "C" fn port_info(_p: *const clap_plugin, index: u32, _input: bool, info: *mut clap_audio_port_info) -> bool {
    let i = &mut *info;
    i.id = index;
    i.channel_count = 2;
    i.flags = if index == 0 { CLAP_AUDIO_PORT_IS_MAIN } else { 0 };
    true
}

static RENDER: clap_plugin_render = clap_plugin_render { has_hard_realtime_requirement: None, set: Some(set_render) };

unsafe extern "C" fn set_render(p: *const clap_plugin, mode: clap_plugin_render_mode) -> bool {
    main_only(p);
    fake(p).counts.render_mode.store(mode, Ordering::Relaxed);
    true
}

static TAIL: clap_plugin_tail = clap_plugin_tail { get: Some(tail) };

unsafe extern "C" fn tail(p: *const clap_plugin) -> u32 {
    fake(p).counts.tail.load(Ordering::Relaxed)
}

static POOL: clap_plugin_thread_pool = clap_plugin_thread_pool { exec: Some(exec) };

unsafe extern "C" fn exec(p: *const clap_plugin, _task: u32) {
    fake(p).counts.tasks_run.fetch_add(1, Ordering::Relaxed);
}

static TIMER: clap_plugin_timer_support = clap_plugin_timer_support { on_timer: Some(on_timer) };

unsafe extern "C" fn on_timer(p: *const clap_plugin, _id: clap_sys::id::clap_id) {
    main_only(p);
    fake(p).counts.timer_fires.fetch_add(1, Ordering::Relaxed);
}

static FD: clap_plugin_posix_fd_support = clap_plugin_posix_fd_support { on_fd: Some(on_fd) };

unsafe extern "C" fn on_fd(p: *const clap_plugin, _fd: i32, flags: clap_posix_fd_flags) {
    main_only(p);
    if flags & CLAP_POSIX_FD_READ != 0 {
        fake(p).counts.fd_fires.fetch_add(1, Ordering::Relaxed);
    }
    if flags & CLAP_POSIX_FD_ERROR != 0 {
        fake(p).counts.fd_errors.fetch_add(1, Ordering::Relaxed);
    }
}

/// A GUI that embeds anywhere and counts how it's taken down.
static GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(gui_api),
    get_preferred_api: None,
    create: Some(gui_api),
    destroy: Some(gui_destroy),
    set_scale: None,
    get_size: None,
    can_resize: None,
    get_resize_hints: None,
    adjust_size: None,
    set_size: None,
    set_parent: None,
    set_transient: None,
    suggest_title: None,
    show: None,
    hide: Some(gui_hide),
};

unsafe extern "C" fn gui_api(_p: *const clap_plugin, _api: *const std::ffi::c_char, floating: bool) -> bool {
    !floating
}
unsafe extern "C" fn gui_hide(p: *const clap_plugin) -> bool {
    fake(p).counts.gui_hides.fetch_add(1, Ordering::Relaxed);
    true
}
unsafe extern "C" fn gui_destroy(p: *const clap_plugin) {
    fake(p).counts.gui_destroys.fetch_add(1, Ordering::Relaxed);
}

/// The fake plug-in's halves, made on this thread, and what it counts. The fake itself is leaked:
/// it must outlive its halves, wherever they end.
fn halves(latency: u32) -> (ClapProcessor, ClapController, &'static Counts) {
    let (p, c, f) = fake_halves(latency);
    (p, c, &f.counts)
}

/// `halves`, with the fake itself, to call the host as the plug-in would.
fn fake_halves(latency: u32) -> (ClapProcessor, ClapController, &'static Fake) {
    mark_main_thread();
    let host = Host::new("Fake");
    let f = Box::leak(Box::new(Fake {
        plugin: clap_plugin {
            desc: std::ptr::null(),
            plugin_data: std::ptr::null_mut(),
            init: Some(yes),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(deactivate),
            start_processing: Some(yes),
            stop_processing: Some(stop_processing),
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
    f.counts.render_mode.store(-1, Ordering::Relaxed);
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
    // The host calls `activate` itself, which the fake counts.
    let (p, c) = unsafe { instantiate::start(&desc, &f.plugin, host, None, 48_000.0, 64, false) }.unwrap();
    f.counts.active.store(true, Ordering::Relaxed);
    (p, c, f)
}

const T: TransportInfo = TransportInfo {
    playing: false,
    recording: false,
    sample_rate: 48_000.0,
    tempo: 120.0,
    sig_num: 4,
    sig_den: 4,
    position_samples: 0,
    position_quarters: 0.0,
    bar_start_quarters: 0.0,
    cycle: None,
};

/// A block of `x` on the main input and `key` on the side-chain, giving the main output's first
/// frame and the extra output's.
fn keyed_block(p: &mut ClapProcessor, x: f32, key: f32) -> (f32, f32) {
    let (mut l, mut r) = ([x; 64], [x; 64]);
    let k = [key; 64];
    p.process_keyed(&mut l, &mut r, Some((&k, &k)), &[], &T);
    (l[0], p.output_bus(1, 64).map_or(f32::NAN, |b| b.0[0]))
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

/// Timers and file descriptors on the main loop: they reach the plug-in, its timers wait while it
/// isn't responding, and what it leaves registered goes with it.
#[cfg(target_os = "linux")]
#[test]
fn timers_and_file_descriptors_run_on_the_main_loop() {
    use plugin_editor::poll_loop::{run_once, PollLoop};
    plugin_editor::set_event_loop(Box::new(PollLoop));
    let (p, mut c, f) = fake_halves(0);
    let run = || {
        for _ in 0..20 {
            run_once();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    };
    let (mut timer, mut fds) = (0, [0; 2]);
    unsafe {
        let ext = |id: &std::ffi::CStr| (*f.host).get_extension.unwrap()(f.host, id.as_ptr());
        let t = &*(ext(CLAP_EXT_TIMER_SUPPORT) as *const clap_host_timer_support);
        let d = &*(ext(CLAP_EXT_POSIX_FD_SUPPORT) as *const clap_host_posix_fd_support);
        assert!(t.register_timer.unwrap()(f.host, 1, &mut timer));
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
        let (register, unregister) = (d.register_fd.unwrap(), d.unregister_fd.unwrap());
        assert!(register(f.host, fds[0], CLAP_POSIX_FD_READ));
        assert!(!register(f.host, fds[0], CLAP_POSIX_FD_READ), "once");
        run();
        let count = |n: &AtomicU32| n.load(Ordering::Relaxed);
        assert!(count(&f.counts.timer_fires) > 0);
        assert_eq!(count(&f.counts.fd_fires), 0, "nothing to read yet");
        libc::write(fds[1], b"x".as_ptr() as *const _, 1);
        run();
        assert!(count(&f.counts.fd_fires) > 0);

        // Stuck: neither its timers nor its fd (readable, unread) reach it, until it's back.
        c.set_unresponsive(true);
        let fired = (count(&f.counts.timer_fires), count(&f.counts.fd_fires));
        run();
        assert_eq!((count(&f.counts.timer_fires), count(&f.counts.fd_fires)), fired, "nothing while it's stuck");
        c.set_unresponsive(false);
        run();
        assert!(count(&f.counts.timer_fires) > fired.0 && count(&f.counts.fd_fires) > fired.1, "both again once it's back");

        let host = f.host as usize;
        let elsewhere = std::thread::spawn(move || unregister(host as *const clap_host, fds[0])).join().unwrap();
        assert!(!elsewhere, "unregistering is a main-thread call");
        assert!(unregister(f.host, fds[0]));
        let fired = count(&f.counts.fd_fires);
        run();
        assert_eq!(count(&f.counts.fd_fires), fired, "unregistered");

        // Hung up: told once, then off the loop, and it may register the fd again.
        assert!(register(f.host, fds[0], CLAP_POSIX_FD_READ));
        libc::close(fds[1]);
        run();
        run();
        assert_eq!(count(&f.counts.fd_errors), 1);
        assert!(register(f.host, fds[0], CLAP_POSIX_FD_READ), "dropped, so it can come back");
        assert!(unregister(f.host, fds[0]));
    }
    drop((p, c));
    assert!(f.counts.destroyed.load(Ordering::Relaxed));
    let fired = f.counts.timer_fires.load(Ordering::Relaxed);
    run();
    assert_eq!(f.counts.timer_fires.load(Ordering::Relaxed), fired, "the timer it left went with it");
    unsafe {
        libc::close(fds[0]);
    }
}

#[test]
fn a_gui_the_plug_in_destroyed_itself_is_only_destroyed() {
    use crate::gui::ClapGui;
    use plugin_editor::View;
    let (_p, c, f) = fake_halves(0);
    assert!(c.has_editor());
    let requests = &c.shared.requests().gui;
    let open = || unsafe { ClapGui::create(&f.plugin, c.shared.gui, requests, "Fake") }.unwrap();
    open().detach();
    assert_eq!((f.counts.gui_hides.load(Ordering::Relaxed), f.counts.gui_destroys.load(Ordering::Relaxed)), (1, 1));
    let gui = open();
    unsafe {
        let ext = (*f.host).get_extension.unwrap()(f.host, CLAP_EXT_GUI.as_ptr()) as *const clap_host_gui;
        (*ext).closed.unwrap()(f.host, true);
    }
    gui.detach();
    assert_eq!((f.counts.gui_hides.load(Ordering::Relaxed), f.counts.gui_destroys.load(Ordering::Relaxed)), (1, 2), "destroyed, not hidden");
}

#[test]
fn a_side_chain_reaches_the_plug_in_once_keyed_and_extra_outputs_are_buses() {
    let (mut p, mut c, _counts) = halves(0);
    assert!(c.has_key_input() && !c.key_active());
    assert_eq!(c.output_buses().len(), 2);
    assert_eq!(keyed_block(&mut p, 1.0, 0.25), (0.5, 1.0), "not keyed: the key's input is silent");
    p.shut_gate();
    assert!(c.parked());
    c.set_key_active(true).unwrap();
    p.open_gate(0);
    // The fade (240 frames), then wet: half the input plus the key.
    for _ in 0..4 {
        keyed_block(&mut p, 1.0, 0.25);
    }
    assert_eq!(keyed_block(&mut p, 1.0, 0.25), (0.75, 1.0));
    c.set_active_outputs(&[1]);
    assert!(c.output_buses()[1].active);
    c.set_active_outputs(&[]);
    assert!(!c.output_buses()[1].active);
}

#[test]
fn the_side_chain_gate_fades_to_dry_and_back() {
    let (mut p, _c, _counts) = halves(0);
    assert_eq!(keyed_block(&mut p, 1.0, 0.0).0, 0.5);
    p.close_gate();
    let mut l = [1.0f32; 64];
    let mut r = l;
    // 5 ms at 48 kHz is 240 frames: four blocks of 64 frames.
    let mut firsts = Vec::new();
    for _ in 0..5 {
        (l, r) = ([1.0; 64], [1.0; 64]);
        p.process(&mut l, &mut r, &[], &T);
        firsts.push(l[0]);
    }
    assert!(firsts.windows(2).all(|w| w[1] >= w[0]) && firsts[0] > 0.5, "it rises towards dry: {firsts:?}");
    assert_eq!(*firsts.last().unwrap(), 1.0, "shut: the input passes");
    assert!(p.shared.parked.load(Ordering::Acquire));
    let steps = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(steps < 0.01, "no step within the fade");
    p.open_gate(0);
    for _ in 0..5 {
        p.process(&mut l, &mut r, &[], &T);
        (l, r) = ([1.0; 64], [1.0; 64]);
    }
    assert_eq!(keyed_block(&mut p, 1.0, 0.0).0, 0.5, "wet again");
}

#[test]
fn a_restart_waits_for_the_processor_to_fade_out_then_reactivates_on_the_main_thread() {
    let (mut p, mut c, counts) = halves(64);
    keyed_block(&mut p, 1.0, 0.0);
    counts.latency.store(256, Ordering::Relaxed);
    c.shared.requests().restart.store(true, Ordering::Release);
    c.idle();
    assert_eq!(counts.deactivations.load(Ordering::Relaxed), 0, "it plays: the controller waits for it to fade out");
    assert!(p.wants_to_run(), "a sleeping strip wakes for it");
    let mut parked_after = 0;
    for b in 1..10 {
        keyed_block(&mut p, 1.0, 0.0);
        c.idle();
        if counts.deactivations.load(Ordering::Relaxed) > 0 {
            parked_after = b;
            break;
        }
    }
    assert!(parked_after >= 4, "after the fade ({parked_after} blocks)");
    assert_eq!((counts.activations.load(Ordering::Relaxed), counts.stops.load(Ordering::Relaxed)), (2, 1), "stopped, deactivated, activated");
    assert_eq!(counts.off_main.load(Ordering::Relaxed), 0);
    assert!(c.take_latency_changed() && c.latency() == 256, "with its new latency");
    assert_eq!(p.ring_out(), 256);
    // Dry until its latency has passed (four blocks), then back to wet.
    let outs: Vec<f32> = (0..12).map(|_| keyed_block(&mut p, 1.0, 0.0).0).collect();
    assert_eq!(outs[..4], [1.0; 4], "{outs:?}");
    assert_eq!(*outs.last().unwrap(), 0.5);
    assert_eq!(counts.while_inactive.load(Ordering::Relaxed), 0, "never processed while inactive");
}

#[test]
fn a_plug_in_nothing_plays_is_restarted_at_once_and_a_stuck_one_not_at_all() {
    let (mut p, mut c, counts) = halves(0);
    c.shared.requests().restart.store(true, Ordering::Release);
    c.idle();
    assert_eq!(counts.activations.load(Ordering::Relaxed), 2, "not processing: no fade to wait for");
    keyed_block(&mut p, 1.0, 0.0);
    c.set_unresponsive(true);
    c.shared.requests().restart.store(true, Ordering::Release);
    c.idle();
    c.settle();
    assert_eq!(counts.activations.load(Ordering::Relaxed), 2, "not while a thread may be in it");
    assert!(!p.shared.park.wanted(), "nor is it asked to park");
    c.set_unresponsive(false);
    // `settle` waits for the processor as a render would, so blocks run beside it.
    let blocks = std::thread::spawn(move || {
        for _ in 0..40 {
            keyed_block(&mut p, 1.0, 0.0);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        p
    });
    c.settle();
    assert_eq!(counts.activations.load(Ordering::Relaxed), 3, "settle restarts it");
    drop(blocks.join().unwrap());
}

#[test]
fn renders_switch_the_render_mode_on_the_main_thread_only() {
    let (_p, mut c, counts) = halves(0);
    c.set_offline(true);
    assert_eq!(counts.render_mode.load(Ordering::Relaxed), CLAP_RENDER_OFFLINE);
    c.set_offline(false);
    assert_eq!(counts.render_mode.load(Ordering::Relaxed), CLAP_RENDER_REALTIME);
    let c = std::thread::spawn(move || {
        c.set_offline(true);
        c
    })
    .join()
    .unwrap();
    assert_eq!(counts.render_mode.load(Ordering::Relaxed), CLAP_RENDER_REALTIME, "not from a render's thread");
    assert_eq!(counts.off_main.load(Ordering::Relaxed), 0);
    drop(c);
}

#[test]
fn a_new_tail_is_read_after_the_next_block() {
    let (mut p, _c, counts) = halves(100);
    assert_eq!(p.ring_out(), 100);
    counts.tail.store(4800, Ordering::Relaxed);
    assert_eq!(p.ring_out(), 100, "until it says so");
    unsafe {
        let ext = (*(*p.shared.inst.host).raw()).get_extension.unwrap()(p.shared.inst.host.raw(), CLAP_EXT_TAIL.as_ptr())
            as *const clap_sys::ext::tail::clap_host_tail;
        (*ext).changed.unwrap()(p.shared.inst.host.raw());
    }
    keyed_block(&mut p, 0.0, 0.0);
    assert_eq!(p.ring_out(), 4900);
    counts.tail.store(u32::MAX, Ordering::Relaxed);
    p.shared.requests().tail.store(true, Ordering::Release);
    keyed_block(&mut p, 0.0, 0.0);
    assert_eq!(p.ring_out(), u32::MAX, "endless");
}

#[test]
fn thread_pool_tasks_run_on_the_asking_thread_inside_process_only() {
    let (mut p, _c, f) = fake_halves(0);
    f.counts.tasks.store(4, Ordering::Relaxed);
    keyed_block(&mut p, 0.0, 0.0);
    assert!(f.counts.pool_ran.load(Ordering::Relaxed));
    assert_eq!(f.counts.tasks_run.load(Ordering::Relaxed), 4);
    unsafe {
        let ext = (*f.host).get_extension.unwrap()(f.host, CLAP_EXT_THREAD_POOL.as_ptr()) as *const clap_host_thread_pool;
        assert!(!(*ext).request_exec.unwrap()(f.host, 4), "refused outside process");
    }
    assert_eq!(f.counts.tasks_run.load(Ordering::Relaxed), 4);
}

/// Runs `blocks` blocks of 1.0 with no key, as `keyed_block` does, keeping every frame of the main
/// output; between them, `between(block)`.
fn run_blocks(p: &mut ClapProcessor, blocks: usize, mut between: impl FnMut(usize, &mut ClapProcessor)) -> Vec<f32> {
    let mut out = Vec::new();
    for b in 0..blocks {
        between(b, p);
        let (mut l, mut r) = ([1.0f32; 64], [1.0f32; 64]);
        let k = [0.0f32; 64];
        p.process_keyed(&mut l, &mut r, Some((&k, &k)), &[], &T);
        out.extend_from_slice(&l);
    }
    out
}

fn largest_step(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// Dragging a knob that changes the latency asks for a restart in every step, often while the
/// last one is still fading back in: each must go on from where the fade is.
#[test]
fn restarts_asked_for_while_fading_back_in_dont_click() {
    let (mut p, mut c, counts) = halves(64);
    let mut restarts = 0;
    let out = run_blocks(&mut p, 200, |b, _| {
        if b > 10 && b % 3 == 0 {
            counts.latency.store(64 + (b as u32 % 5) * 16, Ordering::Relaxed);
            c.shared.requests().restart.store(true, Ordering::Release);
        }
        let before = counts.activations.load(Ordering::Relaxed);
        c.idle();
        restarts += (counts.activations.load(Ordering::Relaxed) - before) as usize;
    });
    // Wet is 0.5, dry 1.0: a fade moves 0.5 over 240 frames.
    let step = largest_step(&out);
    assert!(restarts >= 5, "{restarts} restarts");
    assert!(step < 0.5 / 240.0 * 1.5, "largest step {step}");
    assert_eq!(counts.while_inactive.load(Ordering::Relaxed), 0);
}

#[test]
fn a_side_chain_switched_again_while_fading_in_doesnt_click() {
    let (mut p, mut c, _counts) = halves(0);
    keyed_block(&mut p, 1.0, 0.0);
    let out = run_blocks(&mut p, 40, |b, p| match b {
        2 => p.close_gate(),
        8 => {
            c.set_key_active(true).unwrap();
            p.open_gate(0);
        }
        10 => p.close_gate(),
        12 => p.open_gate(0),
        _ => {}
    });
    let step = largest_step(&out);
    assert!(step < 0.5 / 240.0 * 1.5, "largest step {step}");
}

#[test]
fn a_render_plays_a_plug_in_that_was_asked_to_park() {
    let (mut p, mut c, counts) = halves(0);
    keyed_block(&mut p, 1.0, 0.0);
    c.shared.requests().restart.store(true, Ordering::Release);
    c.idle();
    assert!(c.shared.park.wanted() && counts.activations.load(Ordering::Relaxed) == 1, "asked, not yet parked");
    // A render starts before the next idle: no controller call can come until it's over.
    p.set_offline(true);
    let out = run_blocks(&mut p, 20, |_, _| {});
    assert!(out.iter().all(|&x| x == 0.5), "it plays throughout, wet");
    p.set_offline(false);
    // Afterwards the restart goes ahead.
    for _ in 0..10 {
        keyed_block(&mut p, 1.0, 0.0);
        c.idle();
    }
    assert_eq!(counts.activations.load(Ordering::Relaxed), 2);

    // One parked already, which the controller hasn't taken up, plays in the render too.
    c.shared.requests().restart.store(true, Ordering::Release);
    c.idle();
    for _ in 0..6 {
        keyed_block(&mut p, 1.0, 0.0);
    }
    assert!(c.shared.park.take(false), "parked");
    c.shared.park.give_up();
    p.set_offline(true);
    let out = run_blocks(&mut p, 4, |_, _| {});
    assert!(out.iter().all(|&x| x == 0.5), "back in the render");
    assert_eq!(counts.while_inactive.load(Ordering::Relaxed), 0);
}

#[test]
fn a_plug_in_that_wont_start_again_passes_its_input_and_wants_nothing() {
    let (mut p, mut c, counts) = halves(128);
    keyed_block(&mut p, 1.0, 0.0);
    counts.refuse_activate.store(true, Ordering::Relaxed);
    c.shared.requests().restart.store(true, Ordering::Release);
    for _ in 0..10 {
        keyed_block(&mut p, 1.0, 0.0);
        c.idle();
    }
    assert_eq!(counts.activations.load(Ordering::Relaxed), 2, "it tried");
    assert!(c.take_latency_changed() && c.latency() == 0, "no latency: its input passes straight through");
    assert_eq!(p.ring_out(), 0);
    c.edit_param(7, 0.3);
    assert!(!p.wants_to_run() && !p.has_edits() && !c.edits_pending(), "nothing keeps its strip awake or a render waiting");
    assert_eq!(keyed_block(&mut p, 1.0, 0.0).0, 1.0, "dry");
    p.set_offline(true);
    assert_eq!(keyed_block(&mut p, 1.0, 0.0).0, 1.0, "a render doesn't take it back");
    assert_eq!(counts.while_inactive.load(Ordering::Relaxed), 0, "never processed inactive");
}
