//! Take folders on grouped tracks: switching takes and comping together (`session/takes/`).

use crate::audio_io::NULL_OUTPUT;
use crate::session::*;

const LEN: i64 = 48_000;

/// A session with two grouped audio tracks (an overhead and a kick mic) and a third outside the
/// group.
fn session() -> (Session, [Id; 3], Id) {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let t: Vec<Id> = (0..3).map(|_| s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()).collect();
    let g = s.create_group(&t[..2]);
    (s, [t[0], t[1], t[2]], g)
}

/// A take folder at `start` on `track` with a take per entry of `passes` (each a recording's
/// pass, or None for a packed take), every take covering the whole folder. The newest is heard.
fn folder(s: &mut Session, track: Id, start: i64, passes: &[Option<(Id, u32)>]) -> (Id, Vec<Id>) {
    let mut f = TakeFolder::new();
    for (k, &pass) in passes.iter().enumerate() {
        let id = s.project.alloc_id();
        f.takes.push(Take {
            id,
            name: format!("Take {}", k + 1),
            clip: 0,
            offset: 0,
            start: 0,
            end: LEN,
            gain_db: 0.0,
            notes: Vec::new(),
            cc: Vec::new(),
            pass,
            ..Default::default()
        });
    }
    f.default_comp();
    let takes = f.takes.iter().map(|t| t.id).collect();
    let id = s.project.alloc_id();
    let content = RegionContent::Audio { clip: 0, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() };
    s.project.track_mut(track).unwrap().regions.push(Region {
        id,
        name: "Drums".into(),
        start,
        length: LEN,
        muted: false,
        color: None,
        content,
        loop_end: None,
        takes: Some(f),
        params: Default::default(),
        pattern: None,
    });
    (id, takes)
}

fn comp(s: &Session, region: Id) -> Vec<(i64, i64, Id)> {
    s.region(region).unwrap().takes.as_ref().unwrap().comp.iter().map(|c| (c.start, c.end, c.take)).collect()
}

fn comps(s: &Session, region: Id) -> (Vec<String>, usize) {
    let f = s.region(region).unwrap().takes.as_ref().unwrap();
    (f.comp_names(), f.current)
}

#[test]
fn grouped_folders_switch_takes_by_pass() {
    let (mut s, [oh, kick, other], _) = session();
    let (r1, r2) = (s.project.alloc_id(), s.project.alloc_id());
    // The kick mic had a recording of its own first, so its folder has one take more.
    let (a, at) = folder(&mut s, oh, 0, &[Some((r1, 0)), Some((r1, 1)), Some((r1, 2))]);
    let (b, bt) = folder(&mut s, kick, 0, &[Some((r2, 0)), Some((r1, 0)), Some((r1, 1)), Some((r1, 2))]);
    let (c, ct) = folder(&mut s, other, 0, &[Some((r1, 0)), Some((r1, 1)), Some((r1, 2))]);
    s.checkpoint("Select Take", None, false);
    s.select_take(a, at[0]);
    assert_eq!(comp(&s, a), vec![(0, LEN, at[0])]);
    assert_eq!(comp(&s, b), vec![(0, LEN, bt[1])], "the same pass, not the same position");
    assert_eq!(comp(&s, c), vec![(0, LEN, ct[2])], "a track outside the group stays");
    // One undo step puts both back.
    s.undo();
    assert_eq!(comp(&s, a), vec![(0, LEN, at[2])]);
    assert_eq!(comp(&s, b), vec![(0, LEN, bt[3])]);
    // A take the other folder has no pass for leaves it alone.
    s.select_take(b, bt[0]);
    assert_eq!(comp(&s, b), vec![(0, LEN, bt[0])]);
    assert_eq!(comp(&s, a), vec![(0, LEN, at[2])]);
}

#[test]
fn grouped_folders_comp_together() {
    let (mut s, [oh, kick, _], g) = session();
    // Packed by hand: matched by position.
    let (a, at) = folder(&mut s, oh, 0, &[None, None]);
    let (b, bt) = folder(&mut s, kick, 0, &[None, None]);
    s.set_comp_range(a, Some(at[0]), 1000, 2000);
    assert_eq!(comp(&s, a), vec![(0, 1000, at[1]), (1000, 2000, at[0]), (2000, LEN, at[1])]);
    assert_eq!(comp(&s, b), vec![(0, 1000, bt[1]), (1000, 2000, bt[0]), (2000, LEN, bt[1])]);
    // Silencing a stretch follows too.
    s.set_comp_range(b, None, 3000, 4000);
    assert_eq!(comp(&s, a), vec![(0, 1000, at[1]), (1000, 2000, at[0]), (2000, 3000, at[1]), (4000, LEN, at[1])]);
    // The comp list follows too: create, duplicate, switch, rename, delete.
    s.create_comp(a);
    s.duplicate_comp(b);
    assert_eq!(comps(&s, a), (vec!["Comp 1".into(), "Comp 2".into(), "Comp 3".into()], 2));
    assert_eq!(comps(&s, b), comps(&s, a));
    s.select_comp(b, 0);
    s.rename_comp(a, 0, "Verse".into());
    assert_eq!(comps(&s, b), (vec!["Verse".into(), "Comp 2".into(), "Comp 3".into()], 0));
    s.delete_comp(a, 1);
    assert_eq!(comps(&s, b), (vec!["Verse".into(), "Comp 3".into()], 0));
    // With the group off (the clutch), or Editing not linked, each folder acts alone.
    let mut group = s.project.group(g).unwrap().clone();
    group.enabled = false;
    s.update_group(group.clone());
    let kept = comp(&s, b);
    s.select_take(a, at[0]);
    assert_eq!(comp(&s, b), kept);
    group.enabled = true;
    group.editing = false;
    s.update_group(group);
    s.create_comp(a);
    assert_eq!(comps(&s, a).0.len(), 3);
    assert_eq!(comps(&s, b).0.len(), 2);
}

