//! Follows the display's scale while a plug-in editor is open on Linux. The X server tells each
//! editor's connection when a source `x11_scale` reads changes: `xrdb` setting `Xft.dpi` changes
//! the `RESOURCE_MANAGER` property of the root window, the XSETTINGS manager (the desktop's
//! settings daemon) changes `_XSETTINGS_SETTINGS` on its own window, and a manager that starts
//! announces itself with a `MANAGER` message on the root window. Nothing is polled: the events
//! arrive with the editor's other window events, on the main thread.

use std::sync::atomic::{AtomicU32, Ordering};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ChangeWindowAttributesAux, ConnectionExt as _, EventMask};
use x11rb::protocol::Event as XEvent;
use x11rb::rust_connection::RustConnection;

/// The windows and atoms the events name.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Names {
    root: u32,
    /// `RESOURCE_MANAGER`, the X resources (`Xft.dpi`).
    resources: u32,
    /// `_XSETTINGS_S<screen>`, the selection the XSETTINGS manager owns.
    selection: u32,
    /// `_XSETTINGS_SETTINGS`, the property holding its settings.
    settings: u32,
    /// `MANAGER`, the message a new manager sends.
    manager: u32,
}

/// What an event says about the display's scale.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Seen {
    /// Nothing.
    Nothing,
    /// A setting the scale is read from may have changed.
    Changed,
    /// A new XSETTINGS manager, whose window this is, took over.
    NewManager(u32),
    /// The XSETTINGS manager went away, and its settings with it.
    ManagerGone,
}

/// What `ev` says about the scale, `owner` being the XSETTINGS manager's window (0 for none).
fn classify(n: &Names, owner: u32, ev: &XEvent) -> Seen {
    match ev {
        XEvent::PropertyNotify(e) if (e.window == n.root && e.atom == n.resources) || (owner != 0 && e.window == owner && e.atom == n.settings) => {
            Seen::Changed
        }
        XEvent::ClientMessage(e) if e.window == n.root && e.type_ == n.manager && e.data.as_data32()[1] == n.selection => {
            Seen::NewManager(e.data.as_data32()[2])
        }
        XEvent::DestroyNotify(e) if owner != 0 && e.window == owner => Seen::ManagerGone,
        _ => Seen::Nothing,
    }
}

/// Watches the display's scale settings from one editor's connection.
pub(crate) struct ScaleWatch {
    names: Names,
    /// The XSETTINGS manager's window, 0 when there is none.
    owner: AtomicU32,
}

impl ScaleWatch {
    /// Asks the X server for the events that tell a change. Event masks belong to the connection
    /// that selects them, so this changes nothing for the desktop or other programs.
    pub(crate) fn start(conn: &RustConnection, screen_num: usize) -> Option<ScaleWatch> {
        let atom = |name: &str| conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
        let root = conn.setup().roots[screen_num].root;
        let names = Names {
            root,
            resources: x11rb::protocol::xproto::AtomEnum::RESOURCE_MANAGER.into(),
            selection: atom(&format!("_XSETTINGS_S{screen_num}"))?,
            settings: atom("_XSETTINGS_SETTINGS")?,
            manager: atom("MANAGER")?,
        };
        // Structure: MANAGER messages are sent to the root window with this mask.
        let mask = EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY;
        conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(mask)).ok()?;
        let watch = ScaleWatch { names, owner: AtomicU32::new(0) };
        let owner = conn.get_selection_owner(names.selection).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.owner);
        watch.follow(conn, owner);
        Some(watch)
    }

    /// Watches the settings of the XSETTINGS manager whose window is `owner`, and its going away.
    fn follow(&self, conn: &RustConnection, owner: u32) {
        self.owner.store(owner, Ordering::Relaxed);
        if owner != 0 {
            let mask = EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY;
            let _ = conn.change_window_attributes(owner, &ChangeWindowAttributesAux::new().event_mask(mask));
            let _ = conn.flush();
        }
    }

    /// Takes an event from the editor's connection. Returns true when the scale may have changed.
    pub(crate) fn note(&self, conn: &RustConnection, ev: &XEvent) -> bool {
        match classify(&self.names, self.owner.load(Ordering::Relaxed), ev) {
            Seen::Nothing => false,
            Seen::Changed => true,
            Seen::NewManager(owner) => {
                self.follow(conn, owner);
                true
            }
            Seen::ManagerGone => {
                self.owner.store(0, Ordering::Relaxed);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::{ClientMessageEvent, DestroyNotifyEvent, Property, PropertyNotifyEvent};

    const N: Names = Names { root: 1, resources: 23, selection: 300, settings: 301, manager: 302 };
    const OWNER: u32 = 77;

    fn property(window: u32, atom: u32) -> XEvent {
        XEvent::PropertyNotify(PropertyNotifyEvent { response_type: 28, sequence: 0, window, atom, time: 0, state: Property::NEW_VALUE })
    }

    fn message(window: u32, type_: u32, data: [u32; 5]) -> XEvent {
        XEvent::ClientMessage(ClientMessageEvent::new(32, window, type_, data))
    }

    #[test]
    fn xrdb_and_the_settings_daemon_tell_a_change() {
        assert_eq!(classify(&N, OWNER, &property(N.root, N.resources)), Seen::Changed, "xrdb -merge");
        assert_eq!(classify(&N, OWNER, &property(OWNER, N.settings)), Seen::Changed, "the XSETTINGS manager");
    }

    #[test]
    fn other_properties_and_windows_say_nothing() {
        // A window manager changes root properties all the time (_NET_ACTIVE_WINDOW).
        assert_eq!(classify(&N, OWNER, &property(N.root, 400)), Seen::Nothing);
        assert_eq!(classify(&N, OWNER, &property(OWNER, N.resources)), Seen::Nothing);
        assert_eq!(classify(&N, OWNER, &property(N.root, N.settings)), Seen::Nothing);
        assert_eq!(classify(&N, 0, &property(0, N.settings)), Seen::Nothing, "no manager");
        assert_eq!(classify(&N, OWNER, &property(55, N.settings)), Seen::Nothing);
    }

    #[test]
    fn a_new_manager_is_followed_and_one_going_away_is_a_change() {
        assert_eq!(classify(&N, 0, &message(N.root, N.manager, [0, N.selection, 88, 0, 0])), Seen::NewManager(88));
        // Another selection's manager (a clipboard manager, a system tray).
        assert_eq!(classify(&N, 0, &message(N.root, N.manager, [0, 999, 88, 0, 0])), Seen::Nothing);
        assert_eq!(classify(&N, 0, &message(42, 500, [N.selection, 0, 0, 0, 0])), Seen::Nothing, "WM_DELETE_WINDOW and the like");
        let gone = |window| XEvent::DestroyNotify(DestroyNotifyEvent { response_type: 17, sequence: 0, event: window, window });
        assert_eq!(classify(&N, OWNER, &gone(OWNER)), Seen::ManagerGone);
        assert_eq!(classify(&N, OWNER, &gone(12)), Seen::Nothing);
        assert_eq!(classify(&N, 0, &gone(0)), Seen::Nothing);
    }
}
