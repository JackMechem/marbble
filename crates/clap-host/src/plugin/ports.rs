//! The plug-in's audio and note ports, read once it's initialized, and the buffers for its audio
//! ports.

use clap_sys::ext::audio_ports::*;
use clap_sys::ext::note_ports::*;
use clap_sys::plugin::clap_plugin;
use plugin_api::{BusLayout, OutputBus};

use super::Port;
use crate::entry::buf_text;

/// An audio port as the plug-in describes it.
pub(super) struct PortInfo {
    pub name: String,
    pub channels: usize,
    pub main: bool,
}

unsafe fn extension<T>(plugin: *const clap_plugin, id: &std::ffi::CStr) -> Option<&'static T> {
    let get = (*plugin).get_extension?;
    (get(plugin, id.as_ptr()) as *const T).as_ref()
}

/// The plug-in's audio inputs or outputs. One without the extension has none.
pub(super) unsafe fn audio_ports(plugin: *const clap_plugin, input: bool) -> Vec<PortInfo> {
    let Some(ext) = extension::<clap_plugin_audio_ports>(plugin, CLAP_EXT_AUDIO_PORTS) else { return Vec::new() };
    let (Some(count), Some(get)) = (ext.count, ext.get) else { return Vec::new() };
    let mut out = Vec::new();
    for i in 0..count(plugin, input) {
        let mut info: clap_audio_port_info = std::mem::zeroed();
        if get(plugin, i, input, &mut info) {
            out.push(PortInfo { name: buf_text(&info.name), channels: info.channel_count as usize, main: info.flags & CLAP_AUDIO_PORT_IS_MAIN != 0 });
        } else {
            out.push(PortInfo { name: String::new(), channels: 0, main: false });
        }
    }
    out
}

/// The main port: the one flagged so, or else the first.
pub(super) fn main_port(ports: &[PortInfo]) -> Option<usize> {
    ports.iter().position(|p| p.main).or(if ports.is_empty() { None } else { Some(0) })
}

/// How the plug-in takes notes on its first note input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Notes {
    /// CLAP note events; CCs and pitch bend still go as MIDI if it takes that too.
    Clap {
        midi: bool,
    },
    /// MIDI 1.0 messages only.
    Midi,
    None,
}

pub(super) unsafe fn notes(plugin: *const clap_plugin) -> Notes {
    let Some(ext) = extension::<clap_plugin_note_ports>(plugin, CLAP_EXT_NOTE_PORTS) else { return Notes::None };
    let (Some(count), Some(get)) = (ext.count, ext.get) else { return Notes::None };
    if count(plugin, true) == 0 {
        return Notes::None;
    }
    let mut info: clap_note_port_info = std::mem::zeroed();
    if !get(plugin, 0, true, &mut info) {
        return Notes::None;
    }
    let midi = info.supported_dialects & CLAP_NOTE_DIALECT_MIDI != 0;
    if info.supported_dialects & CLAP_NOTE_DIALECT_CLAP != 0 {
        Notes::Clap { midi }
    } else if midi {
        Notes::Midi
    } else {
        Notes::None
    }
}

impl Port {
    pub(super) fn new(channels: usize, max_block: usize) -> Port {
        let mut buffers: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0; max_block]).collect();
        let ptrs = buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
        Port { channels, buffers, ptrs }
    }

    /// Puts `n` frames of one channel on the first, silence on the rest.
    pub(super) fn put_mono(&mut self, x: &[f32], n: usize) {
        if let Some((first, rest)) = self.buffers.split_first_mut() {
            first[..n].copy_from_slice(&x[..n]);
            for b in rest {
                b[..n].fill(0.0);
            }
        }
    }

    /// Puts `n` frames of a stereo pair on the first two channels (mixed to one on a mono port),
    /// silence on the rest.
    pub(super) fn put_stereo(&mut self, l: &[f32], r: &[f32], n: usize) {
        match self.channels {
            0 => {}
            1 => {
                for i in 0..n {
                    self.buffers[0][i] = 0.5 * (l[i] + r[i]);
                }
            }
            _ => {
                self.buffers[0][..n].copy_from_slice(&l[..n]);
                self.buffers[1][..n].copy_from_slice(&r[..n]);
                for b in self.buffers.iter_mut().skip(2) {
                    b[..n].fill(0.0);
                }
            }
        }
    }

    pub(super) fn silence(&mut self, n: usize) {
        for b in self.buffers.iter_mut() {
            b[..n].fill(0.0);
        }
    }

    /// The first two channels (a mono port's one on both sides), or None without channels.
    pub(super) fn stereo(&self, n: usize) -> Option<(&[f32], &[f32])> {
        match self.channels {
            0 => None,
            1 => Some((&self.buffers[0][..n], &self.buffers[0][..n])),
            _ => Some((&self.buffers[0][..n], &self.buffers[1][..n])),
        }
    }
}

