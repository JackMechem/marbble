//! Instantiating a VST3 plug-in: loading its module, creating and connecting the component and
//! controller, setting up buses and returning the two halves ([`PluginProcessor`] and
//! [`PluginController`]) that then go to their own threads.

use super::*;
use crate::module::Module;

pub(super) fn channel_count(arr: SpeakerArrangement) -> usize {
    arr.count_ones() as usize
}

/// The first aux input after the main one, which is where a plug-in takes its side-chain.
unsafe fn find_aux_input(component: &ComPtr<IComponent>, n_in: usize) -> Option<usize> {
    (1..n_in).find(|&i| {
        let mut info: BusInfo = std::mem::zeroed();
        component.getBusInfo(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32, i as i32, &mut info) == kResultOk
            && info.busType == BusTypes_::kAux as BusType
    })
}

/// A plug-in without a side-chain key: see `instantiate_keyed`.
pub fn instantiate(desc: &PluginDescriptor, sample_rate: f64, max_block: usize) -> Result<(PluginProcessor, PluginController), String> {
    instantiate_keyed(desc, sample_rate, max_block, false)
}

/// Room in each ring between the plug-in's two halves: editor edits in, parameter values out,
/// and the editor's gestures.
const RING: usize = 4096;

/// Loads a plug-in, with its aux input active (`keyed`) or not. Without a key the aux bus stays
/// inactive, whatever the plug-in's default, so the plug-in uses its internal key.
pub fn instantiate_keyed(desc: &PluginDescriptor, sample_rate: f64, max_block: usize, keyed: bool) -> Result<(PluginProcessor, PluginController), String> {
    let module = load_module(&desc.path)?;
    let cid = unhex(&desc.cid).ok_or("bad class id")?;
    unsafe {
        let host = host_context();
        let component: ComPtr<IComponent> = module.create::<IComponent>(&cid).ok_or("factory could not create the component")?;
        if component.initialize(host.as_ptr()) != kResultOk {
            return Err("component failed to initialize".into());
        }
        let (controller, separate_controller) = create_controller(&module, &component, &host);
        let connections = if separate_controller { connect(&component, controller.as_ref()) } else { None };

        let (edits_tx, edits_rx) = rtrb::RingBuffer::new(RING);
        let (out_tx, out_rx) = rtrb::RingBuffer::new(RING);
        let (gestures_tx, gestures_rx) = rtrb::RingBuffer::new(RING);
        let handler = ComWrapper::new(ComponentHandler {
            edits: parking_lot::Mutex::new(edits_tx),
            restart_flags: Default::default(),
            touched: Default::default(),
            gestures: parking_lot::Mutex::new(gestures_tx),
        });

        if let Some(c) = &controller {
            // Sync the controller with the component's initial state.
            let s = MemoryStream::new(Vec::new());
            if component.getState(as_stream(&s).as_ptr()) == kResultOk {
                s.rewind();
                c.setComponentState(as_stream(&s).as_ptr());
            }
            c.setComponentHandler(handler.to_com_ptr::<IComponentHandler>().unwrap().as_ptr());
        }

        let processor: ComPtr<IAudioProcessor> = component.cast::<IAudioProcessor>().ok_or("plugin has no audio processor")?;

        let n_in = component.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32).max(0) as usize;
        let n_out = component.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32).max(0) as usize;
        let key_bus = arrange_buses(desc, &component, &processor, n_in, n_out);
        let inputs: Vec<Bus> = (0..n_in).map(|i| Bus::new(channel_count(arrangement(&processor, BusDirections_::kInput as i32, i)), max_block)).collect();
        let outputs: Vec<Bus> = (0..n_out).map(|i| Bus::new(channel_count(arrangement(&processor, BusDirections_::kOutput as i32, i)), max_block)).collect();
        let key_bus = activate_buses(desc, &component, n_in, n_out, keyed, key_bus);
        let output_buses = multi_out::read_outputs(&component, &outputs);
        let n_ev_in = component.getBusCount(MediaTypes_::kEvent as i32, BusDirections_::kInput as i32);
        if n_ev_in > 0 {
            component.activateBus(MediaTypes_::kEvent as i32, BusDirections_::kInput as i32, 0, 1);
        }
        start_processing(desc, &component, &processor, sample_rate, max_block);
        let midi_map = midi_mapping(controller.as_ref());

        let shared = Arc::new(Shared {
            instance: INSTANCES.fetch_add(1, Ordering::AcqRel) + 1,
            component,
            processor: Some(processor.clone()),
            controller,
            separate_controller,
            connections,
            _handler: handler,
            parked: AtomicBool::new(false),
            out_gate: OutGate::default(),
        });

        let blank = AudioBusBuffers { numChannels: 0, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: std::ptr::null_mut() } };
        let proc_half = PluginProcessor {
            processor: processor.as_ptr(),
            _shared: shared.clone(),
            in_abb: vec![blank; inputs.len()],
            out_abb: vec![blank; outputs.len()],
            inputs,
            outputs,
            param_in: ParameterChanges::new(),
            param_out: ParameterChanges::new(),
            events_in: EventList::new(),
            events_out: EventList::new(),
            edits_rx,
            out_tx,
            midi_map,
            max_block,
            is_instrument: desc.is_instrument || n_in == 0,
            queued: Vec::with_capacity(MAX_QUEUED),
            key_bus,
            gate: Gate::Open,
            gate_frames: (GATE_FADE_SECS * sample_rate).max(1.0) as u32,
            dry_l: vec![0.0; max_block],
            dry_r: vec![0.0; max_block],
            out_fade: multi_out::OutFade::new(sample_rate),
            edit_l: vec![0.0; EDIT_BLOCK.min(max_block)],
            edit_r: vec![0.0; EDIT_BLOCK.min(max_block)],
        };
        let ctrl_half = PluginController {
            desc: desc.clone(),
            shared,
            out_rx,
            editor: None,
            gestures_rx,
            latency_changed: false,
            key_bus,
            key_active: keyed && key_bus.is_some(),
            outputs: output_buses,
            params_restarted: false,
            host_changes: Default::default(),
            host_set: Default::default(),
            unresponsive: false,
            last_latency: Default::default(),
        };
        Ok((proc_half, ctrl_half))
    }
}

