//! Plug-in editor windows on Linux: a top-level X11 window the plug-in's GUI embeds into, drawn at
//! the display's scale and following changes of it, with the plug-ins' file descriptors and timers
//! on the app's main loop (`event_loop.rs`).

mod event_loop;
mod gl;
mod libs;
pub mod poll_loop;
mod scale_watch;
mod x11_scale;

pub use event_loop::{event_loop, set_event_loop, EventLoop, FdEvents};
pub use gl::{gl_isolated, set_gl_isolation};

use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event as XEvent;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::{Parent, View};
use scale_watch::ScaleWatch;

struct Window {
    conn: RustConnection,
    id: u32,
    size: Mutex<(u32, u32)>,
    /// The window size the view last heard (`set_size`), which `size` moves past while quiet.
    heard: Mutex<(u32, u32)>,
    /// The view said it can be resized by the user; otherwise the WM hints pin its size.
    resizable: bool,
    /// The plug-in mustn't be called for now (`Editor::set_quiet`).
    quiet: AtomicBool,
    screen: usize,
    /// The display's scale the editor is drawn at (`x11_scale`), followed while it is open.
    scale: Mutex<f64>,
    /// Tells a change of the display's scale settings; None if the server wouldn't say.
    watch: Option<ScaleWatch>,
    /// The display's scale settings changed, and the view hasn't heard yet.
    scale_due: AtomicBool,
    /// The view didn't take a new scale while open: reopening it applies it (`Editor::wants_reopen`).
    reopen: AtomicBool,
    /// The plug-in's GUI, until the editor closes.
    view: Mutex<Option<Arc<dyn View>>>,
}

// The view is called on the main thread, and on the thread a plug-in asks for a new size from
// (`Frame::resize`), as the plug-in's format allows; the rest is the X connection, which is
// thread-safe, and locks and atomics.
unsafe impl Send for Window {}
unsafe impl Sync for Window {}

impl Window {
    fn view(&self) -> Option<Arc<dyn View>> {
        self.view.lock().clone()
    }

    /// WM_NORMAL_HINTS pinning a fixed-size editor to `w` x `h` (PMinSize | PMaxSize), plus
    /// PPosition when `at` gives the position.
    fn pin_size(&self, w: u32, h: u32, at: Option<(i16, i16)>) {
        let (pos, x, y) = match at {
            Some((x, y)) => (4, x as u32, y as u32),
            None => (0, 0, 0),
        };
        let hints: [u32; 18] = [pos | 16 | 32, x, y, w, h, w, h, w, h, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let _ = self.conn.change_property32(PropMode::REPLACE, self.id, AtomEnum::WM_NORMAL_HINTS, AtomEnum::WM_SIZE_HINTS, &hints);
    }

    /// Sizes the window to `w` x `h` for a view that knows it's that size.
    fn fit(&self, w: u32, h: u32) {
        *self.size.lock() = (w, h);
        *self.heard.lock() = (w, h);
        // A fixed-size editor's hints hold it at its old size; move them along, or the WM
        // refuses the new one.
        if !self.resizable {
            self.pin_size(w, h, None);
        }
        let _ = self.conn.configure_window(self.id, &ConfigureWindowAux::new().width(w).height(h));
        let _ = self.conn.flush();
    }
}

/// What a view holds on to its window by: how it asks for a new size, and whether the host's calls
/// into it are held back. Once the editor is closed, it does nothing.
#[derive(Clone)]
pub struct Frame(Weak<Window>);

impl Frame {
    /// The view asks for `size` (VST3's `resizeView`): the window takes it, and the view is told
    /// (`View::set_size`). From any thread.
    pub fn resize(&self, size: (u32, u32)) {
        let Some(win) = self.0.upgrade() else { return };
        let size = (size.0.max(1), size.1.max(1));
        win.fit(size.0, size.1);
        if let Some(v) = win.view() {
            v.set_size(size);
        }
    }

    /// Sizes the window to `size` for a view that has taken that size already.
    pub fn fit(&self, size: (u32, u32)) {
        if let Some(win) = self.0.upgrade() {
            win.fit(size.0.max(1), size.1.max(1));
        }
    }

