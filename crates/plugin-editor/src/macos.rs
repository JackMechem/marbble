//! Plug-in editor windows on macOS: an NSWindow whose content view the plug-in's NSView is added
//! to. Plug-ins drive their own timers through the main run loop, so unlike Linux there is no
//! event loop to give them. AppKit may only be touched on the main thread; `close`, `raise` and
//! the rest hop there when called from elsewhere. Keys the plug-in doesn't use go to the host
//! (`keys::set_key_forwarder`).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSEvent, NSEventModifierFlags, NSFloatingWindowLevel, NSResponder, NSWindow, NSWindowCollectionBehavior, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSPoint, NSRect, NSSize, NSString};

use crate::{Parent, View};

/// Moves a main-thread-only value through `exec_async` to the main thread, where it is used
/// and dropped.
struct MainOnly<T>(T);
unsafe impl<T> Send for MainOnly<T> {}

/// Runs `f` now on the main thread, or queues it there. Queued rather than waited for: the
/// caller may hold the session lock the main thread is waiting on.
fn on_main(f: impl FnOnce() + 'static) {
    if MainThreadMarker::new().is_some() {
        f();
    } else {
        let f = MainOnly(f);
        DispatchQueue::main().exec_async(move || {
            let f = f;
            (f.0)()
        });
    }
}

static HIDE_ON_DEACTIVATE: AtomicBool = AtomicBool::new(true);

/// Whether editor windows opened from now on hide while another app is active (the default).
/// The editor example turns it off: launched from a terminal, macOS may never make it the
/// active app, and its windows would then stay hidden.
pub fn set_hide_on_deactivate(on: bool) {
    HIDE_ON_DEACTIVATE.store(on, Ordering::Relaxed);
}

thread_local! {
    /// Where the next editor's top-left corner goes, so several editors don't stack exactly.
    static CASCADE: Cell<NSPoint> = const { Cell::new(NSPoint::new(0.0, 0.0)) };
    static OPEN: Cell<usize> = const { Cell::new(0) };
}

struct DelegateIvars {
    /// The plug-in's GUI, until `close` takes it out; late window callbacks then leave it alone.
    view: RefCell<Option<Arc<dyn View>>>,
    resizable: bool,
    /// The content size (points) the view last heard about, from a resize it asked for or a user
    /// resize.
    size: Cell<(f64, f64)>,
    /// The user closed the window (the red button only orders it out: not released on close).
    closed: Cell<bool>,
    /// The plug-in mustn't be called for now (`Editor::set_quiet`): the window still resizes,
    /// but the view isn't asked about it or told, and `size` stays what it last heard.
    quiet: AtomicBool,
}

impl DelegateIvars {
    /// The view, if a window callback may call into it now: a clone, so no borrow of the cell is
    /// held while the plug-in may call back.
    fn callable(&self) -> Option<Arc<dyn View>> {
        if self.quiet.load(Ordering::Relaxed) {
            return None;
        }
        self.view.borrow().clone()
    }
}

/// The window's content size, in whole points.
fn content_size(window: &NSWindow) -> (f64, f64) {
    let s = window.contentRectForFrameRect(window.frame()).size;
    (s.width.round(), s.height.round())
}

