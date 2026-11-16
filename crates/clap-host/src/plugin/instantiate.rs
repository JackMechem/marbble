//! Making a CLAP plug-in: creating it from its file's factory, initializing and activating it on
//! the main thread, and setting up the two halves.

use std::ffi::CString;

use clap_sys::ext::gui::CLAP_EXT_GUI;
use clap_sys::ext::latency::CLAP_EXT_LATENCY;
use clap_sys::ext::params::CLAP_EXT_PARAMS;
use clap_sys::ext::render::CLAP_EXT_RENDER;
use clap_sys::ext::state::CLAP_EXT_STATE;
use clap_sys::ext::tail::CLAP_EXT_TAIL;
use clap_sys::ext::thread_pool::{clap_plugin_thread_pool, CLAP_EXT_THREAD_POOL};

use super::*;
use crate::host::mark_main_thread;
use crate::params;

/// Loads a plug-in, ready to process at `sample_rate` in blocks of up to `max_block` frames, with
/// a side-chain key reaching its input (`keyed`) where it has one. Call on the main thread, which
/// every controller call must then come from.
pub fn instantiate(desc: &PluginDescriptor, sample_rate: f64, max_block: usize, keyed: bool) -> Result<(ClapProcessor, ClapController), String> {
    let lib = crate::entry::load(&desc.path)?;
    mark_main_thread();
    let host = Host::new(&desc.name);
    let id = CString::new(desc.cid.as_bytes()).map_err(|_| "a NUL in its id")?;
    unsafe {
        let plugin = lib.create(host.raw(), &id);
        if plugin.is_null() {
            return Err("its factory couldn't create it".into());
        }
        start(desc, plugin, host, Some(lib), sample_rate, max_block, keyed)
    }
}

