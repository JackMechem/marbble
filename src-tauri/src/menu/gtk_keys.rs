//! Keys in the menu bar on GTK, which muda can't draw itself: the key commands' keys shown beside
//! their items (without binding them), the radio items drawn as radio buttons, reading a key spec
//! as src/keyboard/keymap.ts writes it, and how GTK names keys in the app's language.

use std::collections::HashMap;

use tauri::{AppHandle, Manager, Runtime};

use super::MenuBar;

impl<R: Runtime> MenuBar<R> {
    /// Shows the key commands' keys in a window's GTK menu bar, unless it already shows them.
    /// GTK draws a key set on the item's label without binding it (`gtk_accel_label_set_accel`).
    #[cfg(target_os = "linux")]
    pub(super) fn show_keys_in(&self, window: &tauri::WebviewWindow<R>) {
        use gtk::prelude::*;
        let (generation, keys) = {
            let shown = self.shown.lock();
            if shown.applied.get(window.label()) == Some(&shown.generation) {
                return;
            }
            let keys: Vec<_> = shown.keys.iter().filter_map(|(id, k)| Some((self.items.get(id)?.path.clone(), gtk_key(k)))).collect();
            (shown.generation, keys)
        };
        let Some(bar) = window.default_vbox().ok().and_then(|b| b.children().into_iter().find_map(|w| w.downcast::<gtk::MenuBar>().ok())) else { return };
        for (path, key) in keys {
            let label = gtk_item(&bar, &path).and_then(|i| i.child()).and_then(|c| c.downcast::<gtk::AccelLabel>().ok());
            if let Some(label) = label {
                let (key, mods) = key.unwrap_or((0, gdk::ModifierType::empty()));
                label.set_accel(key, mods);
            }
        }
        self.shown.lock().applied.insert(window.label().to_string(), generation);
    }

    /// Draws the one-of-several items (View › Theme, File › Project Alternatives) as radio
    /// buttons in a window's GTK menu bar. muda has only check items; GTK draws those as boxes.
    #[cfg(target_os = "linux")]
    pub(super) fn draw_radios_in(&self, window: &tauri::WebviewWindow<R>) {
        use gtk::prelude::*;
        let Some(bar) = window.default_vbox().ok().and_then(|b| b.children().into_iter().find_map(|w| w.downcast::<gtk::MenuBar>().ok())) else { return };
        let alternatives = gtk_item(&bar, &self.alternatives_path)
            .and_then(|i| i.submenu())
            .and_then(|m| m.downcast::<gtk::Menu>().ok())
            .map(|m| m.children())
            .unwrap_or_default();
        let radios = self.radios.iter().filter_map(|p| gtk_item(&bar, p)).map(|i| i.upcast::<gtk::Widget>());
        for w in radios.chain(alternatives) {
            if let Ok(c) = w.downcast::<gtk::CheckMenuItem>() {
                if !c.draws_as_radio() {
                    c.set_draw_as_radio(true);
                }
            }
        }
    }
}

/// A GDK keyval and its modifiers.
#[cfg(target_os = "linux")]
pub(super) type GtkKey = (u32, gdk::ModifierType);

/// The item at `path` in a GTK menu bar (see `Entry::path`).
#[cfg(target_os = "linux")]
pub(super) fn gtk_item(bar: &gtk::MenuBar, path: &[usize]) -> Option<gtk::MenuItem> {
    use gtk::prelude::*;
    let (first, rest) = path.split_first()?;
    let mut item = bar.children().get(*first)?.clone().downcast::<gtk::MenuItem>().ok()?;
    for i in rest {
        let menu = item.submenu()?.downcast::<gtk::Menu>().ok()?;
        item = menu.children().get(*i)?.clone().downcast::<gtk::MenuItem>().ok()?;
    }
    Some(item)
}

