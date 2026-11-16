//! Editors closed while a thread was stuck in their plug-in: hidden but not told, as telling the
//! GUI it's going would wait on the plug-in. Each is closed for real once its plug-in instance is
//! gone (`close_stranded`).

use std::cell::RefCell;
use std::sync::Arc;

use crate::Editor;

/// A hidden editor and the instance it waits for.
trait Stranded {
    /// The instance's other holders are gone: only this one keeps it.
    fn instance_gone(&self) -> bool;
    fn close(self: Box<Self>);
}

struct Waiting<K> {
    editor: Editor,
    instance: Arc<K>,
}

impl<K: Send + Sync + 'static> Stranded for Waiting<K> {
    fn instance_gone(&self) -> bool {
        Arc::strong_count(&self.instance) == 1
    }

    fn close(self: Box<Self>) {
        self.editor.close(self.instance);
    }
}

thread_local! {
    static STRANDED: RefCell<Vec<Box<dyn Stranded>>> = const { RefCell::new(Vec::new()) };
}

/// Hides `editor` without a word to its plug-in, and keeps it until `instance` (what keeps the
/// plug-in alive) has no other holder: its processor came back from the thread stuck in it and was
/// dropped, and its controller was too. On the main thread.
pub fn strand<K: Send + Sync + 'static>(editor: Editor, instance: Arc<K>) {
    editor.hide();
    STRANDED.with(|s| s.borrow_mut().push(Box::new(Waiting { editor, instance })));
}

/// Closes the stranded editors whose instance is gone. Call periodically on the main thread.
pub fn close_stranded() {
    let done: Vec<_> = STRANDED.with(|s| {
        let mut s = s.borrow_mut();
        let (done, keep) = std::mem::take(&mut *s).into_iter().partition(|e| e.instance_gone());
        *s = keep;
        done
    });
    for e in done {
        e.close();
    }
}
