//! The event lists `process` and `flush` take: input events from a preallocated list kept in time
//! order, and output events turned into gestures for the controller. Neither allocates.

use std::ffi::c_void;

use clap_sys::events::*;
use clap_sys::id::clap_id;
use plugin_api::Gesture;

use crate::params::ParamRanges;

/// One input event: a parameter value, a note or a MIDI message.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union Event {
    header: clap_event_header,
    param: clap_event_param_value,
    note: clap_event_note,
    midi: clap_event_midi,
}

fn header<T>(time: u32, type_: u16) -> clap_event_header {
    clap_event_header { size: std::mem::size_of::<T>() as u32, time, space_id: CLAP_CORE_EVENT_SPACE_ID, type_, flags: 0 }
}

impl Event {
    /// A parameter set to `value`, in the parameter's own units.
    pub fn param(time: u32, id: clap_id, value: f64) -> Event {
        Event {
            param: clap_event_param_value {
                header: header::<clap_event_param_value>(time, CLAP_EVENT_PARAM_VALUE),
                param_id: id,
                cookie: std::ptr::null_mut(),
                note_id: -1,
                port_index: -1,
                channel: -1,
                key: -1,
                value,
            },
        }
    }

    /// A note on or off (`CLAP_EVENT_NOTE_ON`, `CLAP_EVENT_NOTE_OFF`) on note port 0.
    pub fn note(time: u32, type_: u16, channel: i16, key: i16, velocity: f64) -> Event {
        let mut h = header::<clap_event_note>(time, type_);
        h.flags = CLAP_EVENT_IS_LIVE;
        Event { note: clap_event_note { header: h, note_id: -1, port_index: 0, channel, key, velocity } }
    }

    /// A MIDI 1.0 message on note port 0.
    pub fn midi(time: u32, data: [u8; 3]) -> Event {
        let mut h = header::<clap_event_midi>(time, CLAP_EVENT_MIDI);
        h.flags = CLAP_EVENT_IS_LIVE;
        Event { midi: clap_event_midi { header: h, port_index: 0, data } }
    }

    pub fn time(&self) -> u32 {
        unsafe { self.header.time }
    }
}

/// A `clap_input_events` over a list with room for `capacity` events.
pub(crate) struct InputEvents {
    list: Vec<Event>,
    raw: clap_input_events,
}

unsafe impl Send for InputEvents {}

