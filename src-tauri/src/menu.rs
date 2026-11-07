//! The native menu bar. Items emit a `menu` event carrying their id to the frontend (to the
//! focused detached panel window, if any; see windows.rs). The bar is described once, as data
//! (`spec`), and the frontend keeps it in step with the app through `set_menu_state`
//! (src/menuBar.ts): ticks, greyed-out items, the Undo/Redo names, and the keys of the key
//! commands beside their items.
//!
//! Two kinds of key. The accelerators in `spec` (Ctrl+S, Ctrl+Z…) belong to the menu: GTK,
//! AppKit or Win32 runs the item when the key is pressed, before the page sees it. Every other
//! key is a key command (src/keymap.ts) that the window handles itself and the user can
//! reassign; the menu bar only shows those keys and never registers them. A registered key
//! would be taken before the page sees it: R or X would stop reaching text fields and Musical
//! Typing, and the page's own handling (key repeat, the editors' keys, sheets that block
//! keys) would be skipped. So nothing can run twice either: a key either belongs to the menu
//! or to the page.
//!
//! A few items never reach the page: Quit, Window's Minimize and list of open windows, and
//! Help's web pages (`window_command`, `HELP_PAGES`). Help › Check for Updates… does: the
//! frontend asks GitHub (src/updates.ts).

use std::collections::HashMap;

use parking_lot::Mutex;
use serde::Deserialize;
use tauri::menu::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{App, AppHandle, Manager, Runtime};

/// Logic's ⌃B. Elsewhere Ctrl+B is already Bounce Project (⌘B), so Bounce in Place adds Alt.
const BOUNCE_IN_PLACE_KEY: &str = if cfg!(target_os = "macos") { "Ctrl+B" } else { "Ctrl+Alt+B" };