/// The ports in the engine's terms. Every port past the main ones counts as aux.
pub(super) fn bus_layouts(inputs: &[PortInfo], outputs: &[PortInfo]) -> Vec<BusLayout> {
    let side = |ports: &[PortInfo], input: bool| {
        let main = main_port(ports);
        ports
            .iter()
            .enumerate()
            .map(move |(i, p)| BusLayout { name: p.name.clone(), input, aux: Some(i) != main, default_active: true, channels: p.channels })
            .collect::<Vec<_>>()
    };
    side(inputs, true).into_iter().chain(side(outputs, false)).collect()
}

/// The output ports in the engine's bus order: the main one first, then the rest as the plug-in
/// lists them. Bus `i` is port `order[i]`.
pub(super) fn bus_order(outputs: &[PortInfo]) -> Vec<usize> {
    let main = main_port(outputs);
    main.into_iter().chain((0..outputs.len()).filter(|&i| Some(i) != main)).collect()
}

/// The output buses the engine can route, in `bus_order`. Only the main one starts in use: the
/// session says which others auxes take (`set_active_outputs`).
pub(super) fn output_buses(outputs: &[PortInfo]) -> Vec<OutputBus> {
    bus_order(outputs)
        .into_iter()
        .enumerate()
        .map(|(bus, port)| {
            let p = &outputs[port];
            let name = if p.name.trim().is_empty() { format!("Output {}", bus + 1) } else { p.name.clone() };
            OutputBus { index: bus as u32, name, channels: p.channels as u32, active: bus == 0 }
        })
        .collect()
}

/// Where a side-chain key goes: the first input port past the main one, or, where that's mono and
/// so is the next (LSP's "sc_l" and "sc_r"), the key's left channel on one and its right on the
/// other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct KeyPorts {
    pub first: Option<usize>,
    pub right: Option<usize>,
}

pub(super) fn key_ports(inputs: &[PortInfo]) -> KeyPorts {
    let main = main_port(inputs);
    let aux = |i: &usize| Some(*i) != main && inputs[*i].channels > 0;
    let Some(first) = (0..inputs.len()).find(aux) else { return KeyPorts::default() };
    let right = Some(first + 1).filter(|i| *i < inputs.len() && aux(i) && inputs[first].channels == 1 && inputs[*i].channels == 1);
    KeyPorts { first: Some(first), right }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(channels: usize, main: bool) -> PortInfo {
        PortInfo { name: String::new(), channels, main }
    }

    #[test]
    fn the_main_port_is_the_flagged_one_or_the_first() {
        assert_eq!(main_port(&[port(2, false), port(2, true)]), Some(1));
        assert_eq!(main_port(&[port(2, false), port(1, false)]), Some(0));
        assert_eq!(main_port(&[]), None);
    }

    #[test]
    fn a_stereo_block_goes_onto_mono_and_wider_ports() {
        let (l, r) = ([1.0, 0.0], [0.0, 1.0]);
        let mut mono = Port::new(1, 4);
        mono.put_stereo(&l, &r, 2);
        assert_eq!(mono.stereo(2).unwrap(), (&[0.5, 0.5][..], &[0.5, 0.5][..]));
        let mut quad = Port::new(4, 4);
        quad.buffers[3][0] = 9.0;
        quad.put_stereo(&l, &r, 2);
        assert_eq!(quad.stereo(2).unwrap(), (&l[..], &r[..]));
        assert_eq!(quad.buffers[3][0], 0.0);
        assert!(Port::new(0, 4).stereo(2).is_none());
    }

    #[test]
    fn ports_past_the_main_ones_are_aux() {
        let buses = bus_layouts(&[port(2, true), port(2, false)], &[port(2, true)]);
        assert_eq!(buses.iter().map(|b| (b.input, b.aux)).collect::<Vec<_>>(), vec![(true, false), (true, true), (false, false)]);
    }

    #[test]
    fn the_main_output_is_bus_0_and_the_rest_follow_in_order() {
        let outs = [port(2, false), port(2, true), port(1, false)];
        assert_eq!(bus_order(&outs), [1, 0, 2]);
        let buses = output_buses(&outs);
        assert_eq!(buses.iter().map(|b| (b.index, b.channels, b.active)).collect::<Vec<_>>(), [(0, 2, true), (1, 2, false), (2, 1, false)]);
        assert_eq!(buses[2].name, "Output 3");
        assert!(output_buses(&[]).is_empty());
    }

    #[test]
    fn a_key_goes_to_the_first_aux_input_or_a_mono_pair() {
        assert_eq!(key_ports(&[port(2, true)]), KeyPorts::default());
        assert_eq!(key_ports(&[port(2, true), port(2, false)]), KeyPorts { first: Some(1), right: None });
        assert_eq!(key_ports(&[port(2, true), port(1, false), port(1, false)]), KeyPorts { first: Some(1), right: Some(2) });
        assert_eq!(key_ports(&[port(2, true), port(1, false), port(2, false)]), KeyPorts { first: Some(1), right: None });
        assert_eq!(key_ports(&[port(2, false), port(2, true)]), KeyPorts { first: Some(0), right: None }, "the main port may come second");
    }
}
