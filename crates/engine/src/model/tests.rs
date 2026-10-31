//! Regions, loops and real copies.

use super::*;

fn audio(start: i64, length: i64, loop_end: Option<i64>) -> Region {
    Region {
        id: 1,
        name: "A".into(),
        start,
        length,
        muted: false,
        color: None,
        content: RegionContent::Audio { clip: 9, offset: 100, gain_db: 0.0, fade_in: 10, fade_out: 30, fade_shape: FadeShape::default() },
        loop_end,
        takes: None,
        params: Default::default(),
        pattern: None,
    }
}

#[test]
fn loop_passes_cut_the_last_one_short() {
    let r = audio(1000, 100, Some(1350));
    assert_eq!(r.end(), 1350);
    assert!(r.is_looped());
    assert_eq!(r.passes(), vec![(1000, 100), (1100, 100), (1200, 100), (1300, 50)]);
    // A loop end at or before the region's end is no loop.
    for e in [None, Some(1100), Some(500)] {
        let r = audio(1000, 100, e);
        assert_eq!((r.end(), r.passes().len(), r.is_looped()), (1100, 1, false));
    }
}

#[test]
fn moving_takes_the_loop_along() {
    let mut r = audio(1000, 100, Some(1350));
    r.move_to(400);
    assert_eq!((r.start, r.loop_end), (400, Some(750)));
}

#[test]
fn real_copies_match_the_loop() {
    let r = audio(1000, 100, Some(1350));
    let c = r.real_copies();
    let starts: Vec<(i64, i64)> = c.iter().map(|x| (x.start, x.length)).collect();
    assert_eq!(starts, r.passes());
    let fades: Vec<(i64, i64, i64)> = c
        .iter()
        .map(|x| match x.content {
            RegionContent::Audio { offset, fade_in, fade_out, .. } => (offset, fade_in, fade_out),
            _ => unreachable!(),
        })
        .collect();
    // Fade-in on the first pass, the fade-out (shortened to fit) on the last.
    assert_eq!(fades, vec![(100, 10, 0), (100, 0, 0), (100, 0, 0), (100, 0, 30)]);
    assert!(c.iter().all(|x| x.loop_end.is_none()));

    let notes =
        vec![Note { start: 0, length: 10, pitch: 60, velocity: 100, muted: false }, Note { start: 60, length: 10, pitch: 62, velocity: 100, muted: false }];
    let m = Region { content: RegionContent::Midi { notes, cc: Vec::new() }, ..audio(0, 100, Some(250)) };
    let counts: Vec<usize> = m
        .real_copies()
        .iter()
        .map(|x| match &x.content {
            RegionContent::Midi { notes, .. } => notes.len(),
            _ => 0,
        })
        .collect();
    // The 50-sample last pass ends before the second note.
    assert_eq!(counts, vec![2, 2, 1]);
}

#[test]
fn old_projects_load_without_loops_or_hidden_tracks() {
    let r: Region = serde_json::from_str(r#"{"id":1,"name":"x","start":0,"length":10,"content":{"type":"midi","notes":[]}}"#).unwrap();
    assert_eq!(r.loop_end, None);
    let t: Track = serde_json::from_str(
        r##"{"id":2,"kind":"audio","name":"a","color":"#fff","volume_db":0,"pan":0,"mute":false,"solo":false,"armed":false,"monitor":false,"input":{"type":"none"}}"##,
    )
    .unwrap();
    assert!(!t.hidden);
    assert!(!t.off, "an older project's tracks are on");
    let json = serde_json::to_string(&t).unwrap();
    assert!(!json.contains("\"off\""), "a track that's on saves as older projects do");
    let t: Track = serde_json::from_str(&json.replacen('{', "{\"off\":true,", 1)).unwrap();
    assert!(t.off);
    assert!(serde_json::to_string(&t).unwrap().contains("\"off\":true"));
}