#[test]
fn only_overlapping_folders_follow() {
    let (mut s, [oh, kick, _], _) = session();
    let (a, at) = folder(&mut s, oh, 0, &[None, None]);
    // A folder further along on the kick track, and one that overlaps the overhead's.
    let (far, ft) = folder(&mut s, kick, 2 * LEN, &[None, None]);
    let (near, nt) = folder(&mut s, kick, LEN / 2, &[None, None]);
    s.select_take(a, at[0]);
    assert_eq!(comp(&s, far), vec![(0, LEN, ft[1])]);
    assert_eq!(comp(&s, near), vec![(0, LEN, nt[0])]);
    // A swipe is the same stretch of the timeline on each: take time differs by the start.
    s.set_comp_range(a, Some(at[1]), LEN / 2, LEN / 2 + 100);
    assert_eq!(comp(&s, near)[0], (0, 100, nt[1]));
}

#[test]
fn previous_and_next_take_step_selected_folders_and_partners() {
    let (mut s, [oh, kick, other], _) = session();
    let r = s.project.alloc_id();
    let passes: Vec<_> = (0..3).map(|k| Some((r, k))).collect();
    let (a, at) = folder(&mut s, oh, 0, &passes);
    let (b, bt) = folder(&mut s, kick, 0, &passes);
    let (c, ct) = folder(&mut s, other, 0, &passes);
    // Both grouped folders selected (as the group's Editing link selects them), and the third:
    // each steps once.
    s.step_take(&[a, b, c], -1);
    assert_eq!((comp(&s, a), comp(&s, b), comp(&s, c)), (vec![(0, LEN, at[1])], vec![(0, LEN, bt[1])], vec![(0, LEN, ct[1])]));
    s.step_take(&[b], -1);
    assert_eq!(comp(&s, a), vec![(0, LEN, at[0])]);
    // Past the first take nothing changes.
    s.step_take(&[a], -1);
    assert_eq!(comp(&s, b), vec![(0, LEN, bt[0])]);
    s.step_take(&[a], 1);
    assert_eq!(comp(&s, b), vec![(0, LEN, bt[1])]);
    // In a comp, the step is from the take heard longest.
    s.set_comp_range(c, Some(ct[0]), 0, 1000);
    s.step_take(&[c], 1);
    assert_eq!(comp(&s, c), vec![(0, LEN, ct[2])]);
}

#[test]
fn comp_sections_edit_in_grouped_folders_with_undo() {
    let (mut s, [oh, kick, _], _) = session();
    let r = s.project.alloc_id();
    let passes: Vec<_> = (0..3).map(|k| Some((r, k))).collect();
    // The kick's folder starts later on the timeline, so its take time differs from the overhead's.
    let (a, at) = folder(&mut s, oh, 0, &passes);
    let (b, bt) = folder(&mut s, kick, 1000, &passes);
    s.set_comp_range(a, Some(at[0]), 2000, 3000);
    assert_eq!(comp(&s, b), vec![(0, 1000, bt[2]), (1000, 2000, bt[0]), (2000, LEN, bt[2])]);
    // Drag the border at 3000 to 3500: both sections change, on both mics.
    s.checkpoint("Move Comp Section Edge", None, false);
    s.move_comp_edge(a, 3000, 3500);
    assert_eq!(comp(&s, a), vec![(0, 2000, at[2]), (2000, 3500, at[0]), (3500, LEN, at[2])]);
    assert_eq!(comp(&s, b), vec![(0, 1000, bt[2]), (1000, 2500, bt[0]), (2500, LEN, bt[2])]);
    s.undo();
    assert_eq!(comp(&s, b)[1], (1000, 2000, bt[0]));
    // Move the section down a take, then remove it.
    s.move_comp_section(b, 2500, 1);
    assert_eq!(comp(&s, a)[1], (2000, 3000, at[1]));
    assert_eq!(comp(&s, b)[1], (1000, 2000, bt[1]));
    s.move_comp_section(a, 2500, -1);
    assert_eq!(comp(&s, a)[1], (2000, 3000, at[0]));
    s.move_comp_section(a, 2500, -1);
    assert_eq!(comp(&s, a)[1], (2000, 3000, at[0]), "already the top take");
    s.remove_comp_section(a, 2100);
    assert_eq!(comp(&s, a), vec![(0, 2000, at[2]), (3000, LEN, at[2])]);
    assert_eq!(comp(&s, b), vec![(0, 1000, bt[2]), (2000, LEN, bt[2])]);
    // An outer edge: grow the first section back into the gap.
    s.move_comp_edge(b, 2000, 2400);
    assert_eq!(comp(&s, a), vec![(0, 2400, at[2]), (3000, LEN, at[2])]);
}
