//! Audio I/O and the machine it runs on: the duplex stream that drives the engine (here), the
//! devices Settings › Audio lists (`devices.rs`), the recordings' disk writer, and the system
//! settings behind Settings › Audio (processing threads, locked memory, the input volume).
//!
//! cpal exposes input and output as separate streams. The output callback is the engine clock;
//! the input callback feeds a lock-free ring buffer that the output callback drains.

use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, Device, FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::dsp::{Engine, EngineStatus};
use std::sync::atomic::Ordering;

#[cfg(target_os = "macos")]
pub mod coreaudio;
pub mod devices;
pub mod memlock;
pub mod recorder;
#[cfg(target_os = "linux")]
pub mod system_volume;
#[cfg(target_os = "macos")]
#[path = "system_volume_macos.rs"]
pub mod system_volume;
pub mod threads;

use devices::{choose_config, device_name, find_device, host_by_name, pick_host};

#[cfg(target_os = "macos")]
pub use coreaudio::IoThread;
#[cfg(target_os = "macos")]
use coreaudio::IoThreadProbe;

/// What the workers take from the device's IO thread: on macOS its workgroup and time-constraint
/// policy (`coreaudio.rs`), elsewhere nothing.
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Default)]
pub struct IoThread {}

#[cfg(not(target_os = "macos"))]
impl IoThread {
    /// How many threads the system suggests for work beside the IO thread (macOS only).
    pub fn suggested_threads(&self) -> Option<u32> {
        None
    }
}

/// The output callback's notes about its own thread (macOS only).
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Default)]
struct IoThreadProbe {}

#[cfg(not(target_os = "macos"))]
impl IoThreadProbe {
    fn note(&self) {}
}

/// What to open: host, devices, rate and buffer size, and the engine options that go with them.
/// Saved in `settings.json`, so new fields need a default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub host: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub sample_rate: u32,
    pub buffer_size: u32,
    /// When false, no input stream is opened (playback only).
    pub input_enabled: bool,
    /// Play the input of record-enabled tracks without needing their I button.
    #[serde(default)]
    pub auto_monitor: bool,
    /// Settings › Audio › Processing Threads: how many threads play tracks, the callback's own
    /// included. 0 is Automatic ([`crate::audio_io::threads`]), 1 the callback alone.
    #[serde(default = "crate::audio_io::threads::default_setting")]
    pub processing_threads: u32,
    /// Extra offset applied to recordings on top of the measured latency, like Logic's
    /// Recording Delay. Positive values move recordings earlier.
    #[serde(default)]
    pub recording_delay_ms: f32,
    /// Settings › Audio › Lock audio memory (Linux): keep the process's memory in RAM, so the
    /// audio never waits for a page the system swapped out ([`crate::audio_io::memlock`]).
    #[serde(default = "crate::audio_io::memlock::default_setting")]
    pub lock_memory: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            host: None,
            input: None,
            output: None,
            sample_rate: 48000,
            buffer_size: 256,
            input_enabled: true,
            auto_monitor: false,
            recording_delay_ms: 0.0,
            processing_threads: crate::audio_io::threads::default_setting(),
            lock_memory: crate::audio_io::memlock::default_setting(),
        }
    }
}

impl AudioConfig {
    /// True when the two configs open the same streams (other fields can change live).
    pub fn same_devices(&self, o: &AudioConfig) -> bool {
        self.host == o.host
            && self.input == o.input
            && self.output == o.output
            && self.sample_rate == o.sample_rate
            && self.buffer_size == o.buffer_size
            && self.input_enabled == o.input_enabled
    }
}

/// What actually opened, which may differ from what was asked, or why nothing did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AudioStatus {
    pub running: bool,
    pub host: String,
    pub input_name: Option<String>,
    pub output_name: Option<String>,
    pub input_channels: u16,
    pub output_channels: u16,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub error: Option<String>,
}

/// The running streams. They live on the `audio-io` thread; dropping this stops them and joins it.
pub struct AudioIo {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    io_thread: IoThread,
}

