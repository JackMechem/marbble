//! The key commands' keys in the macOS menu bar. AppKit draws a key beside an item only when the
//! item is bound to it (its key equivalent), and a bound key is taken by the menu bar before the
//! page sees it: R or Space would stop reaching text fields and the page's own key handling (see
//! `mod.rs`). So the keys are bound only while the menu bar is open, when every key goes to the
//! menu anyway: on NSMenuDidBeginTrackingNotification each item gets its key command's key, and
//! on NSMenuDidEndTrackingNotification they're taken off again. A key pressed while a menu is
//! open runs the item once (the menu has it; the page doesn't get it), and a click runs it as
//! before. Items with an accelerator of their own (`spec`: ⌘S, ⌘Z…) are muda's and left alone.

// The mapping is tested everywhere; only macOS uses it.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::gtk_keys::{split_key, F_KEYS};

/// A key spec from src/keyboard/keymap.ts as an AppKit key equivalent and its modifiers.
#[derive(Debug, PartialEq)]
pub(super) struct MacKey {
    /// The key equivalent: a lower-case character, or one of AppKit's function-key characters.
    pub equivalent: String,
    /// ⌘ ("Mod" in a spec).
    pub command: bool,
    /// ⌃ ("Ctrl").
    pub control: bool,
    /// ⌥ ("Alt").
    pub option: bool,
    /// ⇧ ("Shift").
    pub shift: bool,
}

// AppKit's characters for keys that have none (NSEvent.h).
const NS_UP_ARROW: u32 = 0xF700;
const NS_DOWN_ARROW: u32 = 0xF701;
const NS_LEFT_ARROW: u32 = 0xF702;
const NS_RIGHT_ARROW: u32 = 0xF703;
const NS_F1: u32 = 0xF704;
const NS_DELETE: u32 = 0xF728;
const NS_HOME: u32 = 0xF729;
const NS_END: u32 = 0xF72B;
const NS_PAGE_UP: u32 = 0xF72C;
const NS_PAGE_DOWN: u32 = 0xF72D;

/// The AppKit key equivalent for a key spec; None for no key or one AppKit can't bind.
pub(super) fn mac_key(spec: &str) -> Option<MacKey> {
    let (mods, key) = split_key(spec)?;
    let has = |m: &str| mods.split('+').any(|x| x == m);
    let function = |c: u32| char::from_u32(c).map(String::from);
    let equivalent = match key {
        "Space" => Some(" ".to_string()),
        "Enter" => Some("\r".to_string()),
        "Tab" => Some("\t".to_string()),
        "Escape" => Some("\u{1b}".to_string()),
        // ⌫: NSBackspaceCharacter. ⌦ (the forward Delete key) is a function key.
        "Backspace" => Some("\u{8}".to_string()),
        "Delete" => function(NS_DELETE),
        "ArrowUp" => function(NS_UP_ARROW),
        "ArrowDown" => function(NS_DOWN_ARROW),
        "ArrowLeft" => function(NS_LEFT_ARROW),
        "ArrowRight" => function(NS_RIGHT_ARROW),
        "Home" => function(NS_HOME),
        "End" => function(NS_END),
        "PageUp" => function(NS_PAGE_UP),
        "PageDown" => function(NS_PAGE_DOWN),
        _ => match F_KEYS.iter().position(|f| *f == key) {
            Some(n) => function(NS_F1 + n as u32),
            None => {
                let mut chars = key.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                // Lower case: an upper-case equivalent would mean Shift too. Shift is its own flag.
                Some(c.to_lowercase().collect())
            }
        },
    }?;
    Some(MacKey { equivalent, command: has("Mod"), control: has("Ctrl"), option: has("Alt"), shift: has("Shift") })
}

#[cfg(target_os = "macos")]
pub(super) use appkit::observe_menu_tracking;

