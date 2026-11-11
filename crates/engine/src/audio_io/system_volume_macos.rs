//! The system's input volume on the recording device, on macOS: the level and mute that Core Audio
//! keeps on a device's input (System Settings › Sound › Input, Audio MIDI Setup). Anything below
//! 100% lowers what gets recorded before Marbble sees it, and a muted input records silence.
//! The same API as the Linux `system_volume.rs`, so Settings › Audio's check works unchanged.
//!
//! These are device properties read and set through `AudioObjectGetPropertyData` and
//! `AudioObjectSetPropertyData`, as Audio MIDI Setup does: no input stream is opened and no audio
//! is captured, so they don't ask for microphone access.
//!
//! A device keeps its volume and mute either on its main element (most built-in and many USB
//! devices) or on each input channel (elements 1..n), or has none (most audio interfaces, whose
//! gain is the knob on the front): then there is nothing to check.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_core_audio::{
    kAudioDevicePropertyMute, kAudioDevicePropertyStreamConfiguration, kAudioDevicePropertyVolumeDecibels, kAudioDevicePropertyVolumeRangeDecibels,
    kAudioDevicePropertyVolumeScalar, kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput, kAudioObjectSystemObject, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectHasProperty, AudioObjectID, AudioObjectIsPropertySettable, AudioObjectPropertyAddress, AudioObjectPropertySelector, AudioObjectSetPropertyData,
};
use objc2_core_foundation::{CFRetained, CFString};
use serde::Serialize;

use super::coreaudio::device_id;

/// The system's volume for a recording device, which can quietly lower what gets recorded.
#[derive(Debug, Clone, Serialize)]
pub struct InputLevel {
    /// Human-readable device name.
    pub device: String,
    /// The device's input volume, 1.0 = 100% (the lowest channel's when each has its own).
    pub volume: f32,
    /// The resulting attenuation in dB (0 at 100%).
    pub gain_db: f32,
    pub muted: bool,
    #[serde(skip)]
    target: AudioObjectID,
}

impl InputLevel {
    /// True when the system is lowering or muting the signal.
    pub fn attenuating(&self) -> bool {
        self.muted || self.gain_db < -0.5
    }
}

/// The Core Audio UID in a cpal device id ("coreaudio:<UID>"), or None for System Default.
fn uid_of(device: Option<&str>) -> Option<&str> {
    let id = device?;
    Some(id.strip_prefix("coreaudio:").unwrap_or(id)).filter(|uid| !uid.is_empty())
}

/// The device a recording from `device` uses: the one its UID names, or the default input when it
/// names none or a device that's gone (as `find_device` falls back when the engine opens it).
fn target_for(device: Option<&str>) -> Option<AudioObjectID> {
    uid_of(device).and_then(device_id).or_else(default_input)
}

fn address(selector: AudioObjectPropertySelector, scope: u32, element: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { mSelector: selector, mScope: scope, mElement: element }
}

fn input(selector: AudioObjectPropertySelector, element: u32) -> AudioObjectPropertyAddress {
    address(selector, kAudioObjectPropertyScopeInput, element)
}

fn has(object: AudioObjectID, addr: &AudioObjectPropertyAddress) -> bool {
    // SAFETY: a query with a valid address.
    unsafe { AudioObjectHasProperty(object, NonNull::from(addr)) }
}

fn settable(object: AudioObjectID, addr: &AudioObjectPropertyAddress) -> bool {
    let mut out = 0u8;
    // SAFETY: a query with a valid address, into a Boolean.
    let status = unsafe { AudioObjectIsPropertySettable(object, NonNull::from(addr), NonNull::from(&mut out)) };
    status == 0 && out != 0
}

/// Reads a fixed-size property of type `T`.
fn get<T: Copy + Default>(object: AudioObjectID, addr: &AudioObjectPropertyAddress) -> Option<T> {
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    // SAFETY: the properties read through here have exactly the type `T` (callers pair them), of
    // the size given.
    let status =
        unsafe { AudioObjectGetPropertyData(object, NonNull::from(addr), 0, std::ptr::null(), NonNull::from(&mut size), NonNull::from(&mut value).cast()) };
    (status == 0 && size == size_of::<T>() as u32).then_some(value)
}