/// A key spec from src/keyboard/keymap.ts as GDK's keyval and modifiers; None for no key or one GDK can't name.
#[cfg(target_os = "linux")]
pub(super) fn gtk_key(spec: &str) -> Option<GtkKey> {
    let k = parse_key(spec)?;
    let keyval = match k.key {
        Key::Named(name) => *gdk::keys::Key::from_name(name),
        Key::Char(c) => *gdk::keys::Key::from_unicode(c),
    };
    if keyval == 0 || keyval == *gdk::keys::constants::VoidSymbol {
        return None;
    }
    let mut mods = gdk::ModifierType::empty();
    mods.set(gdk::ModifierType::CONTROL_MASK, k.ctrl);
    mods.set(gdk::ModifierType::MOD1_MASK, k.alt);
    mods.set(gdk::ModifierType::SHIFT_MASK, k.shift);
    Some((keyval, mods))
}

/// A key as GDK names it.
#[derive(Debug, PartialEq)]
pub(super) enum Key {
    /// A keyval name ("space", "BackSpace", "comma").
    Named(&'static str),
    /// A character with no name of its own here.
    Char(char),
}

#[derive(Debug, PartialEq)]
pub(super) struct ParsedKey {
    ctrl: bool,
    alt: bool,
    shift: bool,
    key: Key,
}

pub(super) const F_KEYS: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];

/// Reads a key spec as src/keyboard/keymap.ts writes it: modifiers then the key, joined with "+" ("Mod" is
/// Ctrl outside macOS, where this is used; "Mod++" is the + key). None for an empty spec.
pub(super) fn parse_key(spec: &str) -> Option<ParsedKey> {
    let (mods, key) = match spec.strip_suffix("++") {
        Some(m) => (m, "+"),
        None if spec == "+" => ("", "+"),
        None => spec.rsplit_once('+').unwrap_or(("", spec)),
    };
    if key.is_empty() {
        return None;
    }
    let has = |m: &str| mods.split('+').any(|x| x == m);
    let named = match key {
        "Space" => "space",
        "Enter" => "Return",
        "Backspace" => "BackSpace",
        "Delete" => "Delete",
        "Escape" => "Escape",
        "Tab" => "Tab",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "Page_Up",
        "PageDown" => "Page_Down",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "," => "comma",
        "." => "period",
        "'" => "apostrophe",
        ";" => "semicolon",
        "`" => "grave",
        "[" => "bracketleft",
        "]" => "bracketright",
        "=" => "equal",
        "-" => "minus",
        "/" => "slash",
        "\\" => "backslash",
        "+" => "plus",
        _ => "",
    };
    let key = if !named.is_empty() {
        Key::Named(named)
    } else if let Some(f) = F_KEYS.iter().find(|f| **f == key) {
        Key::Named(f)
    } else {
        let mut chars = key.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        // GDK binds letters by their lower-case keyval and shows them upper case.
        Key::Char(c.to_ascii_lowercase())
    };
    Some(ParsedKey { ctrl: has("Mod") || has("Ctrl"), alt: has("Alt"), shift: has("Shift"), key })
}

/// Shows the current keys in every window's menu bar (GTK, on the main thread).
#[cfg(target_os = "linux")]
pub(super) fn show_keys<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(bar) = handle.try_state::<MenuBar<R>>() else { return };
        for w in handle.webview_windows().values() {
            bar.show_keys_in(w);
        }
    });
}

/// How GTK writes keys in the app's language: each named key on its own, and each modifier as
/// the text put before a key ("Shift+"). GTK draws the menu bar's keys itself, translated
/// ("BackSpace" in the C locale, "Backspace" in English), so src/keyboard/keyFormat.ts writes keys with
/// these names on Linux and the in-app menus and tooltips read like the menu bar. None
/// elsewhere.
#[derive(serde::Serialize, Debug)]
pub struct GtkKeyNames {
    keys: HashMap<&'static str, String>,
    mods: HashMap<&'static str, String>,
}

