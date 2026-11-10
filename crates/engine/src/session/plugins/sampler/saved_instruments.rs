//! Sampler instruments saved with their samples, as presets: saving one and loading it back.

use super::*;

impl Session {
    /// Saves the sampler on `track` as a folder under the user's Music folder (`presets::bundle_root`,
    /// created on first save), with a copy of every sample it plays, so any project can load it.
    /// It shows in the instrument's preset menu. Returns the name it was saved under.
    pub(crate) fn sampler_save_instrument(&self, track: Id, name: &str, overwrite: bool) -> Result<String> {
        let name = presets::clean_name(name)?;
        let slot = self.sampler_slot(track)?;
        let dest = presets::bundle_dir(&name);
        if dest.exists() && !overwrite {
            return Err(anyhow!("A preset named \"{name}\" already exists"));
        }
        let mut state = SamplerState::parse(slot.state.as_deref());
        // Built beside it, so a failure halfway leaves the old one as it was.
        let root = presets::bundle_root();
        let tmp = root.join(format!(".saving {name}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(SAMPLES_DIR))?;
        adopt_zones(&mut state, &self.dir, &tmp, Path::new(""))?;
        let p = Preset {
            name: name.clone(),
            effect: SAMPLER.into(),
            version: 1,
            values: self.builtin_params(slot.uid),
            legacy_eq: None,
            state: Some(state.to_json()),
            base: None,
        };
        std::fs::write(tmp.join(presets::BUNDLE_DOC), serde_json::to_string_pretty(&p)?)?;
        let old = root.join(format!(".replaced {name}"));
        let _ = std::fs::remove_dir_all(&old);
        if dest.exists() {
            std::fs::rename(&dest, &old)?;
        }
        std::fs::rename(&tmp, &dest)?;
        let _ = std::fs::remove_dir_all(&old);
        Ok(name)
    }

    /// Takes a preset's zones, with their files copied into this project.
    pub(crate) fn sampler_apply_preset(&mut self, track: Id, state: &str, base: Option<&Path>) -> Result<()> {
        self.sampler_slot(track)?;
        let mut state = SamplerState::parse(Some(state));
        if let Some(base) = base {
            let sub = base.file_name().map(PathBuf::from).unwrap_or_default();
            adopt_zones(&mut state, base, &self.dir, &sub)?;
        }
        self.sampler_store(track, &state)
    }
}
