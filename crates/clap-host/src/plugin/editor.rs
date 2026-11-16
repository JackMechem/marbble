//! The controller's editor window: the plug-in's GUI (`crate::gui`) in a `plugin_editor::Editor`,
//! and what the plug-in asks of it (`GuiRequests`), answered in `idle`. While the plug-in isn't
//! responding, the window is quiet, and closing it only hides it until the instance is gone, as
//! for a VST3.

use plugin_editor::Editor;

use super::*;
use crate::gui::ClapGui;

impl ClapController {
    pub(super) fn open_window(&mut self, title: &str) -> Result<(), String> {
        if self.unresponsive {
            return Err(format!("{} isn't responding", self.desc.name));
        }
        if let Some(e) = &self.editor {
            e.raise();
            return Ok(());
        }
        if !crate::host::is_main() {
            return Err("plug-in windows open on the main thread".into());
        }
        self.shared.requests().gui.clear();
        self.editor = Some(self.make_editor(title, None)?);
        Ok(())
    }

    fn make_editor(&self, title: &str, at: Option<(i16, i16)>) -> Result<Editor, String> {
        let gui = plugin_editor::gl_isolated(|| unsafe { ClapGui::create(self.shared.plugin, self.shared.gui, &self.desc.name) })?;
        Editor::open_at(gui, title, at)
    }

    pub(super) fn close_window(&mut self) {
        let Some(e) = self.editor.take() else { return };
        if self.unresponsive {
            // Destroying its GUI would wait on the plug-in.
            plugin_editor::strand(e, self.shared.clone());
            return;
        }
        e.close(self.shared.clone());
    }

    /// Opens the editor again where it is, for a GUI that takes a new display scale only when it
    /// opens (`Editor::wants_reopen`).
    #[cfg(target_os = "linux")]
    fn reopen_window(&mut self) {
        let Some(e) = self.editor.take() else { return };
        let (title, at) = (e.title().to_string(), e.position());
        e.close(self.shared.clone());
        match self.make_editor(&title, at) {
            Ok(e) => self.editor = Some(e),
            Err(err) => log::warn!("{}: couldn't reopen its window at the new scale: {err}", self.desc.name),
        }
    }

    /// The window's own events (a close), and what the plug-in asked of it, on the main thread.
    /// While the plug-in isn't responding, only a close is seen.
    pub(super) fn window_idle(&mut self) {
        let closed = self.editor.as_mut().is_some_and(|e| e.pump());
        if self.unresponsive {
            if closed {
                self.close_window();
            }
            return;
        }
        let shared = self.shared.clone();
        let asked = &shared.requests().gui;
        if asked.closed.swap(false, Ordering::AcqRel) || closed {
            self.close_window();
        }
        let Some(e) = &self.editor else { return };
        if let Some(size) = asked.take_resize() {
            e.fit(size);
        }
        if asked.show.swap(false, Ordering::AcqRel) {
            e.raise();
        }
        if asked.hide.swap(false, Ordering::AcqRel) {
            e.hide();
        }
        #[cfg(target_os = "linux")]
        if e.wants_reopen() {
            self.reopen_window();
        }
    }
}

impl Drop for ClapController {
    fn drop(&mut self) {
        // Its GUI goes before the plug-in can.
        self.close_window();
    }
}