/// A size in whole points, as views take it.
fn points((w, h): (f64, f64)) -> (u32, u32) {
    (w.max(1.0) as u32, h.max(1.0) as u32)
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and WindowDelegate doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MarbblePlugInWindowDelegate"]
    #[ivars = DelegateIvars]
    struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        // A delegate rather than polling `isVisible`: hidesOnDeactivate also orders the window
        // out while another app is active.
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            self.ivars().closed.set(true);
        }

        /// Lets the plug-in constrain a user resize while it happens (`View::constrain`).
        #[unsafe(method(windowWillResize:toSize:))]
        fn window_will_resize(&self, sender: &NSWindow, frame_size: NSSize) -> NSSize {
            let iv = self.ivars();
            let Some(view) = iv.callable().filter(|_| iv.resizable) else { return frame_size };
            let content = sender.contentRectForFrameRect(NSRect::new(NSPoint::new(0.0, 0.0), frame_size)).size;
            let Some((w, h)) = view.constrain(points((content.width.round(), content.height.round()))) else { return frame_size };
            let c = NSSize::new(w.max(1) as f64, h.max(1) as f64);
            sender.frameRectForContentRect(NSRect::new(NSPoint::new(0.0, 0.0), c)).size
        }

        /// Tells the view about user resizes. A resize the view asked for records its own size
        /// first, so it isn't passed back.
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, n: &NSNotification) {
            let iv = self.ivars();
            let Some(view) = iv.callable().filter(|_| iv.resizable) else { return };
            let Some(window) = n.object().and_then(|o| o.downcast::<NSWindow>().ok()) else { return };
            let size = content_size(&window);
            if iv.size.get() == size {
                return;
            }
            iv.size.set(size);
            view.set_size(points(size));
        }
    }
);

