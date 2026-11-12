//! Recording into take folders and the take folder commands, against a whole session.

use super::recording::*;
use crate::edit::takes::recording::RecPlan;
use crate::session::*;

fn notes_of(s: &Session, region: Id) -> Vec<u8> {
    match &s.region(region).unwrap().content {
        RegionContent::Midi { notes, .. } => notes.iter().map(|n| n.pitch).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn midi_recordings_become_take_folders() {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-midi-takes-{}", std::process::id()));
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let pass = |pitch: u8, at: i64| (vec![Note { start: at, length: 100, pitch, velocity: 100, muted: false }], Vec::new());
    // One pass over nothing: a plain region.
    s.place_midi_takes(t, 0, 1000, vec![pass(60, 10)]);
    let plain = s.project.track(t).unwrap().regions[0].id;
    assert!(s.region(plain).unwrap().takes.is_none());
    // Two passes over it: the region is packed with them, the newest heard.
    s.place_midi_takes(t, 500, 1500, vec![pass(62, 20), pass(64, 30)]);
    let regions = &s.project.track(t).unwrap().regions;
    assert_eq!(regions.len(), 1);
    let folder = regions[0].id;
    let f = regions[0].takes.clone().unwrap();
    assert_eq!((regions[0].start, regions[0].length, f.takes.len()), (0, 1500, 3));
    assert_eq!(notes_of(&s, folder), vec![64]);
    assert!(matches!(&s.region(folder).unwrap().content, RegionContent::Midi { notes, .. } if notes[0].start == 530));
    // Recording over the folder adds a take.
    s.place_midi_takes(t, 0, 1000, vec![pass(65, 0)]);
    assert_eq!(s.region(folder).unwrap().takes.as_ref().unwrap().takes.len(), 4);
    assert_eq!(notes_of(&s, folder), vec![65]);
    // Choosing a take plays it, whole; a swipe on a MIDI folder does the same.
    let ids: Vec<Id> = s.region(folder).unwrap().takes.as_ref().unwrap().takes.iter().map(|k| k.id).collect();
    s.select_take(folder, ids[0]);
    assert_eq!(notes_of(&s, folder), vec![60]);
    s.set_comp_range(folder, Some(ids[1]), 600, 700);
    assert_eq!(notes_of(&s, folder), vec![62]);
    // Edits to the content belong to the take heard.
    if let RegionContent::Midi { notes, .. } = &mut s.region_mut(folder).unwrap().content {
        Arc::make_mut(notes)[0].pitch = 70;
    }
    s.select_take(folder, ids[0]);
    s.select_take(folder, ids[1]);
    assert_eq!(notes_of(&s, folder), vec![70]);
    // Unpack: a muted track per take, the folder flattened to the take heard.
    let tracks = s.unpack_take_folder(folder);
    assert_eq!(tracks.len(), 4);
    assert!(s.region(folder).unwrap().takes.is_none());
    assert_eq!(notes_of(&s, folder), vec![70]);
    let first = &s.project.track(tracks[0]).unwrap().regions[0];
    assert!(first.muted && matches!(&first.content, RegionContent::Midi { notes, .. } if notes[0].pitch == 60));
}

fn recording_session(tag: &str) -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-audio-takes-{tag}-{}", std::process::id()));
    s
}

/// Places a recording as `finish_recording` does, one undo step: `frames` of audio captured
/// from `start` on each of `tracks`.
fn record_onto(s: &mut Session, plan: Option<RecPlan>, start: i64, frames: usize, tracks: &[Id]) {
    s.checkpoint("Record", None, false);
    s.rec_punch = Some(start);
    s.rec_plan = plan;
    let takes = tracks
        .iter()
        .map(|&t| (t, FinishedTake { path: s.dir.join(format!("{t}-{start}.wav")), start, channels: vec![vec![0.1; frames]], error: None }))
        .collect();
    s.place_recording(takes, 0);
}

/// A track's regions as (start, length), and the comp of its folder as (start, end, take index).
type Layout = (Vec<(i64, i64)>, Vec<(i64, i64, usize)>);

fn layout(s: &Session, track: Id) -> Layout {
    let regions = &s.project.track(track).unwrap().regions;
    let spans = regions.iter().map(|r| (r.start, r.length)).collect();
    let comp = regions
        .iter()
        .find_map(|r| r.takes.as_ref())
        .map(|f| f.comp.iter().map(|c| (c.start, c.end, f.takes.iter().position(|t| t.id == c.take).unwrap())).collect())
        .unwrap_or_default();
    (spans, comp)
}

#[test]
fn recording_over_audio_makes_take_folders() {
    let mut s = recording_session("over");
    assert_eq!(s.prefs.recording.audio_overlap, crate::prefs::AudioOverlap::TakeFolders);
    // Two mics: every armed track gets a folder of its own.
    let oh = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let kick = s.add_track(TrackKind::Audio, InputSel::Mono(1), None, None).unwrap();
    record_onto(&mut s, None, 1000, 1000, &[oh, kick]);
    for t in [oh, kick] {
        assert_eq!(layout(&s, t), (vec![(1000, 1000)], vec![]));
    }
    record_onto(&mut s, None, 1000, 1000, &[oh, kick]);
    let mut clips = Vec::new();
    for t in [oh, kick] {
        let r = &s.project.track(t).unwrap().regions[0];
        let f = r.takes.as_ref().unwrap();
        assert_eq!((f.takes.len(), f.whole_take()), (2, Some(f.takes[1].id)));
        assert_eq!(f.takes.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["Take 1", "Take 2"]);
        // The folder plays its newest take's audio: this track's new file.
        assert!(matches!(r.content, RegionContent::Audio { clip, offset: 0, .. } if clip == f.takes[1].clip));
        clips.push(f.takes[1].clip);
    }
    assert_ne!(clips[0], clips[1]);
    // A third pass adds a take to each folder.
    record_onto(&mut s, None, 1000, 1000, &[oh, kick]);
    for t in [oh, kick] {
        assert_eq!(s.project.track(t).unwrap().regions.len(), 1);
        assert_eq!(s.project.track(t).unwrap().regions[0].takes.as_ref().unwrap().takes.len(), 3);
    }
    // Each recording is one undo step, on both tracks.
    s.undo();
    for t in [oh, kick] {
        assert_eq!(s.project.track(t).unwrap().regions[0].takes.as_ref().unwrap().takes.len(), 2);
    }
    s.undo();
    for t in [oh, kick] {
        assert_eq!(layout(&s, t), (vec![(1000, 1000)], vec![]));
    }
    // The Replace button still wins.
    s.prefs.recording.replace = true;
    record_onto(&mut s, None, 1000, 1000, &[oh]);
    assert!(s.project.track(oh).unwrap().regions.iter().all(|r| r.takes.is_none()));
    assert_eq!(s.project.track(oh).unwrap().regions.len(), 1);
}

#[test]
fn grouped_mics_recorded_over_switch_takes_together() {
    let mut s = recording_session("grouped");
    let oh = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let kick = s.add_track(TrackKind::Audio, InputSel::Mono(1), None, None).unwrap();
    s.create_group(&[oh, kick]);
    for _ in 0..3 {
        record_onto(&mut s, None, 1000, 1000, &[oh, kick]);
    }
    let folder = |s: &Session, t: Id| s.project.track(t).unwrap().regions[0].clone();
    let (a, b) = (folder(&s, oh), folder(&s, kick));
    let passes = |r: &Region| r.takes.as_ref().unwrap().takes.iter().map(|t| t.pass).collect::<Vec<_>>();
    // The packed first take and both recordings carry the same passes on both mics.
    assert_eq!(passes(&a), passes(&b));
    assert!(passes(&a).iter().all(|p| p.is_some()));
    let ids = |r: &Region| r.takes.as_ref().unwrap().takes.iter().map(|t| t.id).collect::<Vec<_>>();
    let (at, bt) = (ids(&a), ids(&b));
    for k in 0..3 {
        s.select_take(a.id, at[k]);
        assert_eq!(folder(&s, kick).takes.unwrap().whole_take(), Some(bt[k]), "take {}", k + 1);
    }
}

#[test]
fn take_folders_cover_what_the_recording_overlaps() {
    let mut s = recording_session("spans");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    // A punch-in part-way over a region: the old take still plays before it.
    record_onto(&mut s, None, 0, 1000, &[t]);
    record_onto(&mut s, None, 500, 1000, &[t]);
    assert_eq!(layout(&s, t), (vec![(0, 1500)], vec![(0, 500, 0), (500, 1500, 1)]));
    let f = s.project.track(t).unwrap().regions[0].takes.clone().unwrap();
    // Take 2's audio starts at take time 500, its file's first frame.
    assert_eq!((f.takes[1].start, f.takes[1].end, f.takes[1].offset), (500, 1500, -500));
    s.undo();
    s.undo();

    // Over two regions: both are packed, each heard where the recording isn't.
    record_onto(&mut s, None, 0, 1000, &[t]);
    record_onto(&mut s, None, 2000, 1000, &[t]);
    record_onto(&mut s, None, 500, 2000, &[t]);
    assert_eq!(layout(&s, t), (vec![(0, 3000)], vec![(0, 500, 0), (500, 2500, 2), (2500, 3000, 1)]));
    s.undo();
    assert_eq!(layout(&s, t), (vec![(0, 1000), (2000, 1000)], vec![]));
    s.undo();
    s.undo();

    // Starting before a folder: it grows to the left and keeps playing what it did.
    record_onto(&mut s, None, 1000, 1000, &[t]);
    record_onto(&mut s, None, 1000, 1000, &[t]);
    record_onto(&mut s, None, 0, 1500, &[t]);
    assert_eq!(layout(&s, t), (vec![(0, 2000)], vec![(0, 1500, 2), (1500, 2000, 1)]));
    let r = &s.project.track(t).unwrap().regions[0];
    let f = r.takes.as_ref().unwrap();
    assert!(matches!(r.content, RegionContent::Audio { offset: 0, .. }) && f.offset == 0);
    assert_eq!(f.takes.iter().map(|k| (k.start, k.end)).collect::<Vec<_>>(), [(1000, 2000), (1000, 2000), (0, 1500)]);

    // Muted regions are left alone.
    let other = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    record_onto(&mut s, None, 0, 1000, &[other]);
    s.project.track_mut(other).unwrap().regions[0].muted = true;
    record_onto(&mut s, None, 0, 1000, &[other]);
    assert!(s.project.track(other).unwrap().regions.iter().all(|r| r.takes.is_none()));
}

#[test]
fn cycle_recording_over_take_folders_adds_takes() {
    let mut s = recording_session("cycle");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let plan = RecPlan { from: 1000, cycle: Some((1000, 2000)), punch: None, latency: 0, midi_latency: 0, min_take: 1 };
    record_onto(&mut s, None, 0, 1500, &[t]);
    // Two passes over a region: the region and both passes in one folder, the last pass heard.
    record_onto(&mut s, Some(plan), 1000, 2000, &[t]);
    assert_eq!(layout(&s, t), (vec![(0, 2000)], vec![(0, 1000, 0), (1000, 2000, 2)]));
    // Two more over that folder (which is longer than the cycle) join it.
    record_onto(&mut s, Some(plan), 1000, 2000, &[t]);
    assert_eq!(layout(&s, t), (vec![(0, 2000)], vec![(0, 1000, 0), (1000, 2000, 4)]));
    let f = s.project.track(t).unwrap().regions[0].takes.clone().unwrap();
    assert_eq!(f.takes.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["Take 1", "Take 2", "Take 3", "Take 4", "Take 5"]);
    // The passes are a cycle apart in their file.
    assert_eq!((f.takes[4].offset - f.takes[3].offset, f.takes[3].clip), (1000, f.takes[4].clip));
    s.undo();
    assert_eq!(s.project.track(t).unwrap().regions[0].takes.as_ref().unwrap().takes.len(), 3);
}

/// A session on the null device with a 10 s audio clip, and `tracks` audio tracks.
fn audio_session(tracks: usize) -> (Session, Vec<Id>, Id) {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-audio-takes-{}", std::process::id()));
    let ids = (0..tracks).map(|_| s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()).collect();
    let path = s.dir.join("Audio Files").join("ramp.wav");
    let ramp: Vec<f32> = (0..480_000).map(|i| i as f32 / 480_000.0).collect();
    let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48000, vec![ramp]));
    (s, ids, clip)
}