#[cfg(target_os = "macos")]
mod appkit {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuDidBeginTrackingNotification, NSMenuDidEndTrackingNotification, NSMenuItem};
    use objc2_foundation::{MainThreadMarker, NSNotification, NSNotificationCenter, NSString};
    use tauri::{AppHandle, Manager, Runtime};

    use super::{mac_key, MacKey};
    use crate::menu::MenuBar;

    impl MacKey {
        pub(in crate::menu) fn flags(&self) -> NSEventModifierFlags {
            let mut f = NSEventModifierFlags::empty();
            f.set(NSEventModifierFlags::Command, self.command);
            f.set(NSEventModifierFlags::Control, self.control);
            f.set(NSEventModifierFlags::Option, self.option);
            f.set(NSEventModifierFlags::Shift, self.shift);
            f
        }
    }

    /// Binds the key commands' keys to their items while the menu bar is open (see the module's
    /// notes). Call on the main thread.
    pub fn observe_menu_tracking<R: Runtime>(app: &AppHandle<R>) {
        let Some(mtm) = MainThreadMarker::new() else {
            log::warn!("menu bar keys: not on the main thread");
            return;
        };
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: AppKit's notification names, constant for the process.
        let names = unsafe { [(NSMenuDidBeginTrackingNotification, true), (NSMenuDidEndTrackingNotification, false)] };
        for (name, open) in names {
            let app = app.clone();
            let block = RcBlock::new(move |note: NonNull<NSNotification>| {
                // SAFETY: AppKit passes a valid notification for the block's duration.
                let note = unsafe { note.as_ref() };
                let Some(main) = NSApplication::sharedApplication(mtm).mainMenu() else { return };
                // The menu bar's notifications carry the main menu; a pop-up's carry its own.
                let ours = note.object().is_some_and(|o| std::ptr::eq(Retained::as_ptr(&o).cast::<u8>(), Retained::as_ptr(&main).cast::<u8>()));
                if let (true, Some(bar)) = (ours, app.try_state::<MenuBar<R>>()) {
                    bar.bind_keys(&main, open);
                }
            });
            // SAFETY: no object filter; queue None runs the block on the posting thread, the
            // main thread for menu tracking. The center keeps the observer for the app's life.
            let observer = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) };
            std::mem::forget(observer);
        }
    }

    /// The item at `path` in the menu bar (see `Entry::path`).
    fn ns_item(main: &NSMenu, path: &[usize]) -> Option<Retained<NSMenuItem>> {
        let (first, rest) = path.split_first()?;
        let mut item = main.itemAtIndex(*first as isize)?;
        for i in rest {
            item = item.submenu()?.itemAtIndex(*i as isize)?;
        }
        Some(item)
    }

    impl<R: Runtime> MenuBar<R> {
        /// Binds each item to its key command's key (`open`), or takes the keys off again.
        fn bind_keys(&self, main: &NSMenu, open: bool) {
            let items: Vec<_> = {
                let shown = self.shown.lock();
                self.items
                    .iter()
                    .filter(|(_, e)| !e.accel)
                    .map(|(id, e)| (e.path.clone(), self.label(&shown, id, e), if open { shown.keys.get(id).and_then(|k| mac_key(k)) } else { None }))
                    .collect()
            };
            let none = NSString::new();
            for (path, label, key) in items {
                // An item found where it's expected, under its label: AppKit adds nothing to these menus.
                let Some(item) = ns_item(main, &path).filter(|i| i.submenu().is_none() && i.title().to_string() == label) else { continue };
                match key {
                    Some(k) => {
                        item.setKeyEquivalent(&NSString::from_str(&k.equivalent));
                        item.setKeyEquivalentModifierMask(k.flags());
                    }
                    None if item.keyEquivalent().length() > 0 => {
                        item.setKeyEquivalent(&none);
                        item.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());
                    }
                    None => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(equivalent: &str, command: bool, control: bool, option: bool, shift: bool) -> Option<MacKey> {
        Some(MacKey { equivalent: equivalent.into(), command, control, option, shift })
    }

    #[test]
    fn key_specs_read_as_appkit_key_equivalents() {
        assert_eq!(mac_key("R"), k("r", false, false, false, false));
        assert_eq!(mac_key("Shift+K"), k("k", false, false, false, true));
        assert_eq!(mac_key("Mod+Shift+V"), k("v", true, false, false, true));
        assert_eq!(mac_key("Mod+Ctrl+P"), k("p", true, true, false, false));
        assert_eq!(mac_key("Mod+Alt+Shift+K"), k("k", true, false, true, true));
        assert_eq!(mac_key("Ctrl+B"), k("b", false, true, false, false));
        assert_eq!(mac_key("Alt+,"), k(",", false, false, true, false));
        assert_eq!(mac_key("Mod++"), k("+", true, false, false, false));
        assert_eq!(mac_key("+"), k("+", false, false, false, false));
        assert_eq!(mac_key("1"), k("1", false, false, false, false));
        assert_eq!(mac_key("Space"), k(" ", false, false, false, false));
        assert_eq!(mac_key("Enter"), k("\r", false, false, false, false));
        assert_eq!(mac_key("Escape"), k("\u{1b}", false, false, false, false));
        assert_eq!(mac_key("Tab"), k("\t", false, false, false, false));
        assert_eq!(mac_key("Backspace"), k("\u{8}", false, false, false, false));
        assert_eq!(mac_key("Delete"), k("\u{F728}", false, false, false, false));
        assert_eq!(mac_key("ArrowUp"), k("\u{F700}", false, false, false, false));
        assert_eq!(mac_key("ArrowDown"), k("\u{F701}", false, false, false, false));
        assert_eq!(mac_key("Mod+ArrowLeft"), k("\u{F702}", true, false, false, false));
        assert_eq!(mac_key("Mod+ArrowRight"), k("\u{F703}", true, false, false, false));
        assert_eq!(mac_key("Home"), k("\u{F729}", false, false, false, false));
        assert_eq!(mac_key("End"), k("\u{F72B}", false, false, false, false));
        assert_eq!(mac_key("PageUp"), k("\u{F72C}", false, false, false, false));
        assert_eq!(mac_key("PageDown"), k("\u{F72D}", false, false, false, false));
        assert_eq!(mac_key("F1"), k("\u{F704}", false, false, false, false));
        assert_eq!(mac_key("Shift+F12"), k("\u{F70F}", false, false, false, true));
        assert_eq!(mac_key("F13"), None);
        assert_eq!(mac_key(""), None);
        assert_eq!(mac_key("Mod+Banana"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn modifiers_map_to_appkit_flags() {
        use objc2_app_kit::NSEventModifierFlags as F;
        assert_eq!(mac_key("R").unwrap().flags(), F::empty());
        assert_eq!(mac_key("Mod+Ctrl+Alt+Shift+K").unwrap().flags(), F::Command | F::Control | F::Option | F::Shift);
        assert_eq!(mac_key("Ctrl+B").unwrap().flags(), F::Control);
    }
}