impl WindowDelegate {
    fn new(mtm: MainThreadMarker, ivars: DelegateIvars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

/// Resizes the window's content to `size`, keeping its top-left corner where it is (Cocoa frames
/// grow from the bottom-left), then tells the view if `tell` (it asked for the size, and expects
/// to hear it) or notes that it knows. While quiet the window still resizes, and the view hears
/// about it when quiet ends (`catch_up`).
fn apply_resize(window: &NSWindow, delegate: &WindowDelegate, size: (u32, u32), tell: bool) {
    let iv = delegate.ivars();
    let Some(view) = iv.view.borrow().clone() else { return };
    let quiet = iv.quiet.load(Ordering::Relaxed);
    let (w, h) = (size.0.max(1) as f64, size.1.max(1) as f64);
    if !quiet {
        iv.size.set((w, h));
    }
    let old = window.frame();
    let mut frame = window.frameRectForContentRect(NSRect::new(old.origin, NSSize::new(w, h)));
    frame.origin.y = old.origin.y + old.size.height - frame.size.height;
    window.setFrame_display(frame, true);
    if !quiet && tell {
        view.set_size(points((w, h)));
    }
}

/// Once quiet ends, tells the view the size the window took meanwhile (by the user, or by a
/// resize the view asked for while quiet), within its constraints for a user-resizable one.
fn catch_up(window: &NSWindow, delegate: &WindowDelegate) {
    let iv = delegate.ivars();
    let Some(view) = iv.callable() else { return };
    let now = content_size(window);
    if iv.size.get() == now {
        return;
    }
    if iv.resizable {
        if let Some(c) = view.constrain(points(now)) {
            if (c.0.max(1) as f64, c.1.max(1) as f64) != now {
                // `apply_resize` records the size and tells the view; the window callbacks it
                // causes see the size already known.
                apply_resize(window, delegate, c, true);
                return;
            }
        }
    }
    iv.size.set(now);
    view.set_size(points(now));
}

/// What a view holds on to its window by: how it asks for a new size.
#[derive(Clone)]
pub struct Frame {
    window: Retained<NSWindow>,
    delegate: Retained<WindowDelegate>,
}

// Retaining and releasing are thread-safe; AppKit is only touched on the main thread (`on_main`).
unsafe impl Send for Frame {}
unsafe impl Sync for Frame {}

impl Frame {
    /// The view asks for `size` (VST3's `resizeView`): the window takes it, and the view is told
    /// (`View::set_size`). From any thread: elsewhere than the main one, it happens there later.
    pub fn resize(&self, size: (u32, u32)) {
        if MainThreadMarker::new().is_some() {
            apply_resize(&self.window, &self.delegate, size, true);
        } else {
            let (w, d) = (self.window.clone(), self.delegate.clone());
            on_main(move || apply_resize(&w, &d, size, true));
        }
    }

    /// Sizes the window to `size` for a view that has taken that size already. From any thread.
    pub fn fit(&self, size: (u32, u32)) {
        let (w, d) = (self.window.clone(), self.delegate.clone());
        on_main(move || apply_resize(&w, &d, size, false));
    }
}

pub struct Editor {
    window: Retained<NSWindow>,
    delegate: Retained<WindowDelegate>,
}

impl Editor {
    /// Opens where macOS places it (`at` is for Linux, where the window manager may not).
    pub fn open_at(view: impl View + 'static, title: &str, _at: Option<(i16, i16)>) -> Result<Editor, String> {
        Self::open(view, title)
    }

    /// Must be called on the main thread; elsewhere it fails rather than blocking on it.
    pub fn open(view: impl View + 'static, title: &str) -> Result<Editor, String> {
        let mtm = MainThreadMarker::new().ok_or("plug-in editors can only be opened on the main thread")?;
        // Some of the AppKit calls here are unsafe in the bindings.
        unsafe {
            let view: Arc<dyn View> = Arc::new(view);
            // Sizes on macOS are in points, the same units as AppKit's. HiDPI is the view's own
            // business here: it reads its window's backingScaleFactor, so the host never tells it a
            // scale (VST3's `IPlugViewContentScaleSupport` is for platforms without one, and CLAP's
            // `set_scale` isn't for Cocoa).
            let (w, h) = view.size().map_or((64.0, 64.0), |(w, h)| (w.max(64) as f64, h.max(64) as f64));
            let resizable = view.can_resize();

            let mut style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
            if resizable {
                style |= NSWindowStyleMask::Resizable;
            }
            let window = PlugInWindow::create(mtm, NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h)), style);
            // We own the window through the Retained; the close button just orders it out.
            window.setReleasedWhenClosed(false);
            window.setTitle(&NSString::from_str(title));
            // Like Logic's plug-in windows: floating above the project windows, hidden while
            // another app is active so they don't cover it (switching between Marbble's own
            // windows keeps the app active, so that doesn't hide them), and shown over a full-screen
            // main window.
            window.setLevel(NSFloatingWindowLevel);
            window.setHidesOnDeactivate(HIDE_ON_DEACTIVATE.load(Ordering::Relaxed));
            window.setCollectionBehavior(NSWindowCollectionBehavior::FullScreenAuxiliary);
            // The first editor is centred; while it is open, the next ones cascade from it.
            window.center();
            let from = if OPEN.get() == 0 { NSPoint::new(0.0, 0.0) } else { CASCADE.get() };
            CASCADE.set(window.cascadeTopLeftFromPoint(from));

            // Before the view has a frame or the window a delegate, which this return would leave.
            let content = window.contentView().ok_or("editor window has no content view")?;
            let delegate = WindowDelegate::new(
                mtm,
                DelegateIvars {
                    view: RefCell::new(Some(view.clone())),
                    resizable,
                    size: Cell::new((w, h)),
                    closed: Cell::new(false),
                    quiet: AtomicBool::new(false),
                },
            );
            window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            let frame = Frame { window: window.clone(), delegate: delegate.clone() };
            if let Err(e) = view.attach(Parent::NsView(Retained::as_ptr(&content) as *mut c_void), frame) {
                let view = delegate.ivars().view.borrow_mut().take();
                window.setDelegate(None);
                window.close();
                drop(view);
                return Err(e);
            }
            window.makeKeyAndOrderFront(None);
            OPEN.set(OPEN.get() + 1);
            Ok(Editor { window, delegate })
        }
    }

    /// Returns true once the user closed the window. Resizes are handled as they happen.
    pub fn pump(&mut self) -> bool {
        MainThreadMarker::new().is_some() && self.delegate.ivars().closed.get()
    }

    /// While `on`, nothing of the host's calls into the plug-in: the window can still be resized
    /// (by the user, or by the plug-in asking), but the view isn't asked about it (`constrain`) or
    /// told (`set_size`) until quiet ends, when it hears the size the window has then. Its own
    /// timers and its view's events are AppKit's, not the host's, and still reach it.
    pub fn set_quiet(&self, on: bool) {
        self.delegate.ivars().quiet.store(on, Ordering::Relaxed);
        if !on {
            let (w, d) = (MainOnly(self.window.clone()), MainOnly(self.delegate.clone()));
            on_main(move || {
                let (w, d) = (w, d);
                // Not if quiet started again before this ran.
                if !d.0.ivars().quiet.load(Ordering::Relaxed) {
                    catch_up(&w.0, &d.0);
                }
            });
        }
    }