    /// Whether the plug-in mustn't be called for now (`Editor::set_quiet`): its timers wait.
    pub fn quiet(&self) -> bool {
        self.0.upgrade().is_some_and(|w| w.quiet.load(Ordering::Relaxed))
    }
}

/// Tells a view the display's scale (see `x11_scale`), before its size is asked for: it answers
/// `size` in pixels at that scale, which is what the window is sized from. Nothing is said at a
/// scale of 1, so plug-ins that read the display's scale themselves (JUCE, DPF) still do. X11 has
/// one scale for the whole display, so it doesn't change with the monitor; a change of the setting
/// while the editor is open is followed (`rescale`). Returns the scale.
fn set_content_scale(view: &dyn View, conn: &RustConnection, screen_num: usize) -> f64 {
    let (scale, source) = x11_scale::choose(&x11_scale::read(conn, screen_num));
    if scale != 1.0 {
        tell_scale(view, scale, source);
    }
    scale
}

/// Tells the view `scale`, if it can be told. Returns whether it could.
fn tell_scale(view: &dyn View, scale: f64, source: &str) -> bool {
    let told = view.set_scale(scale);
    if told {
        log::info!("plug-in editor scale {scale} (from {source})");
    } else {
        log::info!("plug-in editor scale {scale} (from {source}); the plug-in can't be told, it may scale itself");
    }
    told
}

/// A view's size for its window: at least 64 x 64, as some say 0 before they're shown.
fn window_size(view: &dyn View) -> Option<(u32, u32)> {
    view.size().map(|(w, h)| (w.clamp(64, u16::MAX as u32), h.clamp(64, u16::MAX as u32)))
}

/// Follows a change of the display's scale settings in an open editor, as reopening it would
/// apply it: the view hears the new scale, and the window takes the size it then asks for (a view
/// may already have asked, with `Frame::resize`). A view that can't be told, or that says yes but
/// keeps its size (DPF's take the scale only as they open), is opened again instead. On the main
/// thread, not while quiet.
fn rescale(win: &Window) {
    let (now, source) = x11_scale::choose(&x11_scale::read(&win.conn, win.screen));
    let Some(scale) = x11_scale::change(*win.scale.lock(), now) else { return };
    *win.scale.lock() = scale;
    let Some(view) = win.view() else { return };
    let before = *win.size.lock();
    let asked = gl_isolated(|| {
        if !tell_scale(&*view, scale, source) {
            return None;
        }
        let size = window_size(&*view)?;
        if size != *win.size.lock() {
            win.fit(size.0, size.1);
            view.set_size(size);
        }
        Some(size)
    });
    if !x11_scale::followed(before, asked) {
        log::info!("plug-in editor kept its size at scale {scale}; opening it again");
        win.reopen.store(true, Ordering::Relaxed);
    }
}

/// An open plug-in editor in an X11 window of its own.
pub struct Editor {
    title: String,
    window: Arc<Window>,
    wm_delete: u32,
    closed: Arc<AtomicBool>,
    x_source: Option<u64>,
}

impl Editor {
    pub fn open(view: impl View + 'static, title: &str) -> Result<Editor, String> {
        Self::open_at(view, title, None)
    }

    /// Opens with the window's top-left corner at `at` on the screen; near the middle when None.
    pub fn open_at(view: impl View + 'static, title: &str, at: Option<(i16, i16)>) -> Result<Editor, String> {
        gl_isolated(|| Self::open_isolated(Arc::new(view), title, at))
    }