fn set<T: Copy>(object: AudioObjectID, addr: &AudioObjectPropertyAddress, value: T) -> Result<(), i32> {
    // SAFETY: as `get`, the property has type `T`; Core Audio copies the value.
    let status =
        unsafe { AudioObjectSetPropertyData(object, NonNull::from(addr), 0, std::ptr::null(), size_of::<T>() as u32, NonNull::from(&value).cast::<c_void>()) };
    if status == 0 {
        Ok(())
    } else {
        Err(status)
    }
}

fn default_input() -> Option<AudioObjectID> {
    let addr = address(kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain);
    get::<AudioObjectID>(kAudioObjectSystemObject as AudioObjectID, &addr).filter(|&d| d != 0)
}

fn name(device: AudioObjectID) -> String {
    let addr = address(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain);
    // The property gives a CFStringRef the caller owns.
    get::<usize>(device, &addr)
        .and_then(|p| NonNull::new(p as *mut CFString))
        // SAFETY: a +1 CFString from Core Audio, released when this is dropped.
        .map(|p| unsafe { CFRetained::from_raw(p) }.to_string())
        .unwrap_or_else(|| "Input device".into())
}

/// How many input channels the device has (from its input stream configuration, an
/// AudioBufferList).
fn input_channels(device: AudioObjectID) -> u32 {
    #[repr(C)]
    struct Buffer {
        channels: u32,
        bytes: u32,
        data: *mut c_void,
    }
    let addr = input(kAudioDevicePropertyStreamConfiguration, kAudioObjectPropertyElementMain);
    let mut size = 0u32;
    // SAFETY: a size query with a valid address.
    if unsafe { AudioObjectGetPropertyDataSize(device, NonNull::from(&addr), 0, std::ptr::null(), NonNull::from(&mut size)) } != 0 || size < 4 {
        return 0;
    }
    // u64s, for the buffers' pointer alignment.
    let mut list = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: room for `size` bytes, aligned for an AudioBufferList.
    if unsafe {
        AudioObjectGetPropertyData(device, NonNull::from(&addr), 0, std::ptr::null(), NonNull::from(&mut size), NonNull::new(list.as_mut_ptr()).unwrap().cast())
    } != 0
    {
        return 0;
    }
    let base = list.as_ptr() as *const u8;
    // SAFETY: an AudioBufferList is a u32 count, then that many Buffers from offset 8 (pointer
    // alignment); only the buffers that fit in what Core Audio wrote are read.
    unsafe {
        let count = *(base as *const u32) as usize;
        let fit = (size as usize).saturating_sub(8) / size_of::<Buffer>();
        (0..count.min(fit)).map(|i| (*(base.add(8) as *const Buffer).add(i)).channels).sum()
    }
}

/// The elements that carry `selector` on the device's input: the main element when it does, else
/// each channel that does.
fn elements(device: AudioObjectID, selector: AudioObjectPropertySelector) -> Vec<u32> {
    if has(device, &input(selector, kAudioObjectPropertyElementMain)) {
        return vec![kAudioObjectPropertyElementMain];
    }
    (1..=input_channels(device)).filter(|&ch| has(device, &input(selector, ch))).collect()
}

/// One element's volume: its scalar, and its attenuation in dB below its own maximum.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Channel {
    scalar: f32,
    gain_db: f32,
}

fn channel_volume(device: AudioObjectID, element: u32) -> Option<Channel> {
    let scalar = get::<f32>(device, &input(kAudioDevicePropertyVolumeScalar, element))?;
    let db = get::<f32>(device, &input(kAudioDevicePropertyVolumeDecibels, element));
    // AudioValueRange: two f64s, minimum and maximum.
    let range = get::<[f64; 2]>(device, &input(kAudioDevicePropertyVolumeRangeDecibels, element));
    Some(Channel { scalar, gain_db: gain_db(scalar, db.zip(range).map(|(db, r)| (db, r[1] as f32))) })
}

/// The attenuation below 100%: from the device's own dB scale when it has one (its level less its
/// maximum), else from the scalar taken as linear gain.
fn gain_db(scalar: f32, db_and_max: Option<(f32, f32)>) -> f32 {
    match db_and_max {
        Some((db, max)) if db.is_finite() && max.is_finite() => (db - max).clamp(-120.0, 0.0),
        _ if scalar <= 0.0 => -120.0,
        _ => (20.0 * scalar.min(1.0).log10()).max(-120.0),
    }
}