    /// Sizes the window to `size` for a view that has taken that size already (a CLAP plug-in's
    /// `request_resize`).
    pub fn fit(&self, size: (u32, u32)) {
        let (w, d) = (MainOnly(self.window.clone()), MainOnly(self.delegate.clone()));
        on_main(move || {
            let (w, d) = (w, d);
            apply_resize(&w.0, &d.0, size, false);
        });
    }

    /// Takes the window off the screen, without a word to the plug-in.
    pub fn hide(&self) {
        let w = MainOnly(self.window.clone());
        on_main(move || {
            let w = w;
            w.0.orderOut(None);
        });
    }

    pub fn raise(&self) {
        let w = MainOnly(self.window.clone());
        on_main(move || {
            let w = w;
            if w.0.isMiniaturized() {
                w.0.deminiaturize(None);
            }
            w.0.makeKeyAndOrderFront(None);
        });
    }

    /// Closes the editor on the main thread (later, if called from another thread), then drops
    /// `keep_alive`: the plug-in mustn't be terminated before its view is removed.
    pub fn close<K: Send + 'static>(self, keep_alive: K) {
        let this = MainOnly(self);
        on_main(move || {
            let this = this;
            this.0.close_now();
            drop(keep_alive);
        });
    }

    fn close_now(self) {
        let view = self.delegate.ivars().view.borrow_mut().take();
        if let Some(v) = &view {
            v.detach();
        }
        self.window.setDelegate(None);
        self.window.close();
        OPEN.set(OPEN.get().saturating_sub(1));
        drop(view);
    }
}

// ---- keys the plug-in doesn't use -----------------------------------------------------------

define_class!(
    /// The editor window. A key the plug-in's view doesn't handle travels up the responder
    /// chain to here (its text fields keep theirs), and goes to the host's key forwarder rather
    /// than beeping. ⌘ shortcuts of the main menu never get here: the menu takes them first.
    // SAFETY: NSWindow may be subclassed; PlugInWindow has no ivars and doesn't implement Drop.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MarbblePlugInWindow"]
    struct PlugInWindow;

    impl PlugInWindow {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !forward_key(event, false) {
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }

        /// Releases too, for Musical Typing.
        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            if !forward_key(event, true) {
                unsafe {
                    let _: () = msg_send![super(self), keyUp: event];
                }
            }
        }
    }
);

impl PlugInWindow {
    /// A new window of this class, as the `NSWindow` the editor code works with.
    fn create(mtm: MainThreadMarker, content: NSRect, style: NSWindowStyleMask) -> Retained<NSWindow> {
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> =
            unsafe { msg_send![super(this), initWithContentRect: content, styleMask: style, backing: NSBackingStoreType::Buffered, defer: false] };
        Retained::into_super(this)
    }
}

/// Sends a key event to the forwarder; false if there is none or the key has no name.
fn forward_key(event: &NSEvent, up: bool) -> bool {
    if !crate::keys::has_forwarder() {
        return false;
    }
    let chars = event.charactersIgnoringModifiers().map(|s| s.to_string()).unwrap_or_default();
    let Some((key, code)) = crate::keys::mac_key(event.keyCode(), &chars) else { return false };
    let flags = event.modifierFlags();
    crate::keys::forward(crate::keys::KeyInfo {
        up,
        key,
        code,
        shift_key: flags.contains(NSEventModifierFlags::Shift),
        ctrl_key: flags.contains(NSEventModifierFlags::Control),
        alt_key: flags.contains(NSEventModifierFlags::Option),
        meta_key: flags.contains(NSEventModifierFlags::Command),
        repeat: !up && event.isARepeat(),
    })
}