fn add_audio(s: &mut Session, track: Id, clip: Id, name: &str, start: i64, length: i64, offset: i64) -> Id {
    let id = s.project.alloc_id();
    let content = RegionContent::Audio { clip, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() };
    let r = Region {
        id,
        name: name.into(),
        start,
        length,
        muted: false,
        color: None,
        content,
        loop_end: None,
        takes: None,
        params: Default::default(),
        pattern: None,
    };
    s.project.track_mut(track).unwrap().regions.push(r);
    s.push_regions(track);
    id
}

#[test]
fn pack_gathers_takes_from_several_tracks() {
    let (mut s, t, clip) = audio_session(4);
    // One take per track, the bottom one recorded first on its track's timeline; the third
    // track's region stays where it is.
    let a = add_audio(&mut s, t[0], clip, "Overhead 1", 48_000, 96_000, 0);
    let b = add_audio(&mut s, t[1], clip, "Overhead 2", 24_000, 96_000, 1_000);
    let keep = add_audio(&mut s, t[1], clip, "Fill", 400_000, 10_000, 0);
    let c = add_audio(&mut s, t[3], clip, "Overhead 3", 48_000, 120_000, 2_000);
    let other = add_audio(&mut s, t[2], clip, "Kick", 0, 10_000, 0);
    s.checkpoint("Pack Take Folder", None, false);
    // The order of the ids doesn't matter: tracks go top first.
    let folder = s.pack_take_folder(&[c, a, b]).unwrap();
    let (track, _) = s.find_region(folder).unwrap();
    assert_eq!(track, t[0], "the folder is on the topmost track");
    let r = s.region(folder).unwrap().clone();
    assert_eq!((r.start, r.length), (24_000, 168_000 - 24_000));
    let f = r.takes.clone().unwrap();
    let names: Vec<&str> = f.takes.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(names, ["Overhead 1", "Overhead 2", "Overhead 3"]);
    // Each take keeps its own place: take time 0 is the folder start.
    let spans: Vec<(i64, i64, i64)> = f.takes.iter().map(|k| (k.start, k.end, k.offset)).collect();
    assert_eq!(spans, [(24_000, 120_000, -24_000), (0, 96_000, 1_000), (24_000, 144_000, 2_000 - 24_000)]);
    assert_eq!(f.whole_take(), None, "the last take is heard, the others fill in before it");
    assert_eq!(f.comp.last().unwrap().take, f.takes[2].id);
    // The other tracks lose only what was packed.
    assert_eq!(s.project.track(t[1]).unwrap().regions.iter().map(|r| r.id).collect::<Vec<_>>(), [keep]);
    assert!(s.project.track(t[3]).unwrap().regions.is_empty());
    assert_eq!(s.project.track(t[2]).unwrap().regions[0].id, other);
    // One undo step puts every region back on its own track.
    s.undo();
    for (track, id) in [(t[0], a), (t[1], b), (t[3], c)] {
        assert_eq!(s.find_region(id).map(|(tr, _)| tr), Some(track));
    }
    assert!(s.region(folder).is_none());
    // MIDI mixed in is refused, wherever it is.
    let m = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.place_midi_takes(m, 0, 1000, vec![(Vec::new(), Vec::new())]);
    let midi = s.project.track(m).unwrap().regions[0].id;
    assert!(s.pack_take_folder(&[a, midi]).is_err());
    assert!(s.pack_take_folder(&[a]).is_err());
}