    fn open_isolated(view: Arc<dyn View>, title: &str, at: Option<(i16, i16)>) -> Result<Editor, String> {
        if gl::isolating() {
            // After the plug-in is loaded, which brings in its GL library.
            gl::find_glx();
        }
        let (conn, screen_num) = RustConnection::connect(None).map_err(|e| format!("X11: {e}"))?;
        // Before the scale is read, so a change straight after is heard.
        let watch = ScaleWatch::start(&conn, screen_num);
        let scale = set_content_scale(&*view, &conn, screen_num);
        let (w, h) = window_size(&*view).unwrap_or((64, 64));
        let (w, h) = (w as u16, h as u16);
        let resizable = view.can_resize();

        let screen = conn.setup().roots[screen_num].clone();
        let id = conn.generate_id().map_err(|e| e.to_string())?;
        // Open near the middle of the screen rather than under panels in the corner.
        let (x, y) = at.unwrap_or(((screen.width_in_pixels.saturating_sub(w) / 2) as i16, (screen.height_in_pixels.saturating_sub(h) / 3) as i16));
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            id,
            screen.root,
            x,
            y,
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().background_pixel(screen.black_pixel).event_mask(EventMask::STRUCTURE_NOTIFY),
        )
        .map_err(|e| e.to_string())?;

        let atom = |name: &str| conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom).unwrap_or(0);
        let wm_protocols = atom("WM_PROTOCOLS");
        let wm_delete = atom("WM_DELETE_WINDOW");
        let net_wm_name = atom("_NET_WM_NAME");
        let utf8 = atom("UTF8_STRING");
        let _ = conn.change_property32(PropMode::REPLACE, id, wm_protocols, AtomEnum::ATOM, &[wm_delete]);
        let _ = conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_NAME, AtomEnum::STRING, title.as_bytes());
        let _ = conn.change_property8(PropMode::REPLACE, id, net_wm_name, utf8, title.as_bytes());
        let _ = conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_CLASS, AtomEnum::STRING, b"marbble-plugin\0Marbble\0");
        let size = (w as u32, h as u32);
        let window = Arc::new(Window {
            conn,
            id,
            size: Mutex::new(size),
            heard: Mutex::new(size),
            resizable,
            quiet: AtomicBool::new(false),
            screen: screen_num,
            scale: Mutex::new(scale),
            watch,
            scale_due: AtomicBool::new(false),
            reopen: AtomicBool::new(false),
            view: Mutex::new(Some(view.clone())),
        });
        if resizable {
            // WM_NORMAL_HINTS: just PPosition.
            let hints: [u32; 18] = [4, x as u32, y as u32, w as u32, h as u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let _ = window.conn.change_property32(PropMode::REPLACE, id, AtomEnum::WM_NORMAL_HINTS, AtomEnum::WM_SIZE_HINTS, &hints);
        } else {
            window.pin_size(w as u32, h as u32, Some((x, y)));
        }
        window.conn.map_window(id).map_err(|e| e.to_string())?;
        window.conn.flush().map_err(|e| e.to_string())?;
        // Before `attach`: Xlib's default handler would exit on an X error made while the
        // plug-in builds its GUI.
        libs::install_x_error_handlers();
        if let Err(e) = view.attach(Parent::X11(id), Frame(Arc::downgrade(&window))) {
            window.view.lock().take();
            let _ = window.conn.destroy_window(id);
            let _ = window.conn.flush();
            return Err(e);
        }

        let closed = Arc::new(AtomicBool::new(false));
        let x_source = event_loop().and_then(|l| {
            // A resizable view hears about the user's resizes from here, and every view about the
            // display's scale. `close` removes the source before the window goes; the callback
            // can't run in between, as both are on the main thread.
            let (win, closed) = (window.clone(), closed.clone());
            let fd = window.conn.stream().as_raw_fd();
            l.add_fd(fd, FdEvents::READ, Box::new(move |_| drain_events(&win, wm_delete, &closed)))
        });

        Ok(Editor { title: title.to_string(), window, wm_delete, closed, x_source })
    }

    /// Processes pending window events. Returns true once the user closed the window.
    pub fn pump(&mut self) -> bool {
        drain_events(&self.window, self.wm_delete, &self.closed);
        self.closed.load(Ordering::Relaxed)
    }

    /// While `on`, nothing of the host's calls into the plug-in: a resize or a change of the
    /// display's scale isn't passed on, and its timers don't fire. Its own file descriptors' handlers
    /// still do, as their sockets must be read. When quiet ends, a resizable view hears the size the
    /// window took meanwhile, and the view a scale that changed. On the main thread.
    pub fn set_quiet(&self, on: bool) {
        let was = self.window.quiet.swap(on, Ordering::Relaxed);
        if was && !on && self.window.resizable {
            let now = *self.window.size.lock();
            tell_size(&self.window, now);
        }
        if was && !on && self.window.scale_due.load(Ordering::Relaxed) {
            drain_events(&self.window, self.wm_delete, &self.closed);
        }
    }

    /// The view didn't take a change of the display's scale while open, and opening it again
    /// would apply it.
    pub fn wants_reopen(&self) -> bool {
        self.window.reopen.load(Ordering::Relaxed)
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// Where the window is on the screen: its top-left corner, under the window manager's frame.
    pub fn position(&self) -> Option<(i16, i16)> {
        let root = self.window.conn.setup().roots[self.window.screen].root;
        let r = self.window.conn.translate_coordinates(self.window.id, root, 0, 0).ok()?.reply().ok()?;
        Some((r.dst_x, r.dst_y))
    }

    /// Sizes the window to `size` for a view that has taken that size already (a CLAP plug-in's
    /// `request_resize`).
    pub fn fit(&self, size: (u32, u32)) {
        self.window.fit(size.0.max(1), size.1.max(1));
    }

    /// Takes the window off the screen, without a word to the plug-in.
    pub fn hide(&self) {
        let _ = self.window.conn.unmap_window(self.window.id);
        let _ = self.window.conn.flush();
    }

    pub fn raise(&self) {
        let _ = self.window.conn.configure_window(self.window.id, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        let _ = self.window.conn.map_window(self.window.id);
        let _ = self.window.conn.flush();
    }

    /// Takes the view out of the window and closes it. `keep_alive` (the plug-in) is dropped after
    /// the view is.
    pub fn close<K: Send + 'static>(self, keep_alive: K) {
        let view = self.window.view.lock().take();
        if let Some(v) = &view {
            gl_isolated(|| v.detach());
        }
        if let (Some(l), Some(id)) = (event_loop(), self.x_source) {
            l.remove(id);
        }
        let _ = self.window.conn.destroy_window(self.window.id);
        let _ = self.window.conn.flush();
        drop(view);
        drop(keep_alive);
    }
}