impl AudioIo {
    /// The output device's IO thread, as the workers that play beside it take it.
    pub fn io_thread(&self) -> &IoThread {
        &self.io_thread
    }
}

impl Drop for AudioIo {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_output<T>(
    d: &Device,
    cfg: StreamConfig,
    engine: Arc<Mutex<Engine>>,
    mut in_rx: Option<rtrb::Consumer<f32>>,
    in_ch: usize,
    status: Arc<EngineStatus>,
    fast_cpus: Vec<usize>,
    probe: Arc<IoThreadProbe>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let out_ch = cfg.channels as usize;
    let mut placed = false;
    let mut out_buf = vec![0.0f32; 8192 * out_ch];
    let mut in_buf = vec![0.0f32; 8192 * in_ch.max(1)];
    let stream = d.build_output_stream::<T, _, _>(
        cfg,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            // The callback's thread is the driver's (cpal's, PipeWire's or JACK's), so it's placed
            // from inside. Refused, it plays wherever the system puts it, as it did before.
            if !placed {
                placed = true;
                let _ = threads::keep_to(&fast_cpus);
                // For the workers to copy (macOS): one Mach call, no allocation.
                probe.note();
            }
            let ts = info.timestamp();
            let lat = ts.playback.duration_since(ts.callback).as_micros() as u64;
            status.output_latency_us.store(lat, Ordering::Relaxed);
            let frames = data.len() / out_ch;
            if out_buf.len() < frames * out_ch {
                out_buf.resize(frames * out_ch, 0.0);
            }
            if in_buf.len() < frames * in_ch {
                in_buf.resize(frames * in_ch, 0.0);
            }
            let want = frames * in_ch;
            let inp = &mut in_buf[..want];
            match &mut in_rx {
                Some(rx) => {
                    // Keep input latency bounded if the two device clocks drift apart.
                    let excess = rx.slots().saturating_sub(want * 3);
                    if excess > 0 {
                        if let Ok(c) = rx.read_chunk(excess - excess % in_ch.max(1)) {
                            c.commit_all();
                        }
                    }
                    status.ring_frames.store((rx.slots() / in_ch.max(1)) as u64, Ordering::Relaxed);
                    let got = rx.slots().min(want);
                    let got = got - got % in_ch.max(1);
                    if let Ok(chunk) = rx.read_chunk(got) {
                        let (a, b) = chunk.as_slices();
                        inp[..a.len()].copy_from_slice(a);
                        inp[a.len()..a.len() + b.len()].copy_from_slice(b);
                        chunk.commit_all();
                    }
                    inp[got..].fill(0.0);
                }
                None => inp.fill(0.0),
            }
            let out = &mut out_buf[..frames * out_ch];
            match engine.try_lock() {
                Some(mut e) => {
                    // A panic must never reach the audio driver: unwinding into its C callback
                    // aborts the whole app. Silence the block instead; the session reports it.
                    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.process(inp, in_ch, out, out_ch))).is_ok();
                    if !ok {
                        out.fill(0.0);
                        e.status.engine_errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
                None => out.fill(0.0),
            }
            for (d, s) in data.iter_mut().zip(out.iter()) {
                *d = T::from_sample(s.clamp(-4.0, 4.0));
            }
        },
        |e| log::error!("output stream error: {e}"),
        None,
    )?;
    Ok(stream)
}

fn build_input<T>(d: &Device, cfg: StreamConfig, mut tx: rtrb::Producer<f32>, status: Arc<EngineStatus>) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = d.build_input_stream::<T, _, _>(
        cfg,
        move |data: &[T], info: &cpal::InputCallbackInfo| {
            let ts = info.timestamp();
            let lat = ts.callback.duration_since(ts.capture).as_micros() as u64;
            status.input_latency_us.store(lat, Ordering::Relaxed);
            let n = data.len().min(tx.slots());
            if let Ok(mut chunk) = tx.write_chunk(n) {
                let (a, b) = chunk.as_mut_slices();
                for (dst, src) in a.iter_mut().chain(b.iter_mut()).zip(data.iter()) {
                    *dst = f32::from_sample(*src);
                }
                chunk.commit_all();
            }
        },
        |e| log::error!("input stream error: {e}"),
        None,
    )?;
    Ok(stream)
}