fn names(s: &Session) -> Vec<String> {
    s.project.tracks.iter().map(|t| t.name.clone()).collect()
}

#[test]
fn unpack_to_existing_tracks_fills_the_tracks_below() {
    let (mut s, t, clip) = audio_session(3);
    for (k, name) in ["Folder", "Below 1", "Below 2"].iter().enumerate() {
        s.project.track_mut(t[k]).unwrap().name = name.to_string();
    }
    // An instrument track between the two audio tracks is passed over.
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, Some(2)).unwrap();
    s.project.track_mut(inst).unwrap().name = "Keys".into();
    let below = add_audio(&mut s, t[1], clip, "Already here", 200_000, 1_000, 0);
    let ids: Vec<Id> = (0..4).map(|k| add_audio(&mut s, t[0], clip, &format!("Take {}", k + 1), 1_000, 10_000, k * 20_000)).collect();
    let folder = s.pack_take_folder(&ids).unwrap();
    s.checkpoint("Unpack Take Folder", None, false);
    let used = s.unpack_to_existing_tracks(folder);
    assert_eq!(used.len(), 4);
    assert_eq!((used[0], used[1]), (t[1], t[2]));
    // Two more tracks, like the folder's, below the last one used.
    assert_eq!(names(&s), ["Folder", "Below 1", "Keys", "Below 2", "Take 3", "Take 4"]);
    for (k, &track) in used.iter().enumerate() {
        let take = s.project.track(track).unwrap().regions.iter().find(|r| r.name == format!("Take {}", k + 1)).unwrap().clone();
        assert!(take.muted);
        assert!(matches!(take.content, RegionContent::Audio { offset, .. } if offset == k as i64 * 20_000));
        assert_eq!((take.start, take.length), (1_000, 10_000));
    }
    // The tracks keep what they had; the folder is flattened to the take heard.
    assert!(s.region(below).is_some());
    let flat = &s.project.track(t[0]).unwrap().regions;
    assert!(flat.len() == 1 && flat[0].takes.is_none() && !flat[0].muted);
    s.undo();
    assert_eq!(names(&s), ["Folder", "Below 1", "Keys", "Below 2"]);
    assert!(s.region(folder).unwrap().takes.is_some());
    assert_eq!(s.project.track(t[1]).unwrap().regions.len(), 1);
}