/// Initializes and activates a plug-in just created with `host`, and makes its two halves.
pub(super) unsafe fn start(
    desc: &PluginDescriptor,
    plugin: *const clap_plugin,
    host: Box<Host>,
    lib: Option<Arc<Library>>,
    sample_rate: f64,
    max_block: usize,
    keyed: bool,
) -> Result<(ClapProcessor, ClapController), String> {
    {
        let p = &*plugin;
        // Before `init`, from which it may register timers and file descriptors.
        host.set_plugin(plugin);
        // Until it's activated, an early return destroys it.
        let unwind = Unwind(plugin, &host);
        if !p.init.is_some_and(|f| f(plugin)) {
            return Err("it failed to initialize".into());
        }
        let ext = |id: &std::ffi::CStr| p.get_extension.map_or(std::ptr::null(), |f| f(plugin, id.as_ptr()));
        let params_ext = ext(CLAP_EXT_PARAMS) as *const clap_plugin_params;
        let state = ext(CLAP_EXT_STATE) as *const clap_plugin_state;
        let latency = ext(CLAP_EXT_LATENCY) as *const clap_plugin_latency;
        let tail = ext(CLAP_EXT_TAIL) as *const clap_plugin_tail;
        let render = ext(CLAP_EXT_RENDER) as *const clap_plugin_render;
        host.set_thread_pool(ext(CLAP_EXT_THREAD_POOL) as *const clap_plugin_thread_pool);
        let gui = ext(CLAP_EXT_GUI) as *const clap_plugin_gui;
        let gui = if crate::gui::embeds(plugin, gui) { gui } else { std::ptr::null() };
        let in_ports = ports::audio_ports(plugin, true);
        let out_ports = ports::audio_ports(plugin, false);
        let notes = ports::notes(plugin);
        if !p.activate.is_some_and(|f| f(plugin, sample_rate, 1, max_block as u32)) {
            return Err(format!("it refused to start at {sample_rate} Hz"));
        }
        std::mem::forget(unwind);

        let infos = params::infos(plugin, params_ext);
        let lat = latency.as_ref().and_then(|l| l.get).map_or(0, |g| g(plugin));
        let tail_now = tail.as_ref().and_then(|t| t.get).map_or(0, |g| g(plugin));
        let key = ports::key_ports(&in_ports);
        let shared = Arc::new(Shared {
            instance: plugin_api::next_instance(),
            plugin,
            params: params_ext,
            state,
            latency,
            tail,
            render,
            gui,
            parked: AtomicBool::new(false),
            park: Default::default(),
            processing: AtomicBool::new(false),
            keyed: AtomicBool::new(keyed && key.first.is_some()),
            edits_in_block: AtomicBool::new(false),
            latency_now: AtomicU32::new(lat),
            inst: ManuallyDrop::new(Instance { plugin, host, active: AtomicBool::new(true), _lib: lib }),
        });
        let (edits_tx, edits_rx) = rtrb::RingBuffer::new(RING);
        let (gestures_tx, gestures_rx) = rtrb::RingBuffer::new(RING);
        let inputs: Vec<Port> = in_ports.iter().map(|p| Port::new(p.channels, max_block)).collect();
        let outputs: Vec<Port> = out_ports.iter().map(|p| Port::new(p.channels, max_block)).collect();
        let blank = clap_audio_buffer { data32: std::ptr::null_mut(), data64: std::ptr::null_mut(), channel_count: 0, latency: 0, constant_mask: 0 };
        let proc_half = ClapProcessor {
            shared: shared.clone(),
            in_bufs: vec![blank; inputs.len()],
            out_bufs: vec![blank; outputs.len()],
            inputs,
            outputs,
            main_in: ports::main_port(&in_ports),
            main_out: ports::main_port(&out_ports),
            key,
            bus_ports: ports::bus_order(&out_ports),
            events_in: InputEvents::new(MAX_EVENTS),
            events_out: OutputEvents::new(gestures_tx, params::ranges(&infos)),
            transport: std::mem::zeroed(),
            edits_rx,
            queued: Vec::with_capacity(MAX_QUEUED),
            notes,
            gate: park::Fade::Wet,
            park_fade: park::Fade::Wet,
            fade_len: ((park::FADE_SECS * sample_rate) as u32).max(1),
            dry_l: vec![0.0; max_block],
            dry_r: vec![0.0; max_block],
            wet: vec![0.0; max_block],
            steady: 0,
            max_block,
            tail: tail_now,
            name: desc.name.clone(),
        };
        let ctrl_half = ClapController {
            desc: desc.clone(),
            shared,
            gestures_rx,
            driven: Default::default(),
            edits_tx: parking_lot::Mutex::new(edits_tx),
            host_set: HostSet::default(),
            buses: ports::bus_layouts(&in_ports, &out_ports),
            outputs: ports::output_buses(&out_ports),
            ranges: params::ranges(&infos),
            latency_changed: false,
            params_restarted: false,
            key_active: keyed && key.first.is_some(),
            unresponsive: false,
            latency: lat,
            sample_rate,
            max_block,
            restart: None,
            dead: false,
            offline: false,
            editor: None,
        };
        Ok((proc_half, ctrl_half))
    }
}

/// How long its output may outlast its input: the tail and the latency, or `u32::MAX` for a tail
/// that never ends (CLAP's is `INT32_MAX` or more then).
pub(super) fn ring_out(tail: u32, latency: u32) -> u32 {
    if tail >= i32::MAX as u32 {
        u32::MAX
    } else {
        tail.saturating_add(latency)
    }
}

/// Destroys a plug-in that `instantiate` gives up on before it's active.
struct Unwind<'a>(*const clap_plugin, &'a Host);

impl Drop for Unwind<'_> {
    fn drop(&mut self) {
        unsafe {
            if let Some(f) = (*self.0).destroy {
                f(self.0);
            }
        }
        self.1.forget_sources();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_endless_tail_rings_out_forever() {
        assert_eq!(ring_out(100, 64), 164);
        assert_eq!(ring_out(i32::MAX as u32, 64), u32::MAX);
        assert_eq!(ring_out(u32::MAX, 0), u32::MAX);
    }
}