/// The channel that's turned down most stands for the device.
fn lowest(channels: &[Channel]) -> Option<Channel> {
    channels.iter().copied().min_by(|a, b| a.gain_db.total_cmp(&b.gain_db))
}

pub fn input_level(device: Option<&str>) -> Option<InputLevel> {
    level_of(target_for(device)?)
}

fn level_of(target: AudioObjectID) -> Option<InputLevel> {
    let volumes: Vec<Channel> = elements(target, kAudioDevicePropertyVolumeScalar).into_iter().filter_map(|e| channel_volume(target, e)).collect();
    let mutes: Vec<u32> =
        elements(target, kAudioDevicePropertyMute).into_iter().filter_map(|e| get::<u32>(target, &input(kAudioDevicePropertyMute, e))).collect();
    if volumes.is_empty() && mutes.is_empty() {
        // Nothing the system could be turning down.
        return None;
    }
    let level = lowest(&volumes).unwrap_or(Channel { scalar: 1.0, gain_db: 0.0 });
    Some(InputLevel { device: name(target), volume: level.scalar, gain_db: level.gain_db, muted: mutes.iter().any(|&m| m != 0), target })
}

/// Sets the recording device's system input volume to 100% and unmutes it.
pub fn reset_input_level(device: Option<&str>) -> Result<Option<InputLevel>, String> {
    let Some(level) = input_level(device) else { return Ok(None) };
    let target = level.target;
    for e in elements(target, kAudioDevicePropertyVolumeScalar) {
        let addr = input(kAudioDevicePropertyVolumeScalar, e);
        if settable(target, &addr) {
            set(target, &addr, 1.0f32).map_err(|s| format!("Couldn't set {}'s input volume (Core Audio error {s})", level.device))?;
        }
    }
    for e in elements(target, kAudioDevicePropertyMute) {
        let addr = input(kAudioDevicePropertyMute, e);
        if settable(target, &addr) {
            set(target, &addr, 0u32).map_err(|s| format!("Couldn't unmute {}'s input (Core Audio error {s})", level.device))?;
        }
    }
    Ok(level_of(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cpal_id_gives_its_uid() {
        assert_eq!(uid_of(Some("coreaudio:BuiltInMicrophoneDevice")), Some("BuiltInMicrophoneDevice"));
        assert_eq!(uid_of(Some("AppleUSBAudioEngine:Foo:1")), Some("AppleUSBAudioEngine:Foo:1"));
        assert_eq!(uid_of(None), None);
        assert_eq!(uid_of(Some("coreaudio:")), None);
    }

    #[test]
    fn gain_is_measured_below_the_devices_maximum() {
        assert_eq!(gain_db(1.0, Some((12.0, 12.0))), 0.0);
        assert_eq!(gain_db(0.5, Some((-3.0, 9.0))), -12.0);
        // A device without a dB scale: the scalar as linear gain.
        assert!((gain_db(0.5, None) + 6.02).abs() < 0.01);
        assert_eq!(gain_db(1.0, None), 0.0);
        assert_eq!(gain_db(0.0, None), -120.0);
        assert_eq!(gain_db(0.3, Some((f32::NEG_INFINITY, 0.0))), 20.0 * 0.3f32.log10());
    }

    #[test]
    fn the_lowest_channel_stands_for_the_device() {
        let full = Channel { scalar: 1.0, gain_db: 0.0 };
        let low = Channel { scalar: 0.4, gain_db: -18.0 };
        assert_eq!(lowest(&[full, low, full]), Some(low));
        assert_eq!(lowest(&[]), None);
    }

    #[test]
    fn a_turned_down_or_muted_input_attenuates() {
        let level = |gain_db, muted| InputLevel { device: String::new(), volume: 1.0, gain_db, muted, target: 0 };
        assert!(!level(0.0, false).attenuating());
        assert!(!level(-0.3, false).attenuating());
        assert!(level(-6.0, false).attenuating());
        assert!(level(0.0, true).attenuating());
    }

    #[test]
    fn an_unknown_uid_falls_back_to_the_default_input() {
        // Read only: no input stream is opened.
        assert_eq!(target_for(Some("coreaudio:no-such-device-uid")), default_input());
    }
}
