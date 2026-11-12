//! The native menu bar. Items emit a `menu` event carrying their id to the frontend (to the
//! focused detached panel window, if any; see windows.rs). The bar is described once, as data
//! (`spec`), and the frontend keeps it in step with the app through `set_menu_state`
//! (src/app/menuBar/menuBar.ts): ticks, greyed-out items, the Undo/Redo names, and the keys of the key
//! commands beside their items.
//!
//! Two kinds of key. The accelerators in `spec` (Ctrl+S, Ctrl+Z…) belong to the menu: GTK,
//! AppKit or Win32 runs the item when the key is pressed, before the page sees it. Every other
//! key is a key command (src/keyboard/keymap.ts) that the window handles itself and the user can
//! reassign; the menu bar only shows those keys and doesn't register them (on macOS only while
//! a menu is open, when the page gets no keys anyway: `mac_keys.rs`). A registered key
//! would be taken before the page sees it: R or X would stop reaching text fields and Musical
//! Typing, and the page's own handling (key repeat, the editors' keys, sheets that block
//! keys) would be skipped. So nothing can run twice either: a key either belongs to the menu
//! or to the page.
//!
//! A few items never reach the page: Quit, Window's Minimize and list of open windows, and
//! Help's web pages (`window_command`, `HELP_PAGES`). Help › Check for Updates… does: the
//! frontend asks GitHub (src/app/updates/updates.ts).
//!
//! Here: building the bar, handling its items, and `set_menu_state`. The bar as data is in
//! `spec.rs`, the Window menu in `window_menu.rs`, File's project lists in `project_menus.rs`, and
//! what GTK needs drawn by hand (keys beside items, radio items, key names) in `gtk_keys.rs`.
//! AppKit draws a key only beside an item bound to it, so on macOS the key commands' keys are
//! bound only while the menu bar is open (`mac_keys.rs`).

pub mod gtk_keys;
mod mac_keys;
pub mod project_menus;
mod spec;
pub mod window_menu;

#[cfg(target_os = "linux")]
use gtk_keys::show_keys;
use project_menus::fill_project_menus;
use spec::*;
use window_menu::{refresh_windows, window_command};

use std::collections::HashMap;

use parking_lot::Mutex;
use serde::Deserialize;
use tauri::menu::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{App, AppHandle, Manager, Runtime};

enum Handle<R: Runtime> {
    Item(MenuItem<R>),
    Check(CheckMenuItem<R>),
}

/// A built item: its handle, its label in `spec`, and where it sits (its index in the menu bar,
/// then in each submenu down to it, separators counted), which finds its GTK widget or NSMenuItem,
/// and whether it has an accelerator of its own (`spec`).
struct Entry<R: Runtime> {
    handle: Handle<R>,
    label: &'static str,
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    path: Vec<usize>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    accel: bool,
}

/// A submenu of `spec` (Repeat, Trim, Track Alternatives…), greyed out while none of its items
/// applies, as the in-app menus' submenus are.
struct SubEntry<R: Runtime> {
    handle: Submenu<R>,
    /// The ids of the items in it, its own submenus' included.
    ids: Vec<&'static str>,
}

/// What the frontend last sent, by item id.
#[derive(Default)]
struct Shown {
    /// The key command's key shown beside an item, as src/keyboard/keymap.ts writes it ("Mod+Shift+V").
    keys: HashMap<String, String>,
    /// The same, as the platform writes it ("Ctrl+Shift+V"); Windows shows it in the label.
    key_texts: HashMap<String, String>,
    /// Labels other than the one in `spec` ("Undo Move Region").
    texts: HashMap<String, String>,
    checked: HashMap<String, bool>,
    /// Items greyed out (false) or enabled again; an item not here is enabled.
    enabled: HashMap<String, bool>,
    /// The `MenuBar::subs` greyed out, by index.
    subs_off: Vec<usize>,
    /// The windows Window lists, as (label, title).
    windows: Vec<(String, String)>,
    /// Bumped when `keys` changes; `applied` is what each window's GTK menu bar shows.
    generation: u64,
    applied: HashMap<String, u64>,
}

/// The built menu bar, kept as managed state.
pub struct MenuBar<R: Runtime> {
    alternatives: Submenu<R>,
    revert: Submenu<R>,
    /// The Window menu, and how many items it has before its list of windows.
    window: (Submenu<R>, usize),
    items: HashMap<String, Entry<R>>,
    subs: Vec<SubEntry<R>>,
    /// Where the radio items are, and File › Project Alternatives (see `Entry::path`).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    radios: Vec<Vec<usize>>,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    alternatives_path: Vec<usize>,
    shown: Mutex<Shown>,
}

struct Builder<'a, R: Runtime> {
    app: &'a App<R>,
    items: HashMap<String, Entry<R>>,
    subs: Vec<SubEntry<R>>,
    radios: Vec<Vec<usize>>,
    alternatives: Option<(Submenu<R>, Vec<usize>)>,
    revert: Option<Submenu<R>>,
    window: Option<(Submenu<R>, usize)>,
}

