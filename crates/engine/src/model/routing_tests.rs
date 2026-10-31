//! Routing order and refusing feedback loops.

use super::*;

fn track(p: &mut Project, kind: TrackKind) -> Id {
    let id = p.alloc_id();
    p.tracks.push(Track {
        id,
        kind,
        name: format!("{id}"),
        color: String::new(),
        volume_db: 0.0,
        pan: 0.0,
        mute: false,
        solo: false,
        armed: false,
        monitor: false,
        input: InputSel::None,
        instrument: None,
        inserts: Vec::new(),
        regions: Vec::new(),
        eq: ChannelEq::default(),
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
        off: false,
    });
    id
}

fn send(bus: Id) -> Option<TrackSend> {
    Some(TrackSend { bus, level_db: 0.0, pre_fader: false, muted: false })
}

#[test]
fn receivers_follow_senders() {
    let mut p = Project::new("t", 48000);
    let aux2 = track(&mut p, TrackKind::Aux);
    let aux1 = track(&mut p, TrackKind::Aux);
    let sum = track(&mut p, TrackKind::Summing);
    let sub = track(&mut p, TrackKind::Audio);
    p.track_mut(sub).unwrap().parent = Some(sum);
    p.track_mut(sub).unwrap().sends[0] = send(aux1);
    p.track_mut(aux1).unwrap().output = Some(aux2);
    let order = p.process_order();
    let pos = |id| order.iter().position(|&i| i == id).unwrap();
    assert!(pos(sub) < pos(sum) && pos(sub) < pos(aux1) && pos(aux1) < pos(aux2));
    assert_eq!(order.len(), 4);
    assert!(p.reaches(sub, aux2) && !p.reaches(aux2, sub));
    assert_eq!(p.output_of(sub), Some(sum));
    p.track_mut(sub).unwrap().output = Some(MASTER_ID);
    assert_eq!(p.output_of(sub), None);
}

#[test]
fn prune_breaks_loops_and_dangling_routes() {
    let mut p = Project::new("t", 48000);
    let a = track(&mut p, TrackKind::Aux);
    let b = track(&mut p, TrackKind::Aux);
    let audio = track(&mut p, TrackKind::Audio);
    p.track_mut(a).unwrap().sends[0] = send(b);
    p.track_mut(b).unwrap().output = Some(a);
    p.track_mut(audio).unwrap().sends[1] = send(audio);
    p.track_mut(audio).unwrap().output = Some(999);
    p.track_mut(audio).unwrap().sends.truncate(2);
    p.prune_routes();
    assert!(!(p.reaches(a, b) && p.reaches(b, a)));
    let t = p.track(audio).unwrap();
    assert!(t.sends.iter().all(|s| s.is_none()) && t.output.is_none() && t.sends.len() == SEND_SLOTS);
    // Loaded loops never hang the ordering.
    assert_eq!(p.process_order().len(), 3);
}

fn keyed(p: &mut Project, track: Id, source: Id) {
    let slot = PluginSlot {
        uid: p.alloc_id(),
        plugin_id: "builtin:noise-gate".into(),
        name: "Noise Gate".into(),
        bypass: false,
        state: None,
        params: Vec::new(),
        sidechain: Some(source),
        sidechain_pre: false,
    };
    p.track_mut(track).unwrap().inserts = vec![Some(slot)];
}

#[test]
fn side_chain_sources_play_first_and_loops_are_cut() {
    let mut p = Project::new("t", 48000);
    let bass = track(&mut p, TrackKind::Audio);
    let kick = track(&mut p, TrackKind::Audio);
    keyed(&mut p, bass, kick);
    let order = p.process_order();
    let pos = |id| order.iter().position(|&i| i == id).unwrap();
    assert!(pos(kick) < pos(bass));
    assert!(p.depends(kick, bass) && !p.depends(bass, kick));
    // Audio reaching it doesn't make it a feed: soloing the kick doesn't solo the bass.
    assert!(!p.reaches(kick, bass));
    // Keying the kick from an aux the bass sends to would make a loop.
    let aux = track(&mut p, TrackKind::Aux);
    p.track_mut(bass).unwrap().sends[0] = send(aux);
    keyed(&mut p, kick, aux);
    p.prune_routes();
    let key = |id: Id| p.track(id).unwrap().inserts[0].as_ref().unwrap().sidechain;
    // One edge of the loop goes, and the ordering is sound again.
    assert!(key(kick).is_none() != key(bass).is_none());
    assert_eq!(p.process_order().len(), 3);
    // A side-chain from a track that's gone goes too.
    keyed(&mut p, aux, 999);
    p.prune_routes();
    assert_eq!(p.track(aux).unwrap().inserts[0].as_ref().unwrap().sidechain, None);
}
