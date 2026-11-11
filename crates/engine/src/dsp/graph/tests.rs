//! The task graph: dependencies, refusals, width, and which routes become edges.

use super::*;
use crate::model::{AutoMode, InputSel, InstrumentOutput, PluginSlot, Track, TrackKind, TrackSend, INSERT_SLOTS, SEND_SLOTS};

fn graph(ids: &[Id], edges: &[(Id, Id)]) -> TaskGraph {
    TaskGraph::new(ids.to_vec(), edges, |_| true).unwrap()
}

#[test]
fn dependencies_and_successors_are_sorted_and_unique() {
    let g = graph(&[10, 20, 30, 40], &[(30, 40), (10, 40), (10, 40), (20, 40), (10, 30)]);
    assert_eq!(g.deps(3), [0, 1, 2]);
    assert_eq!(g.succs(0), [2, 3]);
    assert_eq!(g.roots(), [0, 1]);
    assert!(g.has_dep(3, 1) && !g.has_dep(2, 1));
    assert_eq!(g.id(2), 30);
}

#[test]
fn cycles_cannot_be_built() {
    let e = TaskGraph::new(vec![1, 2, 3], &[(1, 2), (2, 3), (3, 1)], |_| false);
    assert_eq!(e.unwrap_err(), GraphError::Backwards { from: 3, to: 1 });
    assert_eq!(TaskGraph::new(vec![1, 2], &[(2, 2)], |_| false).unwrap_err(), GraphError::Backwards { from: 2, to: 2 });
}

#[test]
fn unknown_tracks_and_repeats_are_refused() {
    assert_eq!(TaskGraph::new(vec![1], &[(1, 9)], |_| false).unwrap_err(), GraphError::Unknown(9));
    assert_eq!(TaskGraph::new(vec![1, 1], &[], |_| false).unwrap_err(), GraphError::Unknown(1));
    assert!(matches!(TaskGraph::new((0..MAX_TASKS as Id + 1).collect(), &[], |_| false), Err(GraphError::TooBig(_))));
}

#[test]
fn only_strips_with_plug_ins_at_one_depth_count_as_width() {
    let g = TaskGraph::new(vec![1, 2, 3, 4], &[(1, 3), (2, 3), (3, 4)], |id| id != 2).unwrap();
    assert_eq!(g.heavy_width(), 1, "1, 3 and 4 run one after the other");
    let g = TaskGraph::new(vec![1, 2, 3, 4], &[(1, 3), (2, 3), (3, 4)], |_| true).unwrap();
    assert_eq!(g.heavy_width(), 2);
}

fn track(id: Id, kind: TrackKind) -> Track {
    Track {
        id,
        kind,
        name: format!("t{id}"),
        color: String::new(),
        volume_db: 0.0,
        pan: 0.0,
        mute: false,
        solo: false,
        armed: false,
        monitor: false,
        input: InputSel::None,
        instrument: None,
        inserts: vec![None; INSERT_SLOTS],
        regions: Vec::new(),
        legacy_eq: None,
        parent: None,
        collapsed: false,
        group: None,
        sends: vec![None; SEND_SLOTS],
        output: None,
        bus: 0,
        hidden: false,
        automation: Vec::new(),
        auto_mode: AutoMode::Read,
        frozen: None,
        vca: None,
        alternatives: Vec::new(),
        alternative: String::new(),
        auto_lanes: Vec::new(),
        instrument_output: None,
        legacy_eq_preset: None,
        off: false,
        icon: None,
        midi_settings: Default::default(),
    }
}

fn project(tracks: Vec<Track>) -> Project {
    let mut p = Project::new("graph", 48000);
    p.tracks = tracks;
    p
}

fn insert(uid: Id, sidechain: Option<Id>) -> PluginSlot {
    PluginSlot {
        uid,
        plugin_id: "builtin:gain".into(),
        name: "Gain".into(),
        bypass: false,
        state: None,
        params: Vec::new(),
        sidechain,
        sidechain_pre: false,
        preset: None,
    }
}

#[test]
fn every_kind_of_route_is_an_edge() {
    let mut send = track(1, TrackKind::Audio);
    send.sends[0] = Some(TrackSend { bus: 5, level_db: 0.0, pre_fader: false, muted: false });
    let mut sub = track(2, TrackKind::Audio);
    sub.parent = Some(3);
    let stack = track(3, TrackKind::Summing);
    let mut out = track(4, TrackKind::Audio);
    out.output = Some(6);
    let mut aux = track(5, TrackKind::Aux);
    aux.bus = 1;
    let mut out_aux = track(6, TrackKind::Aux);
    out_aux.bus = 2;
    let mut keyed = track(7, TrackKind::Audio);
    keyed.inserts[0] = Some(insert(70, Some(4)));
    let inst = track(8, TrackKind::Instrument);
    let mut multi = track(9, TrackKind::Aux);
    multi.bus = 3;
    multi.instrument_output = Some(InstrumentOutput { track: 8, bus: 1 });
    let p = project(vec![send, sub, stack, out, aux, out_aux, keyed, inst, multi]);
    let g = TaskGraph::of_project(&p).unwrap();
    let at = |id: Id| (0..g.len()).find(|&i| g.id(i) == id).unwrap();
    for (from, to, what) in [(1, 5, "send"), (2, 3, "summing stack"), (4, 6, "output"), (4, 7, "side-chain key"), (8, 9, "instrument output")] {
        assert!(g.has_dep(at(to), at(from)), "{what}: {from} before {to}");
    }
    assert!(!g.has_dep(at(5), at(4)), "no route, no edge");
}

#[test]
fn folder_subtracks_stay_independent() {
    let folder = track(1, TrackKind::Folder);
    let mut a = track(2, TrackKind::Audio);
    a.parent = Some(1);
    let mut b = track(3, TrackKind::Audio);
    b.parent = Some(1);
    let g = TaskGraph::of_project(&project(vec![folder, a, b])).unwrap();
    let at = |id: Id| (0..g.len()).find(|&i| g.id(i) == id).unwrap();
    assert!(g.deps(at(2)).is_empty() && g.deps(at(3)).is_empty(), "they read the folder's lanes before the strips start");
}

#[test]
fn frozen_and_off_tracks_are_not_heavy() {
    let mut a = track(1, TrackKind::Audio);
    a.inserts[0] = Some(insert(10, None));
    let mut b = track(2, TrackKind::Audio);
    b.inserts[0] = Some(insert(20, None));
    let g = TaskGraph::of_project(&project(vec![a.clone(), b.clone()])).unwrap();
    assert_eq!(g.heavy_width(), 2);
    b.off = true;
    assert_eq!(TaskGraph::of_project(&project(vec![a.clone(), b.clone()])).unwrap().heavy_width(), 1);
}
