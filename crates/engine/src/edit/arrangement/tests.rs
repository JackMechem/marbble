//! The Arrangement track's arithmetic: tiling, resizing, moving, copying and deleting sections.

use super::*;

fn sec(id: Id, start: i64, length: i64) -> ArrangementMarker {
    ArrangementMarker { id, name: format!("S{id}"), start, length, color: None }
}

/// Three sections: 1 at 100..200, 2 at 200..250, 3 at 250..400.
fn three() -> Vec<ArrangementMarker> {
    vec![sec(1, 100, 100), sec(2, 200, 50), sec(3, 250, 150)]
}

fn spans(v: &[ArrangementMarker]) -> Vec<(Id, i64, i64)> {
    v.iter().map(|s| (s.id, s.start, s.end())).collect()
}

fn pt(pos: i64, value: f32) -> AutoPoint {
    AutoPoint { pos, value, curve: 0.0 }
}

fn at(points: &[AutoPoint], x: i64) -> f32 {
    value_at(points, x).unwrap()
}

#[test]
fn sections_tile_without_gaps() {
    let mut v = vec![sec(2, 500, 50), sec(1, 100, 100), sec(3, 900, 0)];
    tile(&mut v);
    assert_eq!(spans(&v), vec![(1, 100, 200), (2, 200, 250), (3, 250, 251)]);
    let v = add(&v, 4, "Outro".into(), 80);
    assert_eq!(spans(&v).last(), Some(&(4, 251, 331)));
    assert_eq!(spans(&add(&[], 1, "Intro".into(), 80)), vec![(1, 0, 80)]);
}

#[test]
fn names_follow_a_song() {
    let mut v = Vec::new();
    let mut names = Vec::new();
    for id in 1..=8 {
        let n = next_name(&v);
        v = add(&v, id, n.clone(), 10);
        names.push(n);
    }
    assert_eq!(names, ["Intro", "Verse", "Chorus", "Bridge", "Outro", "Verse", "Chorus", "Bridge"]);
    assert!(names.iter().all(|n| NAMES.contains(&n.as_str())));
}

#[test]
fn resizing_ripples_the_later_sections() {
    let v = resize(&three(), 1, 40);
    assert_eq!(spans(&v), vec![(1, 100, 140), (2, 140, 190), (3, 190, 340)]);
    let v = resize(&three(), 3, 0);
    assert_eq!(spans(&v)[2], (3, 250, 251), "never shorter than a sample");
}

#[test]
fn removing_a_marker_gives_its_time_to_the_one_before() {
    assert_eq!(spans(&remove(&three(), 2)), vec![(1, 100, 250), (3, 250, 400)]);
    assert_eq!(spans(&remove(&three(), 1)), vec![(2, 200, 250), (3, 250, 400)]);
}

#[test]
fn moving_a_section_swaps_its_time_with_the_ones_it_passes() {
    // Section 3 before section 2.
    let plan = move_section(&three(), 3, Some(2)).unwrap();
    assert_eq!(spans(&plan.sections), vec![(1, 100, 200), (3, 200, 350), (2, 350, 400)]);
    assert_eq!(plan.focus, Some((200, 350)));
    assert_eq!(
        plan.pieces,
        vec![
            // Before the arrangement and section 1 stay; 2 and 3 trade places; the rest stays.
            Piece { start: 0, end: 200, dst: 0, copy: false },
            Piece { start: 200, end: 250, dst: 350, copy: false },
            Piece { start: 250, end: 400, dst: 200, copy: false },
            Piece { start: 400, end: i64::MAX, dst: 400, copy: false },
        ]
    );
    assert_eq!(map_pos(&plan.pieces, 260), vec![210]);
    assert_eq!(map_pos(&plan.pieces, 210), vec![360]);
    assert_eq!(map_pos(&plan.pieces, 50), vec![50]);
    // Section 1 to the end: the others move up by its length.
    let plan = move_section(&three(), 1, None).unwrap();
    assert_eq!(spans(&plan.sections), vec![(2, 100, 150), (3, 150, 300), (1, 300, 400)]);
    assert_eq!(plan.pieces[1], Piece { start: 100, end: 200, dst: 300, copy: false });
    assert_eq!(plan.pieces[2], Piece { start: 200, end: 400, dst: 100, copy: false });
    // Onto its own place: nothing to do.
    assert!(move_section(&three(), 2, Some(2)).is_none());
    assert!(move_section(&three(), 2, Some(3)).is_none());
    assert!(move_section(&three(), 3, None).is_none());
    assert!(move_section(&three(), 9, None).is_none());
}