macro_rules! by_format {
    ($fmt:expr, $f:ident, $($args:expr),*) => {
        match $fmt {
            SampleFormat::F32 => $f::<f32>($($args),*),
            SampleFormat::I32 => $f::<i32>($($args),*),
            SampleFormat::I16 => $f::<i16>($($args),*),
            other => Err(anyhow!("unsupported sample format {other:?}")),
        }
    };
}

/// ALSA's `null` PCM, which the headless checks play to: silent, and as fast as the engine runs.
pub const NULL_OUTPUT: &str = "alsa:null";

/// Stands in for ALSA's `null` PCM where there is no ALSA (macOS, Windows), so the headless
/// checks never fall back to the speakers: a thread runs the engine block after block, with no
/// input and nothing played, at exactly the configured rate and buffer size.
fn start_null(engine: Arc<Mutex<Engine>>, cfg: &AudioConfig) -> (Option<AudioIo>, AudioStatus) {
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let frames = (cfg.buffer_size as usize).max(1);
    let st = engine.lock().status.clone();
    st.input_latency_us.store(0, Ordering::Relaxed);
    st.output_latency_us.store(0, Ordering::Relaxed);
    st.ring_frames.store(0, Ordering::Relaxed);
    let thread = std::thread::Builder::new()
        .name("audio-null".into())
        .spawn(move || {
            let mut out = vec![0.0f32; frames * 2];
            while let Err(std::sync::mpsc::TryRecvError::Empty) = stop_rx.try_recv() {
                match engine.try_lock() {
                    Some(mut e) => {
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.process(&[], 0, &mut out, 2))).is_err() {
                            e.status.engine_errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    None => std::thread::yield_now(),
                }
            }
        })
        .expect("spawn audio thread");
    let status = AudioStatus {
        running: true,
        host: "Null".into(),
        output_name: Some("Null Output".into()),
        output_channels: 2,
        sample_rate: cfg.sample_rate,
        buffer_size: frames as u32,
        ..Default::default()
    };
    (Some(AudioIo { stop: Some(stop_tx), thread: Some(thread), io_thread: IoThread::default() }), status)
}

/// cpal's real-time promotion sets the process's soft RLIMIT_RTTIME to a single buffer (10.7 ms at
/// 512 samples), and the default action for the SIGXCPU it raises ends the process with a core
/// dump. One slow block must only cost a dropout, so ignore it. The 200 ms hard limit still holds,
/// and the engine sheds load well before that (`OVERLOAD_SECS`).
pub(crate) fn survive_slow_blocks() {
    #[cfg(target_os = "linux")]
    unsafe {
        libc::signal(libc::SIGXCPU, libc::SIG_IGN);
    }
}

