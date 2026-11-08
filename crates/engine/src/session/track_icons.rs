//! Logic's Track Icon: the icon a track, aux or stack shows in its header, on its Mixer strip and
//! in the Inspector. `Track::icon` holds the user's choice by the UI's id for it; the icons, and
//! the default a track without a choice shows (from its name, else its kind), are the UI's (see
//! `src/trackIcons.ts`). The icon is saved, copied by Duplicate Track and kept by templates, but
//! isn't part of channel strip settings.

use super::*;

impl Session {
    /// Gives `ids` the icon `icon` (None: back to the default). Unknown tracks are skipped.
    pub fn set_track_icon(&mut self, ids: &[Id], icon: Option<String>) {
        let icon = icon.filter(|i| !i.is_empty());
        for t in self.project.tracks.iter_mut().filter(|t| ids.contains(&t.id)) {
            t.icon = icon.clone();
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    fn audio(s: &mut Session) -> Id {
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()
    }

    fn icon(s: &Session, id: Id) -> Option<&str> {
        s.project.track(id).unwrap().icon.as_deref()
    }

    #[test]
    fn new_tracks_have_none_and_older_projects_open_without_it() {
        let mut s = session();
        let a = audio(&mut s);
        assert_eq!(icon(&s, a), None);
        // Saved only once chosen, so a project without icons reads as before.
        let json = serde_json::to_value(&s.project).unwrap();
        assert!(json["tracks"][0].get("icon").is_none());
        let mut old = json.clone();
        old["tracks"][0].as_object_mut().unwrap().remove("icon");
        let back: Project = serde_json::from_value(old).unwrap();
        assert_eq!(back.track(a).unwrap().icon, None);
    }

    #[test]
    fn several_tracks_in_one_undo_step() {
        let mut s = session();
        let (a, b, c) = (audio(&mut s), audio(&mut s), audio(&mut s));
        s.checkpoint("Track Icon", None, false);
        s.set_track_icon(&[a, b], Some("kick".into()));
        assert_eq!((icon(&s, a), icon(&s, b), icon(&s, c)), (Some("kick"), Some("kick"), None));
        s.checkpoint("Track Icon", None, false);
        s.set_track_icon(&[b], None);
        assert_eq!(icon(&s, b), None);
        s.undo();
        assert_eq!(icon(&s, b), Some("kick"));
        s.undo();
        assert_eq!((icon(&s, a), icon(&s, b)), (None, None));
        s.redo();
        assert_eq!((icon(&s, a), icon(&s, b)), (Some("kick"), Some("kick")));
        // An empty id is no choice.
        s.set_track_icon(&[a], Some(String::new()));
        assert_eq!(icon(&s, a), None);
    }

    #[test]
    fn duplicates_templates_and_saved_projects_keep_it() {
        let mut s = session();
        let a = audio(&mut s);
        let stack = s.create_stack(&[a], true).unwrap();
        s.set_track_icon(&[a], Some("snare".into()));
        s.set_track_icon(&[stack], Some("kit".into()));
        for content in [true, false] {
            let copy = s.duplicate_tracks(&[a], content)[0];
            assert_eq!(icon(&s, copy), Some("snare"));
        }
        let copy = s.duplicate_tracks(&[stack], true)[0];
        assert_eq!(icon(&s, copy), Some("kit"));
        let json = serde_json::to_string(&s.project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.track(a).unwrap().icon.as_deref(), Some("snare"));

        let folder = std::env::temp_dir().join(format!("marbble-icon-templates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        s.save_template(&folder, "Drums", false).unwrap();
        let mut s2 = session();
        s2.new_from_template(&folder, "Drums").unwrap();
        assert_eq!(icon(&s2, a), Some("snare"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn channel_strip_settings_leave_it_alone() {
        let mut s = session();
        let (a, b) = (audio(&mut s), audio(&mut s));
        s.set_track_icon(&[a], Some("mic".into()));
        s.set_track_icon(&[b], Some("bass".into()));
        let st = s.copy_strip(a).unwrap();
        assert!(!serde_json::to_string(&st).unwrap().contains("mic"));
        s.paste_strip(&[b], &st).unwrap();
        s.reset_strip(&[b]).unwrap();
        assert_eq!(icon(&s, b), Some("bass"));
    }
}