#[test]
fn export_and_move_the_active_take() {
    let (mut s, t, clip) = audio_session(2);
    s.project.track_mut(t[0]).unwrap().name = "Vocal".into();
    let a = add_audio(&mut s, t[0], clip, "A", 0, 1_000, 0);
    let b = add_audio(&mut s, t[0], clip, "B", 0, 1_000, 5_000);
    let folder = s.pack_take_folder(&[a, b]).unwrap();
    let takes: Vec<Id> = s.region(folder).unwrap().takes.as_ref().unwrap().takes.iter().map(|k| k.id).collect();
    let region_on = |s: &Session, track: Id| s.project.track(track).unwrap().regions.clone();
    // Export: a muted copy of take B on a new track below, the folder as it was.
    s.checkpoint("Export Active Take", None, false);
    let made = s.export_active_take(folder, false);
    assert_eq!(names(&s), ["Vocal", "B", "Audio 2"]);
    let copy = region_on(&s, made[0]);
    assert_eq!(copy.len(), 1);
    assert!(copy[0].muted && copy[0].takes.is_none() && copy[0].name == "B");
    assert!(matches!(copy[0].content, RegionContent::Audio { offset: 5_000, clip: c, .. } if c == clip));
    assert_eq!(s.region(folder).unwrap().takes.as_ref().unwrap().takes.len(), 2);
    s.undo();
    // Move: the copy plays, the folder loses take B, plays A, and is muted.
    s.checkpoint("Move Active Take", None, false);
    let made = s.export_active_take(folder, true);
    assert!(!region_on(&s, made[0])[0].muted);
    let f = s.region(folder).unwrap().takes.clone().unwrap();
    assert_eq!((f.takes.len(), f.whole_take()), (1, Some(takes[0])));
    assert!(s.region(folder).unwrap().muted);
    // Moving the only take left moves the folder out altogether.
    s.export_active_take(folder, true);
    assert!(s.region(folder).is_none());
    assert_eq!(names(&s), ["Vocal", "A", "B", "Audio 2"]);
    s.undo();
    assert_eq!(names(&s), ["Vocal", "Audio 2"]);
    // A comp is exported flattened and named after the comp; moving it leaves a fresh one.
    s.set_comp_range(folder, Some(takes[0]), 0, 500);
    let made = s.export_active_take(folder, false);
    let copy = region_on(&s, made[0]);
    assert_eq!(copy.iter().map(|r| (r.start, r.length, r.name.as_str())).collect::<Vec<_>>(), [(0, 500, "Comp 1"), (500, 500, "Comp 1")]);
    s.export_active_take(folder, true);
    let f = s.region(folder).unwrap().takes.clone().unwrap();
    assert_eq!((f.takes.len(), f.comp_names().len(), f.whole_take()), (2, 1, Some(takes[1])));
}