/// Opens the devices on a new `audio-io` thread that calls `Engine::process` for each buffer, and
/// returns once they are open or have failed. `AudioStatus` says which; on failure there is no `AudioIo`.
pub fn start(engine: Arc<Mutex<Engine>>, cfg: &AudioConfig) -> (Option<AudioIo>, AudioStatus) {
    survive_slow_blocks();
    if cfg.output.as_deref() == Some(NULL_OUTPUT) && host_by_name("ALSA").is_none() {
        return start_null(engine, cfg);
    }
    let (res_tx, res_rx) = std::sync::mpsc::channel::<(AudioStatus, IoThread)>();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let cfg = cfg.clone();
    let thread = std::thread::Builder::new()
        .name("audio-io".into())
        .spawn(move || {
            let mut status = AudioStatus { sample_rate: cfg.sample_rate, buffer_size: cfg.buffer_size, ..Default::default() };
            let status_arc = engine.lock().status.clone();
            status_arc.input_latency_us.store(0, Ordering::Relaxed);
            status_arc.output_latency_us.store(0, Ordering::Relaxed);
            status_arc.ring_frames.store(0, Ordering::Relaxed);
            let result = (|| -> Result<((cpal::Stream, Option<cpal::Stream>), IoThread)> {
                let host = pick_host(cfg.host.as_deref());
                status.host = host.id().name().to_string();
                let out_dev = find_device(&host, cfg.output.as_deref(), false).ok_or_else(|| anyhow!("no output device"))?;
                status.output_name = Some(device_name(&out_dev));
                let (out_cfg, out_fmt) = choose_config(&out_dev, false, cfg.sample_rate, cfg.buffer_size)?;
                status.output_channels = out_cfg.channels;
                if let BufferSize::Fixed(n) = out_cfg.buffer_size {
                    status.buffer_size = n;
                }

                let mut input = None;
                if cfg.input_enabled {
                    if let Some(in_dev) = find_device(&host, cfg.input.as_deref(), true) {
                        match choose_config(&in_dev, true, cfg.sample_rate, cfg.buffer_size) {
                            Ok((in_cfg, in_fmt)) => {
                                let ch = in_cfg.channels as usize;
                                let (tx, rx) = rtrb::RingBuffer::new(cfg.sample_rate as usize * ch);
                                match by_format!(in_fmt, build_input, &in_dev, in_cfg, tx, status_arc.clone()) {
                                    Ok(s) => {
                                        status.input_name = Some(device_name(&in_dev));
                                        status.input_channels = ch as u16;
                                        input = Some((s, rx, ch));
                                    }
                                    Err(e) => status.error = Some(format!("input: {e}")),
                                }
                            }
                            Err(e) => status.error = Some(format!("input: {e}")),
                        }
                    }
                }
                let (in_stream, in_rx, in_ch) = match input {
                    Some((s, rx, ch)) => (Some(s), Some(rx), ch),
                    None => (None, None, 0),
                };
                let fast = threads::Topology::detect().fast;
                let probe = Arc::new(IoThreadProbe::default());
                let out = by_format!(out_fmt, build_output, &out_dev, out_cfg, engine.clone(), in_rx, in_ch, status_arc.clone(), fast, probe.clone())?;
                if let Some(s) = &in_stream {
                    s.play()?;
                }
                out.play()?;
                #[cfg(target_os = "macos")]
                let io_thread = {
                    let io = out_dev.id().map(|id| IoThread::of(id.id(), &probe)).unwrap_or_default();
                    log::info!(
                        "audio device's IO thread: workgroup {}, time constraint {:?}, suggested threads {:?}",
                        if io.workgroup.is_some() { "found" } else { "none" },
                        io.policy,
                        io.suggested_threads()
                    );
                    io
                };
                #[cfg(not(target_os = "macos"))]
                let io_thread = {
                    drop(probe);
                    IoThread {}
                };
                Ok(((out, in_stream), io_thread))
            })();
            match result {
                Ok((streams, io_thread)) => {
                    status.running = true;
                    let _ = res_tx.send((status, io_thread));
                    let _ = stop_rx.recv();
                    drop(streams);
                }
                Err(e) => {
                    status.error = Some(e.to_string());
                    let _ = res_tx.send((status, IoThread::default()));
                }
            }
        })
        .expect("spawn audio thread");

    let (status, io_thread) = res_rx.recv().unwrap_or_default();
    if status.running {
        (Some(AudioIo { stop: Some(stop_tx), thread: Some(thread), io_thread }), status)
    } else {
        let _ = thread.join();
        (None, status)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn sigxcpu_does_not_end_the_process() {
        super::survive_slow_blocks();
        let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::sigaction(libc::SIGXCPU, std::ptr::null(), &mut old) }, 0);
        assert_eq!(old.sa_sigaction, libc::SIG_IGN);
    }
}
