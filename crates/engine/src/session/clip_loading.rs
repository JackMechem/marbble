//! Decoding clips when something can play them. Opening a project decodes only the clips its
//! tracks can play ([`Project::clips_in_use`]: regions, every take in a folder, inactive track
//! alternatives, freeze files). The rest of `Project::clips`, regions and takes deleted in an
//! earlier session, stays in the document as a file reference, so saving keeps it, and is
//! decoded only if an edit brings it back.
//!
//! Whatever hands regions to the engine (`push_regions`, `build_track`) loads their clips first,
//! on the calling thread, so undo, redo, switching an alternative or a paste never leave the
//! audio thread a region without its audio. Edits that check a region's clip before placing it
//! (paste, Insert Cut Section) call `load_clips` themselves. Clips are never unloaded while the
//! project is open: undo and redo may need them again.

use super::*;

impl Session {
    /// Decodes those of `ids` that the project lists and that aren't in memory yet. A file that
    /// can't be read is logged once and not tried again until a project opens.
    pub(super) fn load_clips(&mut self, ids: impl IntoIterator<Item = Id>) {
        for id in ids {
            if self.clips.contains_key(&id) || self.clips_unreadable.contains(&id) {
                continue;
            }
            let Some(r) = self.project.clips.iter().find(|c| c.id == id) else { continue };
            let path = if Path::new(&r.path).is_absolute() { PathBuf::from(&r.path) } else { self.dir.join(&r.path) };
            match Clip::load(&path) {
                Ok(clip) => {
                    self.clips.insert(id, Arc::new(clip));
                }
                Err(e) => {
                    log::error!("missing audio file {}: {e}", path.display());
                    self.clips_unreadable.insert(id);
                }
            }
        }
    }

    /// Decodes the clips a just-opened project can play, and only those.
    pub(super) fn load_project_clips(&mut self) {
        self.clips.clear();
        self.clips_unreadable.clear();
        let used = self.project.clips_in_use();
        let unused = self.project.clips.iter().filter(|c| !used.contains(&c.id)).count();
        if unused > 0 {
            log::info!("{unused} of the project's {} audio files are used by nothing, and stay on disk", self.project.clips.len());
        }
        self.load_clips(used);
    }

    /// Whether clip `id`'s audio is in memory.
    pub fn clip_loaded(&self, id: Id) -> bool {
        self.clips.contains_key(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEN: usize = 4800;

    /// A session on ALSA's null device in a project folder of its own, with an audio track
    /// playing `a.wav` at 0 and `b.wav` at `LEN`, each a constant level.
    fn session(tag: &str) -> (Session, Id, [Id; 2]) {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("marbble-clip-loading-{tag}-{}.marbble", std::process::id()));
        let _ = std::fs::remove_dir_all(&s.dir);
        let audio = s.dir.join(AUDIO_DIR);
        std::fs::create_dir_all(&audio).unwrap();
        let rate = s.sample_rate() as u32;
        let track = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let mut regions = [0; 2];
        for (k, (name, level)) in [("a", 0.25f32), ("b", 0.5)].into_iter().enumerate() {
            let path = audio.join(format!("{name}.wav"));
            crate::decode::write_wav(&path, rate, &[vec![level; LEN]]).unwrap();
            regions[k] = s.import_audio(&path, track, (k * LEN) as i64).unwrap();
        }
        // No fades, so the levels render exactly.
        for t in &mut s.project.tracks {
            for r in &mut t.regions {
                if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                    (*fade_in, *fade_out) = (0, 0);
                }
            }
        }
        s.push_regions(track);
        (s, track, regions)
    }

    fn clip_of(s: &Session, region: Id) -> Id {
        match s.region(region).unwrap().content {
            RegionContent::Audio { clip, .. } => clip,
            _ => unreachable!(),
        }
    }

    fn render(s: &mut Session) -> Vec<Vec<f32>> {
        s.render(0, 2 * LEN as i64, 0, None, &mut |_| {}).unwrap()
    }

    /// Saves, opens the project again in the same session and returns it.
    fn reopen(s: &mut Session) {
        let dir = s.save(None).unwrap();
        s.open(&dir).unwrap();
    }