/// The ids of the items in `specs`, submenus included.
fn ids_in(specs: &[Spec]) -> Vec<&'static str> {
    specs
        .iter()
        .flat_map(|s| match s {
            Item(id, ..) | Check(id, ..) | Radio(id, _) => vec![*id],
            Sub(_, children) => ids_in(children),
            _ => vec![],
        })
        .collect()
}

/// Whether any of `ids` is enabled.
fn any_enabled(ids: &[&str], enabled: &HashMap<String, bool>) -> bool {
    ids.iter().any(|id| enabled.get(*id).copied().unwrap_or(true))
}

impl<R: Runtime> Builder<'_, R> {
    fn fill(&mut self, parent: &Submenu<R>, specs: &[Spec], path: &mut Vec<usize>) -> tauri::Result<()> {
        for (i, s) in specs.iter().enumerate() {
            path.push(i);
            match s {
                Item(id, label, accel) => {
                    let m = MenuItem::with_id(self.app, *id, *label, true, *accel)?;
                    parent.append(&m)?;
                    self.items.insert(id.to_string(), Entry { handle: Handle::Item(m), label, path: path.clone(), accel: accel.is_some() });
                }
                Check(id, label, accel) => {
                    let m = CheckMenuItem::with_id(self.app, *id, *label, true, false, *accel)?;
                    parent.append(&m)?;
                    self.items.insert(id.to_string(), Entry { handle: Handle::Check(m), label, path: path.clone(), accel: accel.is_some() });
                }
                Radio(id, label) => {
                    let m = CheckMenuItem::with_id(self.app, *id, *label, true, false, None::<&str>)?;
                    parent.append(&m)?;
                    self.items.insert(id.to_string(), Entry { handle: Handle::Check(m), label, path: path.clone(), accel: false });
                    self.radios.push(path.clone());
                }
                Sep => parent.append(&PredefinedMenuItem::separator(self.app)?)?,
                Sub(label, children) => {
                    let sub = Submenu::new(self.app, *label, true)?;
                    self.fill(&sub, children, path)?;
                    parent.append(&sub)?;
                    self.subs.push(SubEntry { handle: sub, ids: ids_in(children) });
                }
                Spec::Revert => {
                    let sub = Submenu::new(self.app, "Revert to", true)?;
                    parent.append(&sub)?;
                    self.revert = Some(sub);
                }
                Spec::Alternatives => {
                    let sub = Submenu::new(self.app, "Project Alternatives", true)?;
                    parent.append(&sub)?;
                    self.alternatives = Some((sub, path.clone()));
                }
                Spec::Windows => self.window = Some((parent.clone(), i)),
                Spec::About => {
                    let about = AboutMetadata {
                        name: Some("Marbble".into()),
                        version: Some(self.app.package_info().version.to_string()),
                        comments: Some("A digital audio workstation with VST3 hosting.".into()),
                        license: Some("MIT".into()),
                        website: Some("https://marbble.dev/".into()),
                        // GTK's About dialog looks the logo up in the icon theme otherwise,
                        // which a build that isn't installed doesn't have.
                        icon: Some(tauri::include_image!("icons/128x128.png")),
                        ..Default::default()
                    };
                    parent.append(&PredefinedMenuItem::about(self.app, Some("About Marbble"), Some(about))?)?;
                }
            }
            path.pop();
        }
        Ok(())
    }
}

pub fn install<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let mut b = Builder { app, items: HashMap::new(), subs: Vec::new(), radios: Vec::new(), alternatives: None, revert: None, window: None };
    let menu = Menu::new(app)?;
    for (i, top) in spec().iter().enumerate() {
        let Sub(label, children) = top else { unreachable!("the menu bar holds menus") };
        let sub = Submenu::new(app, *label, true)?;
        b.fill(&sub, children, &mut vec![i])?;
        menu.append(&sub)?;
    }
    let (Some((alternatives, alternatives_path)), Some(revert), Some(window)) = (b.alternatives, b.revert, b.window) else {
        unreachable!("File has both lists, Window its list")
    };
    fill_project_menus(app.handle(), &alternatives, &revert, &[], "", &[])?;
    app.set_menu(menu)?;
    app.manage(MenuBar {
        alternatives,
        revert,
        window,
        items: b.items,
        subs: b.subs,
        radios: b.radios,
        alternatives_path,
        shown: Mutex::new(Shown::default()),
    });
    refresh_windows(app.handle(), None);
    #[cfg(target_os = "macos")]
    mac_keys::observe_menu_tracking(app.handle());
    app.on_menu_event(|app, ev| {
        let id = ev.id().0.clone();
        crate::diagnostics::activity::note("menu", id.clone());
        if id == "quit" {
            return quit(app);
        }
        // A check item ticks or unticks itself when chosen. Put the tick back: the frontend
        // sends the new state once the command has run, so a command that changes nothing
        // (or fails) leaves the tick right.
        if let Some(bar) = app.try_state::<MenuBar<R>>() {
            bar.restore_check(&id);
        }
        if window_command(app, &id) {
            return;
        }
        if let Some((_, url)) = HELP_PAGES.iter().find(|(h, _)| *h == id) {
            return open_url(url);
        }
        // Help › Key Commands is Marbble › Key Commands.
        let id = if id == "help-keys" { "settings-keys".to_string() } else { id };
        crate::windows::route_menu(app, id)
    });
    Ok(())
}

