//! The audio hosts and devices Settings › Audio lists, and finding and configuring the ones a
//! stream opens (the stream itself is in `mod.rs`).

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, Device, SampleFormat, StreamConfig, SupportedBufferSize};
use serde::Serialize;

/// One input or output device of a host, as Settings › Audio lists it.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub channels: u16,
    pub sample_rates: Vec<u32>,
    pub is_default: bool,
}

/// An audio host (ALSA, PipeWire, JACK, WASAPI...) and how many devices it offers.
#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    pub name: String,
    pub devices: usize,
}

/// What [`list_devices`] found: every host, and the devices of the chosen one.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceList {
    pub host: String,
    pub hosts: Vec<HostInfo>,
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

const COMMON_RATES: [u32; 6] = [44100, 48000, 88200, 96000, 176400, 192000];

/// Preferred driver order: talk to PipeWire natively where it runs, since going through its
/// ALSA plugin adds a second buffering stage.
const HOST_PREFERENCE: [&str; 4] = ["PipeWire", "PulseAudio", "JACK", "ALSA"];

fn host_names() -> Vec<String> {
    let mut names: Vec<String> = cpal::available_hosts().iter().map(|h| h.name().to_string()).collect();
    names.sort_by_key(|n| HOST_PREFERENCE.iter().position(|p| p == n).unwrap_or(HOST_PREFERENCE.len()));
    names
}

pub(super) fn host_by_name(name: &str) -> Option<cpal::Host> {
    cpal::available_hosts().into_iter().find(|id| id.name() == name).and_then(|id| cpal::host_from_id(id).ok())
}

pub(super) fn pick_host(name: Option<&str>) -> cpal::Host {
    if let Some(h) = name.and_then(host_by_name) {
        return h;
    }
    // JACK only counts as a default when a server is actually running.
    for n in host_names() {
        if let Some(h) = host_by_name(&n) {
            if n != "JACK" || h.devices().map(|mut d| d.next().is_some()).unwrap_or(false) {
                return h;
            }
        }
    }
    cpal::default_host()
}

pub(super) fn device_name(d: &Device) -> String {
    let name = d.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Unknown device".into());
    let id = device_id(d);
    if id == "alsa:default" || id == "pipewire:input_default" || id == "pipewire:output_default" {
        "System Default".into()
    } else if id == "alsa:pipewire" {
        "PipeWire".into()
    } else if let Some(hw) = id.strip_prefix("alsa:hw:") {
        // "M-Track 2X2, USB Audio" -> "M-Track 2X2"; secondary PCMs get their number appended.
        let base = name.split(',').next().unwrap_or(&name).trim().to_string();
        match hw.rsplit_once("DEV=") {
            Some((_, dev)) if dev != "0" => format!("{base} ({dev})"),
            _ => base,
        }
    } else {
        name
    }
}

fn device_id(d: &Device) -> String {
    d.id().map(|i| i.to_string()).unwrap_or_default()
}

fn describe(d: &Device, input: bool, default_id: &str) -> Option<DeviceInfo> {
    let configs: Vec<_> = if input { d.supported_input_configs().ok()?.collect() } else { d.supported_output_configs().ok()?.collect() };
    if configs.is_empty() {
        return None;
    }
    let channels = if input { d.default_input_config().ok() } else { d.default_output_config().ok() }
        .map(|c| c.channels())
        .unwrap_or_else(|| configs.iter().map(|c| c.channels()).min().unwrap_or(0));
    let mut rates: Vec<u32> = COMMON_RATES.iter().copied().filter(|r| configs.iter().any(|c| c.min_sample_rate() <= *r && *r <= c.max_sample_rate())).collect();
    rates.dedup();
    let id = device_id(d);
    Some(DeviceInfo { is_default: id == default_id, id, name: device_name(d), channels, sample_rates: rates })
}

