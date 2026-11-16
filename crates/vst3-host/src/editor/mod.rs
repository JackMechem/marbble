//! A VST3 editor: the plug-in's `IPlugView` as a `plugin_editor::View`, and the host's
//! `IPlugFrame` it asks for new sizes through (with, on Linux, the `IRunLoop` its file descriptors
//! and timers go through, `run_loop.rs`). The windows themselves are `plugin_editor`'s.

#[cfg(target_os = "linux")]
mod run_loop;

use std::ffi::c_void;

use parking_lot::Mutex;
use plugin_editor::{Editor, Frame, Parent, View};
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper};

#[cfg(target_os = "linux")]
const PLATFORM: (FIDString, &str) = (kPlatformTypeX11EmbedWindowID, "plugin editor does not support X11 embedding");
#[cfg(target_os = "macos")]
const PLATFORM: (FIDString, &str) = (kPlatformTypeNSView, "this plug-in's editor does not support macOS (NSView) embedding");
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const PLATFORM: (FIDString, &str) = (kPlatformTypeHWND, "this plug-in's editor does not support Windows (HWND) embedding");

/// Opens `view` in a window of its own, its top-left corner at `at` (Linux; near the middle when
/// None).
pub(crate) fn open(view: ComPtr<IPlugView>, title: &str, at: Option<(i16, i16)>) -> Result<Editor, String> {
    let view = plugin_editor::gl_isolated(|| Vst3View::new(view))?;
    Editor::open_at(view, title, at)
}

/// A VST3 plug-in's editor view.
struct Vst3View {
    view: ComPtr<IPlugView>,
    /// The frame it was given in `attach`, until `detach`.
    frame: Mutex<Option<ComWrapper<PlugFrame>>>,
}

impl Vst3View {
    fn new(view: ComPtr<IPlugView>) -> Result<Vst3View, String> {
        if unsafe { view.isPlatformTypeSupported(PLATFORM.0) } != kResultOk {
            return Err(PLATFORM.1.into());
        }
        Ok(Vst3View { view, frame: Mutex::new(None) })
    }
}

fn rect((w, h): (u32, u32)) -> ViewRect {
    ViewRect { left: 0, top: 0, right: w as i32, bottom: h as i32 }
}

fn rect_size(r: &ViewRect) -> (u32, u32) {
    ((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32)
}

impl View for Vst3View {
    /// Through `IPlugViewContentScaleSupport`, which only views that draw at a content scale have.
    fn set_scale(&self, scale: f64) -> bool {
        let Some(s) = self.view.cast::<IPlugViewContentScaleSupport>() else { return false };
        let r = unsafe { s.setContentScaleFactor(scale as f32) };
        log::info!("VST3 editor told scale {scale}; it answered {r}");
        true
    }

    fn size(&self) -> Option<(u32, u32)> {
        let mut r: ViewRect = unsafe { std::mem::zeroed() };
        (unsafe { self.view.getSize(&mut r) } == kResultOk).then(|| ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32))
    }

    fn can_resize(&self) -> bool {
        unsafe { self.view.canResize() == kResultTrue }
    }

    fn constrain(&self, size: (u32, u32)) -> Option<(u32, u32)> {
        let mut r = rect(size);
        (unsafe { self.view.checkSizeConstraint(&mut r) } == kResultOk).then(|| rect_size(&r))
    }

    fn set_size(&self, size: (u32, u32)) {
        let mut r = rect(size);
        unsafe {
            self.view.onSize(&mut r);
        }
    }

    fn attach(&self, parent: Parent, frame: Frame) -> Result<(), String> {
        let frame = ComWrapper::new(PlugFrame::new(frame));
        let parent = match parent {
            Parent::X11(id) => id as usize as *mut c_void,
            Parent::NsView(v) => v,
        };
        unsafe {
            self.view.setFrame(frame.to_com_ptr::<IPlugFrame>().unwrap().as_ptr());
            if self.view.attached(parent, PLATFORM.0) != kResultOk {
                self.view.setFrame(std::ptr::null_mut());
                return Err("plugin refused to attach its editor".into());
            }
        }
        *self.frame.lock() = Some(frame);
        Ok(())
    }

    fn detach(&self) {
        unsafe {
            self.view.removed();
            self.view.setFrame(std::ptr::null_mut());
        }
        let frame = self.frame.lock().take();
        #[cfg(target_os = "linux")]
        if let Some(f) = &frame {
            f.forget_sources();
        }
        drop(frame);
    }
}

/// The host's side of an open editor: `IPlugFrame`, and on Linux the run loop (`run_loop.rs`).
pub struct PlugFrame {
    frame: Frame,
    /// Handler pointer -> the run loop's source ids.
    #[cfg(target_os = "linux")]
    sources: Mutex<std::collections::HashMap<usize, Vec<u64>>>,
}

unsafe impl Send for PlugFrame {}
unsafe impl Sync for PlugFrame {}

#[cfg(target_os = "linux")]
impl Class for PlugFrame {
    type Interfaces = (IPlugFrame, Linux::IRunLoop);
}

#[cfg(not(target_os = "linux"))]
impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

impl PlugFrame {
    fn new(frame: Frame) -> PlugFrame {
        PlugFrame {
            frame,
            #[cfg(target_os = "linux")]
            sources: Mutex::new(Default::default()),
        }
    }
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() || ComRef::from_raw(view).is_none() {
            return kInvalidArgument;
        }
        self.frame.resize(rect_size(&*new_size));
        kResultOk
    }
}