/// The plug-in's edit controller, and whether it is an object of its own (created and
/// initialized here) rather than the component itself.
unsafe fn create_controller(module: &Module, component: &ComPtr<IComponent>, host: &ComPtr<FUnknown>) -> (Option<ComPtr<IEditController>>, bool) {
    // Single-component plugins implement the controller on the same object.
    if let Some(c) = component.cast::<IEditController>() {
        return (Some(c), false);
    }
    let mut ccid: TUID = [0; 16];
    if component.getControllerClassId(&mut ccid) != kResultOk {
        return (None, false);
    }
    let ccid: [u8; 16] = std::mem::transmute(ccid);
    match module.create::<IEditController>(&ccid) {
        Some(c) if c.initialize(host.as_ptr()) == kResultOk => (Some(c), true),
        _ => (None, false),
    }
}

/// Connects a separate controller and its component to each other, so they can exchange messages.
unsafe fn connect(
    component: &ComPtr<IComponent>,
    controller: Option<&ComPtr<IEditController>>,
) -> Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)> {
    let (Some(a), Some(b)) = (component.cast::<IConnectionPoint>(), controller.and_then(|c| c.cast::<IConnectionPoint>())) else { return None };
    a.connect(b.as_ptr());
    b.connect(a.as_ptr());
    Some((a, b))
}

/// The speaker arrangement bus `i` in direction `dir` has now.
unsafe fn arrangement(processor: &ComPtr<IAudioProcessor>, dir: i32, i: usize) -> SpeakerArrangement {
    let mut arr: SpeakerArrangement = 0;
    processor.getBusArrangement(dir, i as i32, &mut arr);
    arr
}