    #[test]
    fn opening_decodes_only_the_clips_in_use_and_saving_keeps_the_rest() {
        let (mut s, _, [a, b]) = session("open");
        let (clip_a, clip_b) = (clip_of(&s, a), clip_of(&s, b));
        s.delete_regions(&[b]);
        reopen(&mut s);
        assert!(s.clip_loaded(clip_a));
        assert!(!s.clip_loaded(clip_b), "a clip no region uses isn't decoded");
        // It stays in the document, saved again and again, with its file.
        assert!(s.project.clips.iter().any(|c| c.id == clip_b));
        reopen(&mut s);
        assert!(s.project.clips.iter().any(|c| c.id == clip_b));
        assert!(!s.clip_loaded(clip_b));
        assert!(s.dir.join(AUDIO_DIR).join("b.wav").exists());
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn undo_and_paste_load_a_clip_that_comes_back() {
        let (mut s, track, [_, b]) = session("undo");
        let both = render(&mut s);
        let clip_b = clip_of(&s, b);
        let region_b = s.region(b).unwrap().clone();
        let before = s.project.clone();
        s.delete_regions(&[b]);
        reopen(&mut s);
        assert!(!s.clip_loaded(clip_b));
        let one = render(&mut s);
        assert!(one[0][LEN..].iter().all(|&x| x == 0.0));

        // An undo step that brings the region back (as one from before the reopen would).
        s.push_step(before, "Delete", None);
        s.undo();
        assert!(s.clip_loaded(clip_b));
        assert_eq!(render(&mut s), both, "undo plays the clip it loaded");
        s.redo();
        assert_eq!(render(&mut s), one);

        // A region still on the clipboard, pasted after the reopen.
        reopen(&mut s);
        assert!(!s.clip_loaded(clip_b));
        let made = s.paste_regions(&[(track, region_b)], 0);
        assert_eq!(made.len(), 1, "a region whose clip the project lists can be pasted");
        assert!(s.clip_loaded(clip_b));
        assert_eq!(render(&mut s), both);
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn inactive_alternatives_and_unplayed_takes_count_as_in_use() {
        let (mut s, track, [a, b]) = session("alternatives");
        let (clip_a, clip_b) = (clip_of(&s, a), clip_of(&s, b));
        // B is left only in the first alternative, inactive once an empty one is made.
        s.delete_regions(&[a]);
        s.new_track_alternative(track, false).unwrap();
        assert!(s.project.track(track).unwrap().regions.is_empty());
        reopen(&mut s);
        assert!(s.clip_loaded(clip_b), "an inactive alternative's clip is decoded");
        assert!(!s.clip_loaded(clip_a));
        let first = s.project.track(track).unwrap().alternatives[0].name.clone();
        s.switch_track_alternative(track, &first).unwrap();
        assert!((render(&mut s)[0][LEN] - 0.5).abs() < 1e-3, "the alternative switched to plays");
        let _ = std::fs::remove_dir_all(&s.dir);

        // A take folder whose comp plays one take keeps the other's clip too.
        let (mut s, _, [a, b]) = session("takes");
        let (clip_a, clip_b) = (clip_of(&s, a), clip_of(&s, b));
        let folder = s.pack_take_folder(&[a, b]).unwrap();
        let take_a = s.region(folder).unwrap().takes.as_ref().unwrap().takes.iter().find(|t| t.clip == clip_a).unwrap().id;
        s.select_take(folder, take_a);
        reopen(&mut s);
        assert!(s.clip_loaded(clip_a) && s.clip_loaded(clip_b));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn an_unreadable_clip_is_tried_once() {
        let (mut s, _, [_, b]) = session("unreadable");
        let clip_b = clip_of(&s, b);
        s.delete_regions(&[b]);
        let dir = s.save(None).unwrap();
        std::fs::remove_file(dir.join(AUDIO_DIR).join("b.wav")).unwrap();
        s.open(&dir).unwrap();
        s.load_clips([clip_b]);
        assert!(!s.clip_loaded(clip_b) && s.clips_unreadable.contains(&clip_b));
        // Another open tries again.
        crate::decode::write_wav(&dir.join(AUDIO_DIR).join("b.wav"), s.sample_rate() as u32, &[vec![0.5; LEN]]).unwrap();
        s.open(&dir).unwrap();
        s.load_clips([clip_b]);
        assert!(s.clip_loaded(clip_b));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
