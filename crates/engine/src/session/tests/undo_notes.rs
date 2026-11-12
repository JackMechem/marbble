//! Undo steps of MIDI edits: each step shares the notes and controllers of the regions its edit
//! left alone (copy-on-write, so a note-heavy project isn't copied per step), and undo and redo
//! still put back exactly what was there.

use std::sync::Arc;

use crate::model::{CcKind, Project};
use crate::session::*;

use super::scratch_session;

const BAR: i64 = 96_000;

/// A one-bar MIDI region at `start` on `track`, holding `count` notes and a few controllers.
fn region_with(s: &mut Session, track: Id, start: i64, count: i64) -> Id {
    let id = s.create_midi_region(track, start, BAR).unwrap();
    let notes = (0..count).map(|i| Note { start: i * BAR / count + 7, length: 900, pitch: (48 + i % 24) as u8, velocity: 90, muted: false }).collect();
    let cc = (0..8).map(|i| CcEvent { start: i * BAR / 8, kind: CcKind::Controller(1), value: (i * 16) as u16 }).collect();
    s.set_midi_content(vec![MidiContent { region: id, notes, cc, start: None, length: None }]);
    id
}

fn notes_in(p: &Project, region: Id) -> &Arc<Vec<Note>> {
    let r = p.tracks.iter().flat_map(|t| &t.regions).find(|r| r.id == region).unwrap();
    let RegionContent::Midi { notes, .. } = &r.content else { panic!("not MIDI") };
    notes
}

/// What the tracks hold, as they're saved.
fn saved(s: &Session) -> String {
    serde_json::to_string(&s.project.tracks).unwrap()
}

#[test]
fn an_edit_copies_only_the_notes_it_changes() {
    let (mut s, _scratch) = scratch_session("undo-notes-shared");
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let (a, b) = (region_with(&mut s, t, 0, 64), region_with(&mut s, t, BAR, 64));
    let b_notes = notes_in(&s.project, b).clone();
    for i in 0..20 {
        s.checkpoint("Transpose", None, false);
        s.transpose_regions(&[a], if i % 2 == 0 { 1 } else { -1 });
        let step = &s.undo_stack.last().unwrap().project;
        assert!(!Arc::ptr_eq(notes_in(step, a), notes_in(&s.project, a)), "the edited region has its own copy");
    }
    assert_eq!(Arc::strong_count(&b_notes), 1 + 20 + 1, "the project, each step and this one share the other region's");
    s.undo();
    assert!(Arc::ptr_eq(notes_in(&s.project, b), &b_notes), "and an undo puts back the same one");
    assert_eq!(notes_in(&s.project, a)[0].pitch, 49);
}

#[test]
fn undo_and_redo_put_midi_edits_back_exactly() {
    let (mut s, _scratch) = scratch_session("undo-notes-exact");
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let u = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let (a, b) = (region_with(&mut s, t, 0, 40), region_with(&mut s, u, BAR, 40));
    let mut states = vec![saved(&s)];
    let mut step = |s: &mut Session, label: &str, edit: &dyn Fn(&mut Session)| {
        s.checkpoint(label, None, false);
        edit(s);
        states.push(saved(s));
    };
    step(&mut s, "Edit Notes", &|s| {
        let notes = vec![Note { start: 10, length: 500, pitch: 60, velocity: 100, muted: false }];
        s.set_midi_content(vec![MidiContent { region: a, notes, cc: Vec::new(), start: None, length: Some(2 * BAR) }]);
    });
    step(&mut s, "Transpose", &|s| s.transpose_regions(&[b], 5));
    step(&mut s, "Quantize", &|s| s.quantize_regions(&[b], BAR / 16));
    step(&mut s, "Split", &|s| s.split_at(BAR + BAR / 2, &[b]));
    step(&mut s, "Trim", &|s| s.resize_region(b, BAR + BAR / 4, BAR / 4));
    step(&mut s, "Tempo", &|s| s.set_tempo_at(0, 90.0, true));
    step(&mut s, "Loop", &|s| s.set_region_loop(a, Some(4 * BAR)));
    step(&mut s, "Real Copies", &|s| {
        s.convert_loops(&[a]);
    });
    assert!(states.windows(2).all(|w| w[0] != w[1]), "every edit changed something");
    let edits = states.len() - 1;
    for i in (0..edits).rev() {
        assert!(s.undo().is_some());
        assert_eq!(saved(&s), states[i], "undo to step {i}");
    }
    for (i, want) in states.iter().enumerate().skip(1) {
        assert!(s.redo().is_some());
        assert_eq!(&saved(&s), want, "redo to step {i}");
    }
}