/// Opens a web page in the default browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "linux")]
    let r = gio::AppInfo::launch_default_for_uri(url, None::<&gio::AppLaunchContext>).map_err(|e| e.to_string());
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string());
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string());
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let r: Result<(), String> = Err("no browser".into());
    if let Err(e) = r {
        log::warn!("could not open {url}: {e}");
    }
}

/// Marbble › Quit Marbble: closes the main window as its close button does, so the page can
/// ask about unsaved changes (src/app/App.tsx `onCloseRequested`); the panel windows go with it.
fn quit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.close();
    }
}

impl<R: Runtime> MenuBar<R> {
    fn restore_check(&self, id: &str) {
        let Some(Entry { handle: Handle::Check(m), .. }) = self.items.get(id) else { return };
        // Built unticked; the frontend sends a tick only when it differs.
        let was = self.shown.lock().checked.get(id).copied().unwrap_or(false);
        let _ = m.set_checked(was);
    }

    /// The label to show: the frontend's (Undo Move Region) or `spec`'s, and on Windows the key
    /// after a tab, which Windows right-aligns as it does an accelerator.
    fn label(&self, shown: &Shown, id: &str, e: &Entry<R>) -> String {
        let text = shown.texts.get(id).map_or(e.label, String::as_str);
        match shown.key_texts.get(id) {
            Some(k) if cfg!(windows) && !k.is_empty() => format!("{text}\t{k}"),
            _ => text.to_string(),
        }
    }
}

/// An item's state from the frontend (src/app/menuBar/menuBar.ts). Each field is sent only when it changed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemState {
    id: String,
    enabled: Option<bool>,
    checked: Option<bool>,
    /// The label, when it isn't `spec`'s (or back to it).
    text: Option<String>,
    /// The key command's key, as src/keyboard/keymap.ts writes it; "" for none.
    key: Option<String>,
    /// The same key as the platform writes it, for the label on Windows.
    key_text: Option<String>,
}

/// Brings the menu bar's items in step with the app: enabled, ticked, their label and the key
/// shown beside them. Ids the bar doesn't have are ignored.
#[tauri::command]
pub fn set_menu_state<R: Runtime>(app: AppHandle<R>, items: Vec<ItemState>) -> Result<(), String> {
    let Some(bar) = app.try_state::<MenuBar<R>>() else { return Ok(()) };
    let mut keys_changed = false;
    // The lock isn't held while the menu changes: those calls run on the main thread, where
    // `restore_check` takes it too.
    let mut changes = Vec::new();
    let mut sub_changes = Vec::new();
    {
        let mut shown = bar.shown.lock();
        for it in items {
            let Some(e) = bar.items.get(&it.id) else { continue };
            if let Some(on) = it.enabled {
                shown.enabled.insert(it.id.clone(), on);
            }
            if let Some(c) = it.checked {
                shown.checked.insert(it.id.clone(), c);
            }
            let relabel = it.text.is_some() || (cfg!(windows) && it.key_text.is_some());
            if let Some(t) = it.text {
                if t == e.label {
                    shown.texts.remove(&it.id);
                } else {
                    shown.texts.insert(it.id.clone(), t);
                }
            }
            if let Some(k) = it.key_text {
                shown.key_texts.insert(it.id.clone(), k);
            }
            if let Some(k) = it.key {
                keys_changed |= shown.keys.get(&it.id) != Some(&k);
                shown.keys.insert(it.id.clone(), k);
            }
            let label = relabel.then(|| bar.label(&shown, &it.id, e));
            changes.push((e, it.enabled, it.checked, label));
        }
        if keys_changed {
            shown.generation += 1;
        }
        for (i, sub) in bar.subs.iter().enumerate() {
            let on = any_enabled(&sub.ids, &shown.enabled);
            if on == !shown.subs_off.contains(&i) {
                continue;
            }
            if on {
                shown.subs_off.retain(|x| *x != i);
            } else {
                shown.subs_off.push(i);
            }
            sub_changes.push((&sub.handle, on));
        }
    }
    for (sub, on) in sub_changes {
        sub.set_enabled(on).map_err(|e| e.to_string())?;
    }
    for (e, enabled, checked, label) in changes {
        let r = match &e.handle {
            Handle::Item(m) => enabled.map_or(Ok(()), |on| m.set_enabled(on)).and_then(|_| label.map_or(Ok(()), |t| m.set_text(t))),
            Handle::Check(m) => enabled
                .map_or(Ok(()), |on| m.set_enabled(on))
                .and_then(|_| checked.map_or(Ok(()), |c| m.set_checked(c)))
                .and_then(|_| label.map_or(Ok(()), |t| m.set_text(t))),
        };
        r.map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    if keys_changed {
        show_keys(&app);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