/// Handles the editor connection's events: the window closed or resized (passed on to a
/// resizable view), and the display's scale settings changed. A quiet editor's view hears
/// nothing; a scale change waits for quiet to end.
fn drain_events(win: &Window, wm_delete: u32, closed: &AtomicBool) {
    loop {
        while let Ok(Some(ev)) = win.conn.poll_for_event() {
            if win.watch.as_ref().is_some_and(|w| w.note(&win.conn, &ev)) {
                win.scale_due.store(true, Ordering::Relaxed);
                continue;
            }
            match ev {
                XEvent::ClientMessage(e) if e.window == win.id && e.data.as_data32()[0] == wm_delete => {
                    closed.store(true, Ordering::Relaxed);
                }
                XEvent::ConfigureNotify(e) if e.window == win.id => {
                    let now = (e.width as u32, e.height as u32);
                    let mut size = win.size.lock();
                    if *size != now {
                        *size = now;
                        drop(size);
                        if win.resizable && !win.quiet.load(Ordering::Relaxed) {
                            tell_size(win, now);
                        }
                    }
                }
                _ => {}
            }
        }
        if win.quiet.load(Ordering::Relaxed) || !win.scale_due.swap(false, Ordering::Relaxed) {
            return;
        }
        // Reading the settings waits for the server's replies, behind which more events may
        // have come in: the loop takes those too.
        rescale(win);
    }
}

/// Tells a resizable view the window's size is now `now`, within its constraints, unless it
/// already knows.
fn tell_size(win: &Window, now: (u32, u32)) {
    if *win.heard.lock() == now {
        return;
    }
    let Some(v) = win.view() else { return };
    gl_isolated(|| {
        if let Some(size) = v.constrain(now) {
            *win.heard.lock() = now;
            v.set_size(size);
        }
    });
}