/// ALSA exposes every PCM alias (front:, surround51:, iec958:, ...). Keep the system default,
/// PipeWire, and each card's direct hardware device.
fn worth_listing(id: &str, input: bool) -> bool {
    if let Some(rest) = id.strip_prefix("alsa:") {
        return rest == "pipewire" || rest.starts_with("hw:");
    }
    if let Some(rest) = id.strip_prefix("pipewire:") {
        // The synthetic defaults are covered by "System Default"; sinks also appear as
        // capture devices (their monitors), which aren't recording inputs.
        return !rest.ends_with("_default") && !(input && rest.starts_with("alsa_output"));
    }
    if id.starts_with("pulseaudio:") {
        return !(input && id.ends_with(".monitor"));
    }
    true
}

/// Enumerates hosts and the devices of `host` (or the default). Slow enough on some systems that its
/// Tauri command is async.
pub fn list_devices(host: Option<&str>) -> DeviceList {
    let host = pick_host(host);
    let def_in = host.default_input_device().map(|d| device_id(&d)).unwrap_or_default();
    let def_out = host.default_output_device().map(|d| device_id(&d)).unwrap_or_default();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    if let Ok(devs) = host.devices() {
        for d in devs {
            let id = device_id(&d);
            if worth_listing(&id, true) {
                if let Some(i) = describe(&d, true, &def_in) {
                    inputs.push(i);
                }
            }
            if worth_listing(&id, false) {
                if let Some(o) = describe(&d, false, &def_out) {
                    outputs.push(o);
                }
            }
        }
    }
    DeviceList {
        host: host.id().name().to_string(),
        hosts: host_names()
            .into_iter()
            .map(|name| {
                let devices = host_by_name(&name).and_then(|h| h.devices().ok()).map(|d| d.count()).unwrap_or(0);
                HostInfo { name, devices }
            })
            .collect(),
        inputs,
        outputs,
    }
}

pub(super) fn find_device(host: &cpal::Host, id: Option<&str>, input: bool) -> Option<Device> {
    if let Some(id) = id {
        if let Ok(devs) = host.devices() {
            for d in devs {
                if device_id(&d) == id {
                    return Some(d);
                }
            }
        }
    }
    if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    }
}

/// Chooses the widest channel layout at `rate`, preferring float samples.
pub(super) fn choose_config(d: &Device, input: bool, rate: u32, buffer: u32) -> Result<(StreamConfig, SampleFormat)> {
    let configs: Vec<_> = if input { d.supported_input_configs()?.collect() } else { d.supported_output_configs()?.collect() };
    let fmt_rank = |f: SampleFormat| match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I16 => 2,
        _ => 9,
    };
    // Use the device's own default layout: ALSA plug devices advertise up to 64 channels.
    let default_ch = if input { d.default_input_config().ok() } else { d.default_output_config().ok() }.map(|c| c.channels()).unwrap_or(2);
    let usable = |c: &&cpal::SupportedStreamConfigRange| c.min_sample_rate() <= rate && rate <= c.max_sample_rate() && fmt_rank(c.sample_format()) < 9;
    let best = configs
        .iter()
        .filter(usable)
        .filter(|c| c.channels() == default_ch)
        .min_by_key(|c| fmt_rank(c.sample_format()))
        .or_else(|| configs.iter().filter(usable).min_by_key(|c| (c.channels().abs_diff(2), fmt_rank(c.sample_format()))))
        .ok_or_else(|| anyhow!("{} does not support {rate} Hz", device_name(d)))?;
    let buffer_size = match best.buffer_size() {
        SupportedBufferSize::Range { min, max } if *min <= buffer && buffer <= *max => BufferSize::Fixed(buffer),
        SupportedBufferSize::Unknown => BufferSize::Fixed(buffer),
        _ => BufferSize::Default,
    };
    Ok((StreamConfig { channels: best.channels(), sample_rate: rate, buffer_size }, best.sample_format()))
}