/// One entry of the menu bar.
enum Spec {
    /// id, label, accelerator.
    Item(&'static str, &'static str, Option<&'static str>),
    /// A ticked item: the frontend sends its state.
    Check(&'static str, &'static str, Option<&'static str>),
    /// A ticked item that is one of several choices (View › Theme): GTK draws it as a radio
    /// button rather than a box. AppKit and Win32 draw a tick either way.
    Radio(&'static str, &'static str),
    Sep,
    Sub(&'static str, Vec<Spec>),
    /// File › Revert to and File › Project Alternatives, filled in by `set_project_menus`.
    Revert,
    Alternatives,
    About,
    /// Window's list of the open windows, at the end of the menu (`set_window_list`).
    Windows,
}

use Spec::{Check, Item, Radio, Sep, Sub};

const fn item(id: &'static str, label: &'static str) -> Spec {
    Item(id, label, None)
}

const fn keyed(id: &'static str, label: &'static str, accel: &'static str) -> Spec {
    Item(id, label, Some(accel))
}

const fn check(id: &'static str, label: &'static str) -> Spec {
    Check(id, label, None)
}

const fn radio(id: &'static str, label: &'static str) -> Spec {
    Radio(id, label)
}

/// The website's pages that Help opens, by item id.
const HELP_PAGES: [(&str, &str); 4] = [
    ("help-guide", "https://marbble.dev/guide/"),
    ("help-whats-new", "https://marbble.dev/changelog/"),
    ("help-report", "https://github.com/JackMechem/marbble/issues/new"),
    ("help-website", "https://marbble.dev/"),
];

/// The menu bar, menu by menu. The Edit menu is the Tracks area's Edit menu (src/components/
/// TracksArea.tsx `editMenu`), item for item; the names are src/menuLabels.ts's.
fn spec() -> Vec<Spec> {
    vec![
        Sub(
            "Marbble",
            vec![
                Spec::About,
                Sep,
                keyed("settings", "Settings…", "CmdOrCtrl+,"),
                item("settings-appearance", "Appearance…"),
                item("settings-keys", "Key Commands…"),
                // Logic Pro › Control Surfaces.
                Sub(
                    "Control Surfaces",
                    vec![item("learn-assignment", "Learn Assignment for Last Touched Parameter"), item("controller-assignments", "Controller Assignments…")],
                ),
                Sep,
                // Not the predefined Quit: GTK draws that one greyed out, and on macOS it would
                // end the app without asking about unsaved changes. This closes the main window,
                // as its close button does (`quit`).
                keyed("quit", "Quit Marbble", "CmdOrCtrl+Q"),
            ],
        ),
        Sub(
            "File",
            vec![
                keyed("new", "New…", "CmdOrCtrl+N"),
                keyed("chooser-recent", "Open Recent…", "Shift+CmdOrCtrl+O"),
                keyed("open", "Open…", "CmdOrCtrl+O"),
                Sep,
                keyed("save", "Save", "CmdOrCtrl+S"),
                keyed("save-as", "Save As…", "Shift+CmdOrCtrl+S"),
                item("save-template", "Save as Template…"),
                item("revert-saved", "Revert to Saved"),
                Spec::Revert,
                Sep,
                Spec::Alternatives,
                Sep,
                item("project-settings", "Project Settings…"),
                Sep,
                keyed("import-audio", "Import Audio File…", "Shift+CmdOrCtrl+I"),
                Sep,
                keyed("bounce", "Bounce Project or Section…", "CmdOrCtrl+B"),
                keyed("export-selected-tracks", "Export Selected Tracks as Audio Files…", "CmdOrCtrl+E"),
                keyed("export-tracks", "Export All Tracks as Audio Files…", "Shift+CmdOrCtrl+E"),
                item("import-midi", "Import MIDI File…"),
                item("export-midi", "Export Selection as MIDI File…"),
            ],
        ),
        Sub(
            "Edit",
            vec![
                keyed("undo", "Undo", "CmdOrCtrl+Z"),
                keyed("redo", "Redo", "Shift+CmdOrCtrl+Z"),
                Sep,
                // Cut/Copy/Paste are key commands, not accelerators, so text fields keep the
                // clipboard keys.
                item("cut", "Cut"),
                item("copy", "Copy"),
                item("paste", "Paste"),
                item("paste-original", "Paste at Original Position"),
                item("paste-replace", "Paste Replace"),
                keyed("duplicate", "Duplicate", "CmdOrCtrl+D"),
                Sub(
                    "Repeat",
                    vec![
                        item("repeat:1", "Once"),
                        item("repeat:2", "2 Times"),
                        item("repeat:3", "3 Times"),
                        item("repeat:4", "4 Times"),
                        item("repeat:8", "8 Times"),
                        item("repeat:16", "16 Times"),
                    ],
                ),
                Sep,
                keyed("split", "Split Regions at Playhead", "CmdOrCtrl+T"),
                keyed("join", "Join Regions", "CmdOrCtrl+J"),
                item("join-per-track", "Join Regions per Track"),
                check("loop", "Loop Regions"),
                item("convert-loops", "Convert Loops to Real Copies"),
                item("pack-takes", "Pack Take Folder"),
                item("set-locators", "Set Locators by Regions"),
                item("strip-silence", "Strip Silence…"),
                Sep,
                // Logic's arranging commands (src/arrangeCmds.ts).
                Sub(
                    "Section Between Locators",
                    vec![
                        item("section-insert-silence", "Insert Silence Between Locators"),
                        item("section-cut", "Cut Section Between Locators"),
                        item("section-repeat", "Repeat Section Between Locators"),
                        item("section-insert-cut", "Insert Cut Section at Left Locator"),
                    ],
                ),
                Sub(
                    "Trim",
                    vec![
                        item("trim-start-playhead", "Trim Region Start to Playhead"),
                        item("trim-end-playhead", "Trim Region End to Playhead"),
                        item("trim-end-next", "Trim Region End to Next Region"),
                        item("fill-locators", "Fill Within Locators"),
                        item("remove-overlaps", "Remove Overlaps"),
                        item("same-length", "Resize to Same Length"),
                        Sep,
                        item("crop-outside", "Crop Regions Outside Locators"),
                    ],
                ),
                Sub(
                    "Move",
                    vec![item("move-to-playhead", "Move Region to Playhead"), item("remove-gaps", "Remove Gaps"), item("delete-shuffle", "Delete and Shuffle")],
                ),
                Sub(
                    "Name and Color",
                    vec![
                        item("name-regions-by-track", "Name Regions by Track"),
                        item("name-tracks-by-region", "Name Tracks by Region"),
                        item("color-regions-by-track", "Color Regions by Track"),
                        item("color-tracks-by-region", "Color Tracks by Region"),
                    ],
                ),
                Sep,
                check("zero-crossings", "Snap Edits to Zero Crossings"),
                Sep,
                keyed("select-all", "Select All", "CmdOrCtrl+A"),
                Sub(
                    "Select",
                    vec![
                        item("sel-following", "All Following"),
                        item("sel-following-track", "All Following of Same Track"),
                        item("sel-inside-locators", "Inside Locators"),
                        Sep,
                        item("sel-muted", "Muted Regions"),
                        item("sel-overlapped", "Overlapped Regions"),
                        item("sel-same-color", "Same Colored Regions"),
                        item("sel-equal", "Equal Regions"),
                        item("sel-similar", "Similar Regions"),
                        Sep,
                        item("sel-invert", "Invert Selection"),
                    ],
                ),
                Sep,
                item("delete", "Delete"),
            ],
        ),
        Sub(
            "Track",
            vec![
                keyed("new-tracks", "New Tracks…", "Alt+CmdOrCtrl+N"),
                item("new-aux", "New Aux"),
                item("duplicate-track", "Duplicate Track"),
                item("same-settings", "New Track with Same Settings"),
                item("new-pattern-region", "New Pattern Region"),
                Sep,
                keyed("bounce-in-place", "Bounce in Place…", BOUNCE_IN_PLACE_KEY),
                item("freeze-track", "Freeze Track"),
                item("unfreeze-track", "Unfreeze Track"),
                check("track-on-off", "Track On"),
                Sep,
                keyed("folder-stack", "Create Folder Stack", "Shift+CmdOrCtrl+D"),
                item("summing-stack", "Create Summing Stack"),
                keyed("group", "Group Selected Tracks", "CmdOrCtrl+G"),
                Sep,
                check("output-track", "Show Output Track"),
                item("hide-tracks", "Hide Track"),
                item("unhide-all", "Unhide All Tracks"),
                Sep,
                keyed("delete-track", "Delete Track", "CmdOrCtrl+Backspace"),
                item("delete-unused", "Delete Unused Tracks"),
                Sep,
                Sub(
                    "Track Alternatives",
                    vec![
                        item("track-alt-new", "New Alternative"),
                        item("track-alt-duplicate", "Duplicate Alternative"),
                        item("track-alt-rename", "Rename Alternative…"),
                        item("track-alt-delete", "Delete Alternative"),
                    ],
                ),
            ],
        ),
        Sub(
            "Record",
            vec![
                item("play", "Play / Stop"),
                item("record", "Record"),
                Sep,
                check("cycle", "Cycle"),
                check("autopunch", "Autopunch"),
                check("metronome", "Metronome"),
                check("count-in", "Count-in"),
                Sep,
                check("low-latency", "Low Latency Mode"),
            ],
        ),
        // Logic's Navigate menu, for the Marker track.
        Sub(
            "Navigate",
            vec![
                item("create-marker", "Create Marker"),
                item("delete-marker", "Delete Marker"),
                Sep,
                item("previous-marker", "Go to Previous Marker"),
                item("next-marker", "Go to Next Marker"),
                Sep,
                item("marker-list", "Open Marker List"),
            ],
        ),
        Sub(
            "View",
            vec![
                check("library", "Library"),
                check("inspector", "Inspector"),
                check("mixer", "Mixer"),
                check("editor", "Editors"),
                check("list-editors", "List Editors"),
                check("browser", "Browsers"),
                check("automation", "Show Automation"),
                item("used-automation", "Show Used Automation"),
                check("global-tracks", "Show Global Tracks"),
                check("overview", "Show Overview"),
                Check("musical-typing", "Musical Typing", Some("CmdOrCtrl+K")),
                check("step-input", "Step Input Keyboard"),
                Sep,
                check("show-hidden", "Show Hidden Tracks"),
                Sep,
                item("zoom-in", "Zoom In"),
                item("zoom-out", "Zoom Out"),
                Sep,
                keyed("ui-larger", "Larger Interface", "CmdOrCtrl+Equal"),
                keyed("ui-smaller", "Smaller Interface", "CmdOrCtrl+Minus"),
                keyed("ui-actual", "Actual Size", "CmdOrCtrl+Digit0"),
                Sep,
                Sub(
                    "Theme",
                    vec![
                        radio("theme:studio", "Studio"),
                        radio("theme:flat", "Flat"),
                        radio("theme:midnight", "Midnight"),
                        radio("theme:light", "Light"),
                        radio("theme:contrast", "High Contrast"),
                    ],
                ),
            ],
        ),
        Sub(
            "Window",
            vec![
                // Not the predefined Minimize, which GTK doesn't have (`window_command`).
                item("minimize", "Minimize"),
                Sep,
                // A detached panel's own window, or back into the main window.
                check("detach:mixer", "Mixer in Separate Window"),
                check("detach:editor", "Editor in Separate Window"),
                check("detach:list", "Event List in Separate Window"),
                Spec::Windows,
            ],
        ),
        // The website's pages open in the browser (`HELP_PAGES`); Check for Updates… is the
        // frontend's (src/updates.ts).
        Sub(
            "Help",
            vec![
                item("help-guide", "Marbble User Guide"),
                item("help-keys", "Key Commands…"),
                Sep,
                item("help-whats-new", "What's New in Marbble"),
                item("help-updates", "Check for Updates…"),
                item("help-report", "Report an Issue…"),
                item("help-website", "Marbble Website"),
            ],
        ),
    ]
}

enum Handle<R: Runtime> {
    Item(MenuItem<R>),
    Check(CheckMenuItem<R>),
}

/// A built item: its handle, its label in `spec`, and where it sits (its index in the menu bar,
/// then in each submenu down to it, separators counted), which finds its GTK widget.
struct Entry<R: Runtime> {
    handle: Handle<R>,
    label: &'static str,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    path: Vec<usize>,
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
    /// The key command's key shown beside an item, as keymap.ts writes it ("Mod+Shift+V").
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
                    self.items.insert(id.to_string(), Entry { handle: Handle::Item(m), label, path: path.clone() });
                }
                Check(id, label, accel) => {
                    let m = CheckMenuItem::with_id(self.app, *id, *label, true, false, *accel)?;
                    parent.append(&m)?;
                    self.items.insert(id.to_string(), Entry { handle: Handle::Check(m), label, path: path.clone() });
                }
                Radio(id, label) => {
                    let m = CheckMenuItem::with_id(self.app, *id, *label, true, false, None::<&str>)?;
                    parent.append(&m)?;
                    self.items.insert(id.to_string(), Entry { handle: Handle::Check(m), label, path: path.clone() });
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
    app.on_menu_event(|app, ev| {
        let id = ev.id().0.clone();
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

/// Window › Minimize (the window in front) and the list of windows (brings one to the front).
/// False for other items.
fn window_command<R: Runtime>(app: &AppHandle<R>, id: &str) -> bool {
    if id == "minimize" {
        let windows = app.webview_windows();
        let front = windows.values().find(|w| w.is_focused().unwrap_or(false)).or_else(|| windows.get("main"));
        if let Some(w) = front {
            let _ = w.minimize();
        }
        return true;
    }
    let Some(label) = id.strip_prefix("window:") else { return false };
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    true
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

/// The windows Window lists, in its order: the main window, then the panel windows by title.
/// `gone` is a window being destroyed.
fn window_list<R: Runtime>(app: &AppHandle<R>, gone: Option<&str>) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| Some(label.as_str()) != gone && (label == "main" || crate::windows::is_panel_label(label)))
        .map(|(label, w)| {
            let title = w.title().unwrap_or_default();
            (label, if title.is_empty() { "Marbble".to_string() } else { title })
        })
        .collect();
    list.sort_by(|a, b| (a.0 != "main", &a.1).cmp(&(b.0 != "main", &b.1)));
    list
}

/// Brings Window's list of windows up to date, later on the main thread (a window may be in the
/// middle of opening or closing now), and draws the radio items in each window's menu bar.
fn refresh_windows<R: Runtime>(app: &AppHandle<R>, gone: Option<String>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(bar) = handle.try_state::<MenuBar<R>>() else { return };
        let list = window_list(&handle, gone.as_deref());
        let changed = {
            let mut shown = bar.shown.lock();
            let changed = shown.windows != list;
            shown.windows = list.clone();
            changed
        };
        if changed {
            if let Err(e) = bar.fill_window_list(&handle, &list) {
                log::warn!("Window menu: {e}");
            }
        }
        #[cfg(target_os = "linux")]
        for w in handle.webview_windows().values() {
            bar.draw_radios_in(w);
        }
    });
}

/// Marbble › Quit Marbble: closes the main window as its close button does, so the page can
/// ask about unsaved changes (App.tsx `onCloseRequested`); the panel windows go with it.
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

    /// Shows the key commands' keys in a window's GTK menu bar, unless it already shows them.
    /// GTK draws a key set on the item's label without binding it (`gtk_accel_label_set_accel`).
    #[cfg(target_os = "linux")]
    fn show_keys_in(&self, window: &tauri::WebviewWindow<R>) {
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
    fn draw_radios_in(&self, window: &tauri::WebviewWindow<R>) {
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

    /// Window's list of windows: a separator, then one item per window.
    fn fill_window_list(&self, app: &AppHandle<R>, list: &[(String, String)]) -> tauri::Result<()> {
        let (menu, fixed) = &self.window;
        for it in menu.items()?.iter().skip(*fixed) {
            menu.remove(it)?;
        }
        if !list.is_empty() {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
        for (label, title) in list {
            menu.append(&MenuItem::with_id(app, format!("window:{label}"), title, true, None::<&str>)?)?;
        }
        Ok(())
    }
}

/// A GDK keyval and its modifiers.
#[cfg(target_os = "linux")]
type GtkKey = (u32, gdk::ModifierType);

/// The item at `path` in a GTK menu bar (see `Entry::path`).
#[cfg(target_os = "linux")]
fn gtk_item(bar: &gtk::MenuBar, path: &[usize]) -> Option<gtk::MenuItem> {
    use gtk::prelude::*;
    let (first, rest) = path.split_first()?;
    let mut item = bar.children().get(*first)?.clone().downcast::<gtk::MenuItem>().ok()?;
    for i in rest {
        let menu = item.submenu()?.downcast::<gtk::Menu>().ok()?;
        item = menu.children().get(*i)?.clone().downcast::<gtk::MenuItem>().ok()?;
    }
    Some(item)
}

/// A key spec from keymap.ts as GDK's keyval and modifiers; None for no key or one GDK can't name.
#[cfg(target_os = "linux")]
fn gtk_key(spec: &str) -> Option<GtkKey> {
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
enum Key {
    /// A keyval name ("space", "BackSpace", "comma").
    Named(&'static str),
    /// A character with no name of its own here.
    Char(char),
}

#[derive(Debug, PartialEq)]
struct ParsedKey {
    ctrl: bool,
    alt: bool,
    shift: bool,
    key: Key,
}

const F_KEYS: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];

/// Reads a key spec as keymap.ts writes it: modifiers then the key, joined with "+" ("Mod" is
/// Ctrl outside macOS, where this is used; "Mod++" is the + key). None for an empty spec.
fn parse_key(spec: &str) -> Option<ParsedKey> {
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

/// An item's state from the frontend (src/menuBar.ts). Each field is sent only when it changed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemState {
    id: String,
    enabled: Option<bool>,
    checked: Option<bool>,
    /// The label, when it isn't `spec`'s (or back to it).
    text: Option<String>,
    /// The key command's key, as keymap.ts writes it; "" for none.
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

/// Shows the current keys in every window's menu bar (GTK, on the main thread).
#[cfg(target_os = "linux")]
fn show_keys<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(bar) = handle.try_state::<MenuBar<R>>() else { return };
        for w in handle.webview_windows().values() {
            bar.show_keys_in(w);
        }
    });
}

/// A window came to the front: a panel window opened since the keys last changed gets them
/// in its menu bar, and Window lists it under its current title (windows.rs `on_window_event`).
pub fn on_window_focused<R: Runtime>(window: &tauri::Window<R>) {
    #[cfg(target_os = "linux")]
    if let (Some(bar), Some(w)) = (window.app_handle().try_state::<MenuBar<R>>(), window.app_handle().get_webview_window(window.label())) {
        bar.show_keys_in(&w);
    }
    refresh_windows(window.app_handle(), None);
}

/// A window closed: one opened later under its label (a panel window) has a new menu bar, and
/// Window stops listing it.
pub fn on_window_destroyed<R: Runtime>(window: &tauri::Window<R>) {
    if let Some(bar) = window.app_handle().try_state::<MenuBar<R>>() {
        bar.shown.lock().applied.remove(window.label());
    }
    refresh_windows(window.app_handle(), Some(window.label().to_string()));
}

/// A backup in File › Revert to: its file (the item's id is `revert:<file>`) and its label in
/// local time, which the frontend formats.
#[derive(Deserialize)]
pub struct BackupItem {
    file: String,
    label: String,
}

fn fill_project_menus<R: Runtime>(
    app: &AppHandle<R>,
    alternatives: &Submenu<R>,
    revert: &Submenu<R>,
    names: &[String],
    active: &str,
    backups: &[BackupItem],
) -> tauri::Result<()> {
    for it in alternatives.items()? {
        alternatives.remove(&it)?;
    }
    for name in names {
        alternatives.append(&CheckMenuItem::with_id(app, format!("alternative:{name}"), name, true, name == active, None::<&str>)?)?;
    }
    if !names.is_empty() {
        alternatives.append(&PredefinedMenuItem::separator(app)?)?;
    }
    alternatives.append(&MenuItem::with_id(app, "alt-new", "New Alternative…", true, None::<&str>)?)?;
    alternatives.append(&MenuItem::with_id(app, "alt-edit", "Edit Alternatives…", true, None::<&str>)?)?;
    for it in revert.items()? {
        revert.remove(&it)?;
    }
    if backups.is_empty() {
        revert.append(&MenuItem::with_id(app, "revert-none", "No Backups", false, None::<&str>)?)?;
    }
    for b in backups {
        revert.append(&MenuItem::with_id(app, format!("revert:{}", b.file), &b.label, true, None::<&str>)?)?;
    }
    Ok(())
}

/// Rebuilds File › Project Alternatives (`alternatives`, `active` ticked) and File › Revert to
/// (`backups`, newest first). The frontend calls it when the project, its alternative or its
/// saved state changes (src/projectFiles.ts).
#[tauri::command]
pub fn set_project_menus<R: Runtime>(app: AppHandle<R>, alternatives: Vec<String>, active: String, backups: Vec<BackupItem>) -> Result<(), String> {
    let Some(menus) = app.try_state::<MenuBar<R>>() else { return Ok(()) };
    fill_project_menus(&app, &menus.alternatives, &menus.revert, &alternatives, &active, &backups).map_err(|e| e.to_string())?;
    // The alternatives' new items are drawn as radio buttons, and the main window's title (the
    // project's name) may have changed.
    refresh_windows(&app, None);
    Ok(())
}

/// How GTK writes keys in the app's language: each named key on its own, and each modifier as
/// the text put before a key ("Shift+"). GTK draws the menu bar's keys itself, translated
/// ("BackSpace" in the C locale, "Backspace" in English), so src/keyFormat.ts writes keys with
/// these names on Linux and the in-app menus and tooltips read like the menu bar. None
/// elsewhere.
#[derive(serde::Serialize, Debug)]
pub struct GtkKeyNames {
    keys: HashMap<&'static str, String>,
    mods: HashMap<&'static str, String>,
}

/// The keys `GtkKeyNames` names: those GTK spells its own way.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const NAMED_KEYS: [&str; 14] =
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
fn gtk_label((key, mods): GtkKey) -> String {
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

    fn walk<'a>(specs: &'a [Spec], out: &mut Vec<(&'a str, bool, Option<&'a str>)>) {
        for s in specs {
            match s {
                Item(id, _, accel) => out.push((id, false, *accel)),
                Check(id, _, accel) => out.push((id, true, *accel)),
                Radio(id, _) => out.push((id, true, None)),
                Sub(_, children) => walk(children, out),
                _ => {}
            }
        }
    }

    #[test]
    fn ids_are_unique() {
        let bar = spec();
        let mut all = Vec::new();
        walk(&bar, &mut all);
        assert!(all.len() > 120, "only {} items", all.len());
        let mut ids: Vec<_> = all.iter().map(|(id, ..)| *id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "two items share an id");
    }

    #[test]
    fn the_bar_holds_menus_and_one_of_each_list() {
        let bar = spec();
        assert!(bar.iter().all(|s| matches!(s, Sub(..))));
        let Sub("File", file) = &bar[1] else { panic!("File is second") };
        assert_eq!(file.iter().filter(|s| matches!(s, Spec::Revert)).count(), 1);
        assert_eq!(file.iter().filter(|s| matches!(s, Spec::Alternatives)).count(), 1);
    }

    #[test]
    fn window_and_help_come_last() {
        let bar = spec();
        let names: Vec<_> = bar.iter().map(|s| if let Sub(n, _) = s { *n } else { "" }).collect();
        assert_eq!(names, ["Marbble", "File", "Edit", "Track", "Record", "Navigate", "View", "Window", "Help"]);
        let Sub("Window", window) = &bar[7] else { panic!() };
        assert!(matches!(window.last(), Some(Spec::Windows)), "the list of windows ends Window");
        // Every Help item but Key Commands and Check for Updates opens a page, and every page has
        // its item.
        let Sub("Help", help) = &bar[8] else { panic!() };
        let ids = ids_in(help);
        for (id, url) in HELP_PAGES {
            assert!(ids.contains(&id), "{id}");
            assert!(url.starts_with("https://"), "{url}");
        }
        assert_eq!(ids.len(), HELP_PAGES.len() + 2);
    }

    #[test]
    fn a_submenu_is_greyed_out_when_none_of_its_items_applies() {
        let bar = spec();
        let Sub("Edit", edit) = &bar[2] else { panic!("Edit is third") };
        let Some(Sub("Repeat", repeat)) = edit.iter().find(|s| matches!(s, Sub("Repeat", _))) else { panic!() };
        let ids = ids_in(repeat);
        assert_eq!(ids, ["repeat:1", "repeat:2", "repeat:3", "repeat:4", "repeat:8", "repeat:16"]);
        let mut enabled = HashMap::new();
        // Nothing sent yet: as built, enabled.
        assert!(any_enabled(&ids, &enabled));
        for id in &ids {
            enabled.insert(id.to_string(), false);
        }
        assert!(!any_enabled(&ids, &enabled));
        enabled.insert("repeat:8".into(), true);
        assert!(any_enabled(&ids, &enabled));
        // A submenu's own submenus count.
        let Some(Sub("Select", select)) = edit.iter().find(|s| matches!(s, Sub("Select", _))) else { panic!() };
        assert!(ids_in(select).contains(&"sel-invert"));
    }

    #[test]
    fn quit_is_an_ordinary_item_with_ctrl_q() {
        let bar = spec();
        let mut all = Vec::new();
        walk(&bar, &mut all);
        assert!(all.contains(&("quit", false, Some("CmdOrCtrl+Q"))));
    }

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

    /// GTK draws the shown keys itself, in its own order and names; src/keyFormat.ts writes
    /// keys the same way on Linux. Both check the pairs in tests/gtkKeyLabels.json.
    #[cfg(target_os = "linux")]
    #[test]
    fn gtk_draws_keys_as_the_app_writes_them() {
        let pairs: Vec<(String, String)> = serde_json::from_str(include_str!("../../tests/gtkKeyLabels.json")).unwrap();
        assert!(pairs.len() > 20);
        for (spec, label) in pairs {
            let key = gtk_key(&spec).unwrap_or_else(|| panic!("{spec} has no GDK key"));
            assert_eq!(gtk_label(key), label, "{spec}");
        }
    }

    /// Untranslated (tests run in the C locale), GTK's names are those tests/gtkKeyLabels.json
    /// pins, which src/keyFormat.ts uses until the app sends the translated ones.
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