#[test]
fn copying_a_section_makes_room_for_it() {
    // A copy of section 1 between 2 and 3.
    let plan = copy_section(&three(), 1, Some(3), 7).unwrap();
    assert_eq!(spans(&plan.sections), vec![(1, 100, 200), (2, 200, 250), (7, 250, 350), (3, 350, 500)]);
    assert_eq!(plan.focus, Some((250, 350)));
    assert_eq!(map_pos(&plan.pieces, 150), vec![150, 300], "the original first, then the copy");
    assert_eq!(map_pos(&plan.pieces, 300), vec![400]);
    assert_eq!(map_pos(&plan.pieces, 1000), vec![1100], "time after the arrangement moves along");
    assert!(plan.pieces.iter().any(|p| p.copy && p.start == 100 && p.end == 200));
}

#[test]
fn deleting_a_section_closes_the_gap() {
    let plan = delete_section(&three(), 2).unwrap();
    assert_eq!(spans(&plan.sections), vec![(1, 100, 200), (3, 200, 350)]);
    assert!(map_pos(&plan.pieces, 220).is_empty(), "its content goes");
    assert_eq!(map_pos(&plan.pieces, 300), vec![250]);
    assert_eq!(map_pos(&plan.pieces, 450), vec![400]);
    assert_eq!(map_pos(&plan.pieces, 150), vec![150]);
    // The only section.
    let plan = delete_section(&[sec(1, 100, 100)], 1).unwrap();
    assert!(plan.sections.is_empty());
    assert_eq!((map_pos(&plan.pieces, 50), map_pos(&plan.pieces, 150), map_pos(&plan.pieces, 250)), (vec![50], vec![], vec![150]));
}

#[test]
fn automation_moves_with_its_section_and_keeps_its_shape() {
    // A ramp from 0 to 1 over section 3 (250..400) on a lane at 0.5 elsewhere, and a step
    // in section 2.
    let lane = vec![pt(0, 0.5), pt(220, 0.5), pt(220, 0.8), pt(250, 0.0), pt(399, 1.0), pt(400, 0.5)];
    let plan = move_section(&three(), 3, Some(2)).unwrap();
    let moved = remap_points(&lane, &plan.pieces);
    assert!(moved.windows(2).all(|w| w[0].pos <= w[1].pos), "{moved:?}");
    // Every position inside each piece plays what it did before the move.
    for p in &plan.pieces {
        let hi = p.end.min(500);
        for x in (p.start..hi).step_by(7).chain([p.start, hi - 1]) {
            assert!((at(&moved, x + p.delta()) - at(&lane, x)).abs() < 1e-5, "{x} -> {}: {} vs {}", x + p.delta(), at(&moved, x + p.delta()), at(&lane, x));
        }
    }
    // No more points than the curve needs: no flat stretches get extra points.
    let flat = remap_points(&[pt(0, -6.0)], &plan.pieces);
    assert_eq!(flat, vec![pt(0, -6.0)]);
}

#[test]
fn automation_is_copied_and_deleted_with_its_section() {
    let lane = vec![pt(100, 0.0), pt(199, 1.0), pt(200, 0.2), pt(400, 0.2)];
    let plan = copy_section(&three(), 1, Some(3), 7).unwrap();
    let copied = remap_points(&lane, &plan.pieces);
    for x in [100, 150, 199] {
        assert!((at(&copied, x + 150) - at(&lane, x)).abs() < 1e-5, "copy at {x}");
        assert!((at(&copied, x) - at(&lane, x)).abs() < 1e-5, "original at {x}");
    }
    assert!((at(&copied, 400) - 0.2).abs() < 1e-5);
    let plan = delete_section(&three(), 1).unwrap();
    let gone = remap_points(&lane, &plan.pieces);
    assert!((at(&gone, 50) - 0.0).abs() < 1e-5 && (at(&gone, 99) - 0.0).abs() < 1e-5, "{gone:?}");
    assert!((at(&gone, 100) - 0.2).abs() < 1e-5 && (at(&gone, 300) - 0.2).abs() < 1e-5, "{gone:?}");
}
