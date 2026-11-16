//! The `IRunLoop` a VST3 editor registers its file descriptors and timers with on Linux, run on
//! the app's main loop (`plugin_editor::EventLoop`). Timers wait while the editor is quiet; file
//! descriptors' handlers still run, as their sockets must be read.

use plugin_editor::{event_loop, gl_isolated, FdEvents};
use vst3::Steinberg::Linux::*;
use vst3::Steinberg::*;
use vst3::{ComPtr, ComRef};

use super::PlugFrame;

impl PlugFrame {
    /// Removes every source the view registered, which it should have done itself.
    pub(super) fn forget_sources(&self) {
        if let Some(l) = event_loop() {
            for (_, ids) in self.sources.lock().drain() {
                for id in ids {
                    l.remove(id);
                }
            }
        }
    }
}

impl IRunLoopTrait for PlugFrame {
    unsafe fn registerEventHandler(&self, handler: *mut IEventHandler, fd: FileDescriptor) -> tresult {
        let (Some(l), Some(h)) = (event_loop(), ComRef::from_raw(handler)) else { return kResultFalse };
        let h: ComPtr<IEventHandler> = h.to_com_ptr();
        let Some(id) = l.add_fd(fd, FdEvents::READ, Box::new(move |_| gl_isolated(|| h.onFDIsSet(fd)))) else { return kResultFalse };
        self.sources.lock().entry(handler as usize).or_default().push(id);
        kResultOk
    }

    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> tresult {
        if let (Some(l), Some(ids)) = (event_loop(), self.sources.lock().remove(&(handler as usize))) {
            for id in ids {
                l.remove(id);
            }
        }
        kResultOk
    }

    unsafe fn registerTimer(&self, handler: *mut ITimerHandler, ms: TimerInterval) -> tresult {
        let (Some(l), Some(h)) = (event_loop(), ComRef::from_raw(handler)) else { return kResultFalse };
        let h: ComPtr<ITimerHandler> = h.to_com_ptr();
        let frame = self.frame.clone();
        let added = l.add_timer(
            ms.max(1),
            Box::new(move || {
                if !frame.quiet() {
                    gl_isolated(|| h.onTimer());
                }
            }),
        );
        let Some(id) = added else { return kResultFalse };
        self.sources.lock().entry(handler as usize).or_default().push(id);
        kResultOk
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> tresult {
        self.unregisterEventHandler(handler as *mut IEventHandler)
    }
}