impl InputEvents {
    pub fn new(capacity: usize) -> InputEvents {
        InputEvents { list: Vec::with_capacity(capacity), raw: clap_input_events { ctx: std::ptr::null_mut(), size: Some(size), get: Some(get) } }
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// Adds an event; past the list's room it is dropped.
    pub fn push(&mut self, e: Event) {
        if self.list.len() < self.list.capacity() {
            self.list.push(e);
        }
    }

    /// Puts the events in time order, as CLAP requires, keeping the order of events at the same
    /// time (a parameter's points, a note off before a note on). An insertion sort: stable and
    /// in place, and quick on lists that come almost sorted.
    pub fn sort(&mut self) {
        let l = &mut self.list;
        for i in 1..l.len() {
            let mut j = i;
            while j > 0 && l[j - 1].time() > l[j].time() {
                l.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    /// The list as the plug-in sees it, valid while `self` isn't moved or changed.
    pub fn raw(&mut self) -> *const clap_input_events {
        self.raw.ctx = self as *mut InputEvents as *mut c_void;
        &self.raw
    }
}

unsafe extern "C" fn size(list: *const clap_input_events) -> u32 {
    let events = &*((*list).ctx as *const InputEvents);
    events.list.len() as u32
}

unsafe extern "C" fn get(list: *const clap_input_events, index: u32) -> *const clap_event_header {
    let events = &*((*list).ctx as *const InputEvents);
    match events.list.get(index as usize) {
        Some(e) => &e.header,
        None => std::ptr::null(),
    }
}

/// A `clap_output_events` that hands the plug-in's parameter gestures and changes to the
/// controller as `Gesture`s, normalized. Other events (notes it plays) are let go.
pub(crate) struct OutputEvents {
    raw: clap_output_events,
    gestures: rtrb::Producer<Gesture>,
    pub ranges: ParamRanges,
}

unsafe impl Send for OutputEvents {}

impl OutputEvents {
    pub fn new(gestures: rtrb::Producer<Gesture>, ranges: ParamRanges) -> OutputEvents {
        OutputEvents { raw: clap_output_events { ctx: std::ptr::null_mut(), try_push: Some(try_push) }, gestures, ranges }
    }

    /// The list as the plug-in sees it, valid while `self` isn't moved.
    pub fn raw(&mut self) -> *const clap_output_events {
        self.raw.ctx = self as *mut OutputEvents as *mut c_void;
        &self.raw
    }

    fn take(&mut self, e: &clap_event_header) {
        if e.space_id != CLAP_CORE_EVENT_SPACE_ID {
            return;
        }
        let g = unsafe {
            match e.type_ {
                CLAP_EVENT_PARAM_GESTURE_BEGIN => Gesture::Begin((*(e as *const _ as *const clap_event_param_gesture)).param_id),
                CLAP_EVENT_PARAM_GESTURE_END => Gesture::End((*(e as *const _ as *const clap_event_param_gesture)).param_id),
                CLAP_EVENT_PARAM_VALUE => {
                    let p = &*(e as *const _ as *const clap_event_param_value);
                    // Meters and other read-only values aren't edits.
                    match self.ranges.get(p.param_id).filter(|r| !r.readonly) {
                        Some(r) => Gesture::Perform(p.param_id, r.to_normalized(p.value)),
                        None => return,
                    }
                }
                _ => return,
            }
        };
        let _ = self.gestures.push(g);
    }
}

unsafe extern "C" fn try_push(list: *const clap_output_events, event: *const clap_event_header) -> bool {
    if let (Some(out), Some(e)) = (((*list).ctx as *mut OutputEvents).as_mut(), event.as_ref()) {
        out.take(e);
    }
    // Taken, or let go on purpose: the plug-in needn't try again.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Range;

    #[test]
    fn input_events_come_in_time_order_and_keep_ties() {
        let mut ev = InputEvents::new(4);
        ev.push(Event::param(10, 1, 0.5));
        ev.push(Event::note(3, CLAP_EVENT_NOTE_OFF, 0, 60, 0.0));
        ev.push(Event::note(3, CLAP_EVENT_NOTE_ON, 0, 60, 1.0));
        ev.push(Event::midi(0, [0xB0, 1, 2]));
        ev.push(Event::midi(0, [0xB0, 1, 3]));
        let raw = ev.raw();
        unsafe {
            assert_eq!((*raw).size.unwrap()(raw), 4, "past its room an event is dropped");
            let at = |i| &*(*raw).get.unwrap()(raw, i);
            let types: Vec<(u32, u16)> = (0..4).map(|i| (at(i).time, at(i).type_)).collect();
            assert_eq!(types, vec![(10, CLAP_EVENT_PARAM_VALUE), (3, CLAP_EVENT_NOTE_OFF), (3, CLAP_EVENT_NOTE_ON), (0, CLAP_EVENT_MIDI)]);
        }
        ev.sort();
        let raw = ev.raw();
        unsafe {
            let at = |i| &*(*raw).get.unwrap()(raw, i);
            let types: Vec<(u32, u16)> = (0..4).map(|i| (at(i).time, at(i).type_)).collect();
            assert_eq!(types, vec![(0, CLAP_EVENT_MIDI), (3, CLAP_EVENT_NOTE_OFF), (3, CLAP_EVENT_NOTE_ON), (10, CLAP_EVENT_PARAM_VALUE)]);
            assert!((*raw).get.unwrap()(raw, 4).is_null());
        }
    }

    #[test]
    fn output_events_become_normalized_gestures() {
        let (tx, mut rx) = rtrb::RingBuffer::new(8);
        let ranges = ParamRanges::new(vec![
            (7, Range { min: -10.0, max: 10.0, stepped: false, readonly: false }),
            (8, Range { min: 0.0, max: 1.0, stepped: false, readonly: true }),
        ]);
        let mut out = OutputEvents::new(tx, ranges);
        let raw = out.raw();
        let begin = clap_event_param_gesture { header: header::<clap_event_param_gesture>(0, CLAP_EVENT_PARAM_GESTURE_BEGIN), param_id: 7 };
        let value = unsafe { Event::param(0, 7, 5.0).param };
        let meter = unsafe { Event::param(0, 8, 0.3).param };
        let end = clap_event_param_gesture { header: header::<clap_event_param_gesture>(0, CLAP_EVENT_PARAM_GESTURE_END), param_id: 7 };
        unsafe {
            let push = (*raw).try_push.unwrap();
            assert!(push(raw, &begin.header));
            assert!(push(raw, &value.header));
            assert!(push(raw, &meter.header));
            assert!(push(raw, &end.header));
        }
        let got: Vec<Gesture> = std::iter::from_fn(|| rx.pop().ok()).collect();
        assert_eq!(got, vec![Gesture::Begin(7), Gesture::Perform(7, 0.75), Gesture::End(7)]);
    }
}