/// The keys `GtkKeyNames` names: those GTK spells its own way.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) const NAMED_KEYS: [&str; 14] =
    ["Backspace", "Enter", "Space", "Escape", "Delete", "Tab", "Home", "End", "PageUp", "PageDown", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"];

#[tauri::command]
pub fn gtk_key_names() -> Option<GtkKeyNames> {
    #[cfg(target_os = "linux")]
    {
        let keys = NAMED_KEYS.iter().filter_map(|k| Some((*k, gtk_label(gtk_key(k)?)))).collect();
        let a = gtk_key("A")?;
        let plain = gtk_label(a);
        let mods = [("Shift", gdk::ModifierType::SHIFT_MASK), ("Ctrl", gdk::ModifierType::CONTROL_MASK), ("Alt", gdk::ModifierType::MOD1_MASK)]
            .into_iter()
            .filter_map(|(name, m)| Some((name, gtk_label((a.0, m)).strip_suffix(&plain)?.to_string())))
            .collect();
        Some(GtkKeyNames { keys, mods })
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// The label GTK draws for a key beside a menu item (`gtk_accelerator_get_label`). The C
/// function, which only formats: the Rust one wants GTK initialised on this thread.
#[cfg(target_os = "linux")]
pub(super) fn gtk_label((key, mods): GtkKey) -> String {
    unsafe {
        let p = gtk::ffi::gtk_accelerator_get_label(key, glib::translate::IntoGlib::into_glib(mods));
        let s = std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned();
        glib::ffi::g_free(p.cast());
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_specs_read_as_gdk_names() {
        let k = |ctrl, alt, shift, key| Some(ParsedKey { ctrl, alt, shift, key });
        assert_eq!(parse_key("R"), k(false, false, false, Key::Char('r')));
        assert_eq!(parse_key("Shift+K"), k(false, false, true, Key::Char('k')));
        assert_eq!(parse_key("Mod+Shift+V"), k(true, false, true, Key::Char('v')));
        assert_eq!(parse_key("Mod+Alt+Shift+K"), k(true, true, true, Key::Char('k')));
        assert_eq!(parse_key("Ctrl+H"), k(true, false, false, Key::Char('h')));
        assert_eq!(parse_key("Space"), k(false, false, false, Key::Named("space")));
        assert_eq!(parse_key("Alt+,"), k(false, true, false, Key::Named("comma")));
        assert_eq!(parse_key("Alt+'"), k(false, true, false, Key::Named("apostrophe")));
        assert_eq!(parse_key("Mod+ArrowRight"), k(true, false, false, Key::Named("Right")));
        assert_eq!(parse_key("Backspace"), k(false, false, false, Key::Named("BackSpace")));
        assert_eq!(parse_key("Mod++"), k(true, false, false, Key::Named("plus")));
        assert_eq!(parse_key("F5"), k(false, false, false, Key::Named("F5")));
        assert_eq!(parse_key("F13"), None);
        assert_eq!(parse_key("1"), k(false, false, false, Key::Char('1')));
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("Mod+Banana"), None);
    }

    /// GTK draws the shown keys itself, in its own order and names; src/keyboard/keyFormat.ts writes
    /// keys the same way on Linux. Both check the pairs in tests/keyboard/gtkKeyLabels.json.
    #[cfg(target_os = "linux")]
    #[test]
    fn gtk_draws_keys_as_the_app_writes_them() {
        let pairs: Vec<(String, String)> = serde_json::from_str(include_str!("../../../tests/keyboard/gtkKeyLabels.json")).unwrap();
        assert!(pairs.len() > 20);
        for (spec, label) in pairs {
            let key = gtk_key(&spec).unwrap_or_else(|| panic!("{spec} has no GDK key"));
            assert_eq!(gtk_label(key), label, "{spec}");
        }
    }

    /// Untranslated (tests run in the C locale), GTK's names are those tests/keyboard/gtkKeyLabels.json
    /// pins, which src/keyboard/keyFormat.ts uses until the app sends the translated ones.
    #[cfg(target_os = "linux")]
    #[test]
    fn gtk_key_names_are_the_ones_gtk_draws() {
        let n = gtk_key_names().unwrap();
        assert_eq!(n.keys.len(), NAMED_KEYS.len());
        assert_eq!(n.keys["Backspace"], "BackSpace");
        assert_eq!(n.keys["Enter"], "Return");
        assert_eq!(n.keys["PageUp"], "Page Up");
        assert_eq!(n.keys["ArrowLeft"], "Left");
        assert_eq!(n.mods["Shift"], "Shift+");
        assert_eq!(n.mods["Ctrl"], "Ctrl+");
        assert_eq!(n.mods["Alt"], "Alt+");
    }
}
