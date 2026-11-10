//! The sampler's files as projects move: copying the Samples folder with a project, warning of
//! missing files, and replacing a zone's file.

use super::*;

impl Session {
    /// Saving somewhere new: the Samples folder goes along whole, not just what's in use, because
    /// undo steps might refer to the rest.
    pub(crate) fn sampler_copy_samples(&self, target: &Path) -> Result<()> {
        fn copy_dir(from: &Path, to: &Path) -> Result<()> {
            let Ok(rd) = std::fs::read_dir(from) else { return Ok(()) };
            std::fs::create_dir_all(to)?;
            for e in rd.flatten() {
                let (src, dest) = (e.path(), to.join(e.file_name()));
                if src.is_dir() {
                    copy_dir(&src, &dest)?;
                } else if !dest.exists() {
                    std::fs::copy(&src, &dest).with_context(|| format!("copying {}", src.display()))?;
                }
            }
            Ok(())
        }
        copy_dir(&self.dir.join(SAMPLES_DIR), &target.join(SAMPLES_DIR))
    }

    /// After opening: says plainly which samples aren't there. The rest play.
    pub(crate) fn sampler_warn_missing(&mut self) {
        let mut names: Vec<String> = Vec::new();
        for t in &self.project.tracks {
            let Some(slot) = t.instrument.as_ref().filter(|s| s.plugin_id == SAMPLER) else { continue };
            for i in self.sampler.issues.get(&slot.uid).into_iter().flatten() {
                names.push(format!("{} on {} ({})", i.file, t.name, i.error.text()));
            }
        }
        if !names.is_empty() {
            let n = names.len();
            names.truncate(5);
            let more = if n > 5 { format!(" and {} more", n - 5) } else { String::new() };
            let text = format!("Samples that can't play: {}{more}. Everything else plays. Replace them in the sampler's panel.", names.join(", "));
            self.warning = Some(match self.warning.take() {
                Some(w) => format!("{w}\n{text}"),
                None => text,
            });
        }
    }

    /// Every zone that can't play and why, across the project.
    pub fn sampler_issues(&self, track: Id) -> Vec<(u64, String, String)> {
        let Ok(slot) = self.sampler_slot(track) else { return Vec::new() };
        self.sampler.issues.get(&slot.uid).into_iter().flatten().map(|i| (i.zone, i.file.clone(), i.error.text())).collect()
    }

    /// Points a zone at another file, copied into the project: mending a missing sample.
    pub fn sampler_replace_file(&mut self, track: Id, zone: u64, path: &Path) -> Result<()> {
        let mut state = self.sampler_state(track)?;
        let rel = adopt_file(&self.dir, Path::new(""), path)?;
        self.sampler.pool.get(&self.dir.join(&rel)).map_err(|e| anyhow!("{}", e.text()))?;
        let z = state.zones.iter_mut().find(|z| z.id == zone).ok_or_else(|| anyhow!("no such zone"))?;
        z.file = rel;
        // Its positions were for the other file.
        z.start = 0;
        z.end = 0;
        z.loop_on = false;
        z.loop_start = 0;
        z.loop_end = 0;
        self.sampler_store(track, &state)
    }
}
