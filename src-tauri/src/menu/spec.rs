//! The menu bar as data: every menu, item, check and radio item with its id, label and the
//! accelerators that belong to the menu (see `mod.rs` for which keys those are), and the web
//! pages Help opens.

/// Logic's ⌃B. Elsewhere Ctrl+B is already Bounce Project (⌘B), so Bounce in Place adds Alt.
pub(super) const BOUNCE_IN_PLACE_KEY: &str = if cfg!(target_os = "macos") { "Ctrl+B" } else { "Ctrl+Alt+B" };

/// One entry of the menu bar.
pub(super) enum Spec {
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

pub(super) use Spec::{Check, Item, Radio, Sep, Sub};

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
pub(super) const HELP_PAGES: [(&str, &str); 4] = [
    ("help-guide", "https://marbble.dev/guide/"),
    ("help-whats-new", "https://marbble.dev/changelog/"),
    ("help-report", "https://github.com/JackMechem/marbble/issues/new"),
    ("help-website", "https://marbble.dev/"),
];

/// The menu bar, menu by menu. The Edit menu is the Tracks area's Edit menu (src/components/
/// src/tracks/TracksArea.tsx `editMenu`), item for item; the names are src/app/menuBar/menuLabels.ts's.
pub(super) fn spec() -> Vec<Spec> {
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
                // Logic's arranging commands (src/tracks/arrangeCommands.ts).
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
                item("drum-replace", "Drum Replacement/Doubling…"),
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
                item("discard-recording", "Discard Last Recording"),
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
                Sep,
                // How the engine, device and threads are doing, and the log (src/diagnostics/).
                item("diagnostics", "Diagnostics"),
                Spec::Windows,
            ],
        ),
        // The website's pages open in the browser (`HELP_PAGES`); Check for Updates… is the
        // frontend's (src/app/updates/updates.ts).
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