/// Asks for stereo on the main buses, keeping whatever the plug-in wants on the others, and
/// sets up the side-chain input. Returns the side-chain bus, if it has one with channels.
unsafe fn arrange_buses(
    desc: &PluginDescriptor,
    component: &ComPtr<IComponent>,
    processor: &ComPtr<IAudioProcessor>,
    n_in: usize,
    n_out: usize,
) -> Option<usize> {
    let current = |dir: i32, i: usize| arrangement(processor, dir, i);
    let mut in_arr: Vec<SpeakerArrangement> = (0..n_in).map(|i| current(BusDirections_::kInput as i32, i)).collect();
    let mut out_arr: Vec<SpeakerArrangement> = (0..n_out).map(|i| current(BusDirections_::kOutput as i32, i)).collect();
    if let Some(a) = in_arr.first_mut() {
        *a = SpeakerArr::kStereo;
    }
    if let Some(a) = out_arr.first_mut() {
        *a = SpeakerArr::kStereo;
    }
    // The side-chain: stereo if the plug-in takes it, else mono, else what it has. A plug-in
    // that refuses a layout may have adapted to something else, so what it reads back as is
    // what the buffers are sized for.
    let mut key_bus = find_aux_input(component, n_in);
    let mut accepted = false;
    let keep = key_bus.map(|k| in_arr[k]);
    for arr in [SpeakerArr::kStereo, SpeakerArr::kMono].into_iter().filter(|_| key_bus.is_some()) {
        in_arr[key_bus.unwrap()] = arr;
        accepted = processor.setBusArrangements(in_arr.as_mut_ptr(), n_in as i32, out_arr.as_mut_ptr(), n_out as i32) == kResultOk
            && current(BusDirections_::kInput as i32, key_bus.unwrap()) == arr;
        if accepted {
            break;
        }
    }
    if !accepted {
        if let (Some(k), Some(arr)) = (key_bus, keep) {
            in_arr[k] = arr;
        }
        if processor.setBusArrangements(in_arr.as_mut_ptr(), n_in as i32, out_arr.as_mut_ptr(), n_out as i32) != kResultOk {
            log::warn!("{}: setBusArrangements refused stereo main buses", desc.name);
        }
    }
    if key_bus.is_some_and(|k| channel_count(current(BusDirections_::kInput as i32, k)) == 0) {
        key_bus = None;
    }
    key_bus
}

/// Activates the main audio buses, and the side-chain `key_bus` when `keyed`; every other input
/// stays off. Returns the side-chain, or None if it was wanted and didn't activate.
unsafe fn activate_buses(
    desc: &PluginDescriptor,
    component: &ComPtr<IComponent>,
    n_in: usize,
    n_out: usize,
    keyed: bool,
    mut key_bus: Option<usize>,
) -> Option<usize> {
    if n_in > 0 {
        component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32, 0, 1);
    }
    if n_out > 0 {
        component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32, 0, 1);
    }
    for i in 1..n_in {
        let on = keyed && key_bus == Some(i);
        let r = component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32, i as i32, on as TBool);
        if on && r != kResultOk {
            log::warn!("{}: the side-chain bus didn't activate", desc.name);
            key_bus = None;
        }
    }
    key_bus
}

/// Tells the plug-in the sample rate and block size and switches it on.
unsafe fn start_processing(desc: &PluginDescriptor, component: &ComPtr<IComponent>, processor: &ComPtr<IAudioProcessor>, sample_rate: f64, max_block: usize) {
    let mut setup = ProcessSetup {
        processMode: ProcessModes_::kRealtime as i32,
        symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
        maxSamplesPerBlock: max_block as i32,
        sampleRate: sample_rate,
    };
    if processor.setupProcessing(&mut setup) != kResultOk {
        log::warn!("{}: setupProcessing failed", desc.name);
    }
    if component.setActive(1) != kResultOk {
        log::warn!("{}: setActive failed", desc.name);
    }
}

/// The parameter each MIDI controller is mapped to (`IMidiMapping`, channel 1): the 128
/// controller numbers, then channel pressure and pitch bend (`ControllerNumbers_`).
unsafe fn midi_mapping(controller: Option<&ComPtr<IEditController>>) -> [Option<ParamID>; 130] {
    let mut midi_map = [None; 130];
    if let Some(mm) = controller.and_then(|c| c.cast::<IMidiMapping>()) {
        for (cc, slot) in midi_map.iter_mut().enumerate() {
            let mut id: ParamID = 0;
            if mm.getMidiControllerAssignment(0, 0, cc as CtrlNumber, &mut id) == kResultOk {
                *slot = Some(id);
            }
        }
    }
    midi_map
}
