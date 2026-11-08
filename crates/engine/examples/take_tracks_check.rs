//! Takes recorded one per track, against a running engine (silent ALSA null output, rendered
//! offline): Pack Take Folder from regions on several tracks, heard as one folder, and deleting
//! the tracks it leaves empty; nudging regions on several tracks by samples; Unpack to Existing
//! Tracks and Export / Move Active Take to New Track, which leave the song sounding the same.
//! usage: take_tracks_check <audio file> <scratch dir>
use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, TrackKind};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn peak(a: &[f32]) -> f32 {
    a.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

fn render(s: &mut Session, from: i64, to: i64) -> Vec<f32> {
    s.render(from, to, 0, None, &mut |_| {}).unwrap().swap_remove(0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("TakeTracksCheck.marbble");
    let _ = std::fs::remove_dir_all(&s.dir);
    std::fs::create_dir_all(s.dir.join("Audio Files")).unwrap();
    let sr = s.sample_rate() as i64;
    let len = 2 * sr;

    // ---- three takes of the tone, one per track, at 0, -6 and -12 dB -------------------------
    let tracks: Vec<Id> = (0..3).map(|_| s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()).collect();
    let mut ids = Vec::new();
    for (&t, gain) in tracks.iter().zip([0.0, -6.0, -12.0]) {
        let id = s.import_audio(std::path::Path::new(&tone), t, 0).unwrap();
        s.resize_region(id, 0, len);
        s.set_region_gain(&[id], Some(gain), None);
        ids.push(id);
    }
    let window = (sr / 10) as usize..(len - sr / 10) as usize;
    let all = peak(&render(&mut s, 0, len)[window.clone()]);

    // ---- Pack Take Folder across the tracks -----------------------------------------------
    s.checkpoint("Pack Take Folder", None, false);
    let folder = s.pack_take_folder(&ids).unwrap();
    let on = |s: &Session, t: Id| s.project.track(t).unwrap().regions.len();
    check(
        &format!("the folder is on the top track, the others are empty ({}, {}, {})", on(&s, tracks[0]), on(&s, tracks[1]), on(&s, tracks[2])),
        on(&s, tracks[0]) == 1 && on(&s, tracks[1]) == 0 && on(&s, tracks[2]) == 0,
    );
    let f = s.project.track(tracks[0]).unwrap().regions[0].takes.clone().unwrap();
    let takes: Vec<Id> = f.takes.iter().map(|t| t.id).collect();
    check(&format!("one take per track, the bottom one heard ({} takes)", takes.len()), takes.len() == 3 && f.whole_take() == Some(takes[2]));
    let heard = peak(&render(&mut s, 0, len)[window.clone()]);
    s.select_take(folder, takes[0]);
    let first = peak(&render(&mut s, 0, len)[window.clone()]);
    check(
        &format!("only the folder's take plays ({:.3} of take 1; all three together were {:.3})", heard / first, all / first),
        (heard / first - 0.251).abs() < 0.01 && (all / first - 1.752).abs() < 0.03,
    );

    // ---- deleting the tracks left empty, as a step of its own ------------------------------
    s.checkpoint("Delete Tracks", None, true);
    s.delete_tracks(&tracks[1..]);
    check("the empty tracks go", s.project.track(tracks[1]).is_none() && s.project.track(tracks[2]).is_none());
    s.undo();
    check("undo brings them back, still empty", on(&s, tracks[1]) == 0 && on(&s, tracks[2]) == 0);
    s.undo();
    check(
        "undo puts each take back on its own track",
        (0..3).all(|k| s.project.track(tracks[k]).unwrap().regions.iter().map(|r| r.id).collect::<Vec<_>>() == [ids[k]]),
    );
    let back = peak(&render(&mut s, 0, len)[window.clone()]);
    check(&format!("and they play together again ({:.3})", back / first), (back - all).abs() < 1e-4);

    // ---- nudging by samples: every region moves the same number of samples -----------------
    s.move_region(ids[1], 10, None);
    let before = render(&mut s, 0, len);
    s.checkpoint("Nudge", None, false);
    s.drag_nudge_regions(&ids, 1, Some(false));
    let after = render(&mut s, 0, len);
    check(
        "a nudge by a sample delays the mix by exactly one sample",
        after[0].abs() < 1e-9 && (1..len as usize).all(|i| (after[i] - before[i - 1]).abs() < 1e-6),
    );
    // Five samples back: the regions at 1 can go back only one, and the one at 11 goes with them.
    s.drag_nudge_regions(&ids, -5, Some(false));
    let starts: Vec<i64> = ids.iter().map(|&id| s.project.tracks.iter().flat_map(|t| &t.regions).find(|r| r.id == id).unwrap().start).collect();
    let again = render(&mut s, 0, len);
    check(&format!("past the start they stop together ({starts:?})"), starts == [0, 10, 0] && again.iter().zip(&before).all(|(a, b)| (a - b).abs() < 1e-6));

    // ---- unpacking to existing tracks, exporting and moving the active take -----------------
    s.move_region(ids[1], 0, None);
    let folder = s.pack_take_folder(&ids).unwrap();
    let packed = render(&mut s, 0, len);
    let same = |a: &[f32]| a.iter().zip(&packed).all(|(x, y)| (x - y).abs() < 1e-6);
    let peak_of = |a: &[f32]| peak(&a[window.clone()]);
    s.checkpoint("Unpack Take Folder", None, false);
    let used = s.unpack_to_existing_tracks(folder);
    let out = render(&mut s, 0, len);
    let muted = used.iter().all(|&t| s.project.track(t).unwrap().regions.iter().all(|r| r.muted));
    check(
        &format!("unpack to existing tracks fills the two below and adds one ({} tracks, {} used)", s.project.tracks.len(), used.len()),
        used.len() == 3 && used[..2] == tracks[1..] && s.project.tracks.len() == 4 && muted,
    );
    check(&format!("the song sounds the same after it ({:.3} of take 1)", peak_of(&out) / first), same(&out));
    s.undo();

    s.checkpoint("Export Active Take", None, false);
    let made = s.export_active_take(folder, false);
    check("exporting the active take changes nothing heard", same(&render(&mut s, 0, len)));
    let copy: Vec<Id> = s.project.track(made[0]).unwrap().regions.iter().map(|r| r.id).collect();
    for &c in &copy {
        s.set_region_muted(c, false);
    }
    s.set_region_muted(folder, true);
    check("the copy on its new track plays what the folder played", copy.len() == 1 && same(&render(&mut s, 0, len)));
    s.undo();

    s.checkpoint("Move Active Take", None, false);
    let made = s.export_active_take(folder, true);
    let out = render(&mut s, 0, len);
    let left = s.project.tracks.iter().flat_map(|t| &t.regions).find(|r| r.id == folder).map(|r| (r.muted, r.takes.as_ref().map(|f| f.takes.len())));
    check(&format!("moving it takes the take out of the folder and mutes the folder ({left:?})"), made.len() == 1 && left == Some((true, Some(2))));
    check(&format!("and the moved take plays instead ({:.3} of take 1)", peak_of(&out) / first), same(&out));
    s.undo();
    check("undo puts the take back", s.project.tracks.len() == 3);

    std::process::exit(if FAILED.load(std::sync::atomic::Ordering::Relaxed) { 1 } else { 0 });
}
