//! The task graph: dependencies, refusals, width, and which routes become edges.

use super::room::EDGES_PER_TASK;
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
        frozen_output: None,
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

/// A graph refilled in place is the one `TaskGraph::new` builds from the same edges, repeated
/// edges and all, and the room never grows: the audio thread does this.
#[test]
fn a_graph_refilled_in_place_is_the_one_built_whole() {
    let mut room = GraphRoom::default();
    let caps = |r: &GraphRoom| (r.graph.ids.capacity(), r.graph.deps.capacity(), r.graph.succs.capacity(), r.graph.dep_at.capacity(), r.graph.roots.capacity());
    let before = caps(&room);
    let mut seed = 7u64;
    let mut rand = move |m: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % m
    };
    for round in 0..=200 {
        let n = match round {
            0 => 1 + rand(MAX_TASKS),
            200 => MAX_TASKS,
            _ => 1 + rand(60),
        };
        // Each strip's edges, as the engine's routes give them: forwards, sometimes twice. The
        // last round fills the room.
        let edges: Vec<Vec<(usize, usize)>> = if round == 200 {
            full(n)
        } else {
            (0..n)
                .map(|i| {
                    (0..rand(5))
                        .filter_map(|_| match rand(2) {
                            0 if i + 1 < n => Some((i, i + 1 + rand(n - i - 1))),
                            1 if i > 0 => Some((rand(i), i)),
                            _ => None,
                        })
                        .collect()
                })
                .collect()
        };
        let ids: Vec<Id> = (0..n).map(|i| 1000 + i as Id * 3).collect();
        let heavy = |i: usize| i % 3 != 1;
        assert!(room.refill(n, |i| ids[i], |i, add| edges[i].iter().for_each(|&(a, b)| add(a, b)), heavy));
        let whole: Vec<(Id, Id)> = edges.iter().flatten().map(|&(a, b)| (ids[a], ids[b])).collect();
        let built = TaskGraph::new(ids.clone(), &whole, |id| heavy(((id - 1000) / 3) as usize)).unwrap();
        assert_eq!(room.graph(), &built, "round {round}, {n} tasks");
    }
    assert_eq!(caps(&room), before, "nothing was allocated");
}

/// Every one of `n` strips making as many edges as a strip can, with itself at one end and some
/// repeated: as many as the room holds.
fn full(n: usize) -> Vec<Vec<(usize, usize)>> {
    (0..n)
        .map(|i| {
            (0..EDGES_PER_TASK)
                .map(|k| {
                    let j = (i * 7 + k * 13 + 1) % n;
                    let j = if j == i { (i + 1) % n } else { j };
                    (i.min(j), i.max(j))
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_graph_that_cannot_be_refilled_says_so() {
    let mut room = GraphRoom::default();
    // Each strip makes the one edge here, which is the same edge twice over.
    assert!(!room.refill(2, |i| i as Id, |_, add| add(1, 0), |_| true), "backwards");
    assert!(!room.refill(2, |i| i as Id, |_, add| add(0, 5), |_| true), "past the end");
    assert!(!room.refill(MAX_TASKS + 1, |i| i as Id, |_, _| {}, |_| true), "too many");
    let mut over = full(MAX_TASKS);
    over[0].push((0, 1));
    assert!(!room.refill(MAX_TASKS, |i| i as Id, |i, add| over[i].iter().for_each(|&(a, b)| add(a, b)), |_| true), "more edges than it has room for");
    assert!(room.refill(2, |i| i as Id, |_, add| add(0, 1), |_| true));
    assert_eq!(room.graph().deps(1), [0]);
}