/// Deleting the take a MIDI folder's comp played leaves it playing another, and the comp kept in
/// the list of comps must say so too (#424).
#[test]
fn a_midi_comp_left_empty_by_a_deleted_take_is_stored_as_what_plays() {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let pass = |pitch: u8| (vec![Note { start: 10, length: 100, pitch, velocity: 100, muted: false }], Vec::new());
    s.place_midi_takes(t, 0, 1000, vec![pass(60), pass(62)]);
    let folder = s.project.track(t).unwrap().regions[0].id;
    let ids: Vec<Id> = s.region(folder).unwrap().takes.as_ref().unwrap().takes.iter().map(|k| k.id).collect();
    s.select_take(folder, ids[0]);
    s.duplicate_comp(folder);
    s.delete_take(folder, ids[0]);
    assert_eq!(notes_of(&s, folder), vec![62]);
    let f = s.region(folder).unwrap().takes.clone().unwrap();
    assert_eq!(f.comp.first().map(|c| c.take), Some(ids[1]));
    assert_eq!(f.comps[f.current].comp, f.comp, "the comp in the list is the one heard");
}

#[test]
fn export_a_midi_take() {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let pass = |pitch: u8| (vec![Note { start: 10, length: 100, pitch, velocity: 100, muted: false }], Vec::new());
    s.place_midi_takes(t, 0, 1000, vec![pass(60), pass(62)]);
    let folder = s.project.track(t).unwrap().regions[0].id;
    let made = s.export_active_take(folder, false);
    let copy = s.project.track(made[0]).unwrap().regions[0].clone();
    assert!(copy.takes.is_none() && copy.muted);
    assert_eq!(notes_of(&s, copy.id), vec![62]);
    // Unpack to existing tracks: the exported track takes the first take, a new one the second.
    let used = s.unpack_to_existing_tracks(folder);
    assert_eq!(used[0], made[0]);
    assert_eq!(s.project.track(used[1]).unwrap().regions.len(), 1);
    assert_eq!(notes_of(&s, s.project.track(used[1]).unwrap().regions[0].id), vec![62]);
    assert_eq!(notes_of(&s, s.project.track(used[0]).unwrap().regions[1].id), vec![60]);
}

#[test]
fn midi_window_follows_cycle_and_punch() {
    let plan = RecPlan { from: 0, cycle: Some((1000, 2000)), punch: None, latency: 0, midi_latency: 0, min_take: 1 };
    let w = midi_window(Some(plan), 1000, 1500);
    assert_eq!((w.lo, w.hi), (1000, 2000));
    // Held across the loop point: ends at the cycle end.
    assert_eq!(w.note_end(1900, 1100), 2000);
    assert_eq!(w.note_end(1200, 1300), 1300);
    let punched = midi_window(Some(RecPlan { cycle: None, punch: Some((1200, 1600)), ..plan }), 0, 3000);
    assert_eq!((punched.lo, punched.hi), (1200, 1600));
    assert!(!punched.keeps(1100) && punched.keeps(1200) && !punched.keeps(1600));
    assert_eq!(punched.note_end(1500, 1900), 1600);
    let plain = midi_window(None, 500, 900);
    assert_eq!((plain.lo, plain.hi, plain.note_end(600, 700)), (500, 900, 700));
}
