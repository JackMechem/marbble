# Marbble roadmap

What's left, in the order it matters for a usable release. Each stage builds on the one before.
The full Logic Pro feature scope is in [LOGIC_FEATURES.md](LOGIC_FEATURES.md); known limitations
of what exists are in the [User Guide](docs/USER_GUIDE.md#20-known-limitations).

## Open items

What's left from the rounds so far, by topic. "Not heard" and "not verified" mean exactly that: a check
passed silently or on a stand-in, and nobody has tried the thing on real gear yet.

### Listening and hands-on checks

Things that pass their checks but want a person.

- **Synth models (Tre, Sei, Cinque, Uno, Sedici, Onda).** Unit tests, `instrument_check`, `rt_alloc_check` and
  `parallel_check` pass, as does a before-and-after render of Marbble Synth. Their panels have been seen in the
  docked row, and on 2026-11-04 the owner had listened to every preset and found them good, so they stay as they
  are. **None has been tuned or compared with the hardware,** which is deferred: Tre's likeness to the Model D is
  from documented behaviour only.
  - Every constant marked as a guess in [ARCHITECTURE](docs/ARCHITECTURE.md#built-in-instruments) and
    [Marbble Tre](docs/ARCHITECTURE.md#marbble-tre) wants tuning by ear.
  - The published figures in the Sei, Uno and Sedici sections came from memory of those sources: worth a check
    against them.
  - New instrument tracks still default to Marbble Synth: to decide after listening.
- **Marbble Sampler.** `sampler_check`, its unit tests, `rt_alloc_check` and `parallel_check` pass, silently,
  against generated audio. The panel has been opened in the dock, and bugs found that way are fixed (it now fills the
  dock, its wheel moves along the row, and percent parameters are right).
  - **Not heard:** drops haven't been tried, and nothing's been listened to. Try one recording played across the
    keyboard, a real instrument's samples (loops above all), a free SFZ library, and a drum loop sliced.
  - **Not tried on a real interface:** recording ran only through a simulated input. Try it through a real one,
    including the input menu's channels, an input with nothing plugged in, and the level bar.
- **Multi-core processing.** Checked in the app on a copy of a real project (5 TONE3000, 512 samples) through
  PipeWire to an M-Track 2X2, with Stereo Out at -∞: 56% average, 72% peak at 1 thread; 13-14%, about 20% at
  Automatic (6), stopped and playing. No late blocks, workers real time, and switching the setting during playback
  changed thread count without restarting audio.
  - **Not heard:** every check above was silent. Listen for dropouts and clicks, also when switching Processing
    Threads and editing routes (a send, an output, a side-chain) during playback. 128 samples was only measured on
    the null device.
  - **Left-out threads** (2026-11-04): refused real-time priority and the demotion of a stuck worker are tested headlessly
    (the demotion with real rtkit priority), not in the app. Not seen: Settings › Audio's note under Processing Threads
    (it needs rtkit refusing, e.g. with rtkit stopped), and the overload message for a stuck worker.
- **Track On/Off, drag to a new track, VST3 side-chain inputs, multi-output instruments, surviving SIGXCPU.** Checked in
  the running app on a virtual display: the new-track lane and drop, undo, the Off look, the Side Chain menu and SC
  badge, the "+", an output aux's name and Input slot, and TONE3000's editor opening on a copy of a real project.
  - **Not heard:** all of it ran stopped or on the null device. Listen for clicks when choosing a side-chain source
    and pressing "+" during playback, on a real interface.
- **Undo in a VST3's own editor** was checked by hand on TONE3000 with a model: the knob follows Undo and Redo, with no
  late blocks over three rounds while stopped. Not listened to during playback. The CPU meter's red state hasn't been
  seen, since nothing ran late.

### Look at in the running app

Nothing here has been checked by eye yet.

- **UI audit fixes, layout (2026-11-04).** Measured in headless Chromium against the real stylesheet (a harness with
  the showcase project), not in the app. The Mixer's and Inspector's faders, pan knobs and Read buttons on one line,
  Stereo Out, folder and VCA strips included; a strip with more than 6 inserts scrolling them with "+N"; track header
  buttons in columns at every height; the Compressor's floating panel opening at 703 px with nothing cut off; the fade
  above Tre's keyboard bar in Grid; Settings at one height with the footer in view on every tab and five theme cards in
  a row; the faint "+" in empty slots and the slot buttons on hover. Not seen at all: how the slots' thin scrollbar
  looks in WebKitGTK.

- **Menus, dialogs and the app shell, round 7 (2026-11-04).** Tests only, nothing seen. The menu bar in Light: a
  light bar (the theme's `--bg`, #e7e9ed) with dark titles, and each other theme's background behind the bar (Studio
  unchanged); switching themes back and forth, also in a detached window. View › Theme and File › Project
  Alternatives drawn as radio buttons, also after an alternative is added. Edit › Repeat, Move and Name and Color
  greyed out with nothing selected, enabled with a region selected. Window: Minimize, the three Separate Window items
  ticking, the list of windows (main window by its title, then each detached panel) updating as panels open and close,
  choosing one bringing it to the front. Help: each item opening the right page in the browser, Key Commands opening
  Settings › Key Commands; Help and Window in a detached window's menu bar too. Edit › Delete and the Tracks area's
  menus both reading "Backspace" (whatever GTK's language makes it). About Marbble with the logo. File › Save As from a
  debug binary started outside the dev shell: the dialog opens (the log says the schema was found); with
  `XDG_DATA_DIRS=/nowhere` and the binary built outside the shell, a "File Dialogs Unavailable" message and no crash.
  Settings: scroll down in Audio, switch to General: at the top. Project Settings: one height on every tab, the empty
  Notes without a scroll bar. The recovery sheet's Decide Later and its help lines (`kill -9` the app with unsaved
  changes). Tooltips: Stop, Rewind, Forward, Catch Playhead and the header M, S and R buttons with their keys. A send
  slot with the bus of another slot greyed out and marked. The Audio FX menu's VST3 heading.

- **Plug-in editors at HiDPI and on native Wayland (2026-11-04).** Checked headlessly only (Xvfb, and Weston with
  XWayland driving GTK and WebKit directly, not the app). At scale 1 nothing should differ: open LSP Compressor,
  Dragonfly Hall, ZamCompX2, CHOW Tape and TONE3000 as usual. With `GDK_SCALE=2` the same editors should open at twice
  the size, sharp (Zam's panel draws at 1x in the corner, as it does on its own). Then start with `MARBBLE_WAYLAND=1`:
  the log says "running natively on Wayland"; every one of those editors should open, draw, resize (CHOW's corner) and
  close, the Tracks area, Mixer meters and a detached Mixer should keep drawing while they're open, and no "X11 error"
  lines should appear. Not tried: NVIDIA, and plug-ins other than those five.

- **The round of 2026-10-29:** the Arrangement track (move, copy, delete with content); File › Revert to and Project
  Alternatives submenus updating on Linux; automation curve bending (Ctrl+Shift-drag) and extra lanes; the Output
  track; a looped region's loop-end crossfade; the region gain handle; the Tool menu (T) and Marquee; the Mixer's
  Setting button, peak readouts, gain-reduction bar, VCA slot and VCA strips; the Inspector's Region rows; the new
  effect panels and Linear Phase EQ; Event List, Step Sequencer and Step Input; Strip Silence; Controller Assignments;
  the name sheet replacing prompts; sheets holding back keys.
- **Effect panels:** the Loudness Meter's units at −100.0 and −∞; resizing floating panels from every edge and corner;
  the dock's tabs, float/dock, the preset sheet not being clipped; Esc order (frontmost floating panel, then the docked
  one).
- **Separate windows:** Mixer and Editor pop-out and back; key commands and ⌘A/⌘C/⌘V in a detached Editor; an effect
  slot clicked in a detached Mixer floats there; positions remembered across a relaunch and on a second monitor
  (Retina next to a 1× screen). Open Effect Panels › Separate windows, and each effect type remembering its window size.
- **The Mixer** scrolling when short, with every strip ending on the same line.
- **The menu bar (2026-11-04):** the key commands' keys beside their items (R, X, Space, Ctrl+C…) and a reassigned key
  following; typing R or X in a name field doesn't run Record or the Mixer; ticks following the View panes, Record's
  modes, Snap Edits to Zero Crossings, Track On and the theme, also after a toggle from the keyboard; Cut, Copy, Join and
  Delete greyed out with nothing selected and enabled with the Piano Roll focused; Undo naming the edit; the keys in a
  detached window's menu bar; Quit Marbble (Ctrl+Q) asking about unsaved changes. Checked by tests and a GTK widget
  check on Xvfb, not yet in the app.
- **The fixes of 2026-11-04** (checked headless in Chromium on the real stylesheet, not yet in the app): the bottom
  section staying clear of the Tracks area with a Mixer remembered 900 px tall, a shrinking window, and a panel opening;
  a 14-strip Mixer scrolling at 1480 px with the control bar on screen, also detached; a preset loaded by the showcase
  project showing by name, and after reopening.
- **macOS VST3 editors in the app** (Surge XT, Keyscape): open, raise, close, resize, cascade, hiding while another app is
  in front; Space, R and Musical Typing with a plug-in window in front; typing into a plug-in's text field; ⌘S running
  once; a project with a missing plug-in showing it struck through.
- **From before the move:** the overview strip in a few themes; track headers at 22–51 px and the meter-in-slider in the
  Light and Contrast themes; crossfade and fade-curve drawing and the Inspector's fade rows; the Drag pop-up; the Scroll
  to Playhead button; the recovery sheet after `kill -9` during a take.

### Windows and macOS: verify by hand

`dev` cross-checks and lints clean for both, and the engine tests and headless checks pass on the silent stand-in device
they use there (simulated on Linux), but none of this has run on a real machine.

- **Both:** audio devices listed in Settings › Audio, playback, recording and input monitoring through an interface; MIDI
  input and MIDI Learn; plug-in scanning; the file dialogs (open, save, import, bounce, export); detached Mixer, Editor
  and Channel EQ windows opening, closing, re-attaching and coming back in place after a relaunch, on two monitors with
  different scaling; native menu commands reaching the focused detached window; renaming a template or alternative by
  case only (both file systems ignore case); recovery after the app is killed during a take.
- **Windows:** the key commands' keys right-aligned in the menu bar (they go into the label after a tab);
  a detached window opening doesn't hang the app; Alt+` and other Alt key commands don't leave the focus in
  the menu bar; the NSIS installer and uninstaller, with LICENSE in the install folder; recording with Settings › Privacy ›
  Microphone off and on.
- **macOS:** the first recording asks for microphone access with the Info.plist text, in `tauri dev` and in the .app; ⌘ and
  ⌥ key commands; the unsigned .dmg opening with right-click › Open; `bash scripts/headless-checks.sh` (bash 3.2, no
  `timeout`).

### Known bugs and unexplained failures

- `soak_check <dir> 600 22` fails, on one thread as on eight: undoing every edit doesn't give the first document back. Not
  looked into. The default seed passes.
- **Dropouts while other work runs:** with builds and checks running on every core (some taking real-time priority), the
  app ran late, went choppy and hit a System Overload once, while total CPU read about 15%. A later 10 minutes with that
  work kept to the efficiency cores, including about a minute of `stress-ng`, had no PipeWire errors. The cause isn't
  proven, and nothing has been seen to line up with one specific test. If it comes back, the overload message now says what
  happened. `parallel_check`'s real-time part still takes real-time priority: run checks with `taskset` on other cores
  beside a live session.
- **Opening TONE3000's editor can blow the 10.7 ms soft real-time limit** (SIGXCPU on two of three first opens on a copy of
  a real project, ignored now, so it should only cost a dropout). What makes that block slow isn't known.
- **LSP's Multi-Sampler can lose a sample for good while loading:** one instrument stays silent, and a new instance or
  reopening the project brings it back. Measured with two samples and our plug-in host only, no engine: 1 load in 300 at
  audio-device pace, 9 in 300 calling `process` flat-out. It hasn't been tried in another host, so whether the fault is LSP's
  or in how we call it is unknown. `multiout_check` and `parallel_check` retry past it and print a line when it happens.
  `multiout_check` still failed once in a full run (2026-11-04, "undo rewires it … and it plays again", where undo makes a
  new instance that isn't retried), and passed 3 of 3 alone: likely the same loss.
- `controller_check` failed once in a full run, counting two MIDI messages too many ("other messages still play the
  instrument (2 -> 6)"), and passed 15 of 15 alone. Cause unknown.
- An undo that turns a latent track back on during a take can shift where the take lands.
- An LSP lookahead change didn't reach delay compensation after a render until the next `update_pdc`.
- In Low Latency Mode a monitored instrument skips its compensation delay but its output auxes keep theirs, so an aux can lag
  while monitoring.
- On a live Stop with delay compensation, the audio still in the compensation delay lines plays out at static levels (renders
  keep reading automation through it; playback doesn't).
- Marbble Synth's renders aren't repeatable with its mod wheel up (its LFO runs free, and `silence` keeps its controllers).
  Left alone, as old projects depend on it; a new default could drop it.

### Built-in instruments

- **Custom panels:** next, one for Tre. Sedici has 18 operator sections plus 4: long, even in the docked row. A custom panel
  for it (a tab per operator, the algorithm's diagram) would serve it far better. Onda's Slots would benefit from one too.
- **Sedici:** the velocity curve, pitch envelope and LFO delay are fits to the published shapes, not the tables. Its detune
  and keyboard scaling's exact break point are approximate.
- **Onda** has no effects section and no way to load a table: it's a code-generated set for now. Its table bank is 12 MB in
  memory once a first instance loads.
- **Sei's** envelope doesn't run at the original's control rate, and its DCO isn't quantized to the original's clock divisions.
- A filter type or oscillator table switched under a held note steps at once (Onda), as do Sei's and Cinque's range switches.
  On Tre, a wave or range switch moved under a held note steps at once, as a real switch does; it could click a little at an
  extreme setting.

### Marbble Sampler

- SFZ is a subset (see [ARCHITECTURE](docs/ARCHITECTURE.md#marbble-sampler)). Likely next by how often libraries need them:
  per-region filters, `amp_veltrack`, `group`/`off_by` for hi-hats, release triggers, `ampeg_hold`, and `loop_sustain` playing on
  past its loop.
- A channel strip setting or a template with a sampler keeps its zones, not its samples, so elsewhere they show as missing. A
  saved instrument is the way across projects.
- Converting a region keeps its gain, not its fades, loop or playback parameters.
- There's no disk streaming, so a library bigger than 4 GB loads in part and says so. Loading an SFZ's samples has no progress
  beyond a message.
- No input monitoring while recording into it. Ping-pong loops, per-zone filters, a zone's own LFO and zone groups aren't there.
- Copying a large library into the project takes the disk space of the library, with no link option yet.

### Multi-core processing

- A plug-in that never returns at all, on a worker, still leaves the engine silent for good: the worker is demoted so the app
  survives (and the project can be saved), but the block it holds never ends, and commands wait for it. Playing on would need
  the strip it holds (and everything after it) to be left out of later blocks.
- A worker retired after a give-up comes back only with Reset Audio Engine or a change of the setting. Bringing it back by
  itself would need a judgement on whether the plug-in that held it is safe again.
- Workers refused real-time priority play nothing, even where the callback itself didn't get it (where they would be no worse
  off than the callback). Whether the callback got it isn't known to the engine.
- Renders stay on one thread. They could go parallel with workers at normal priority.
- Automatic is 1 on macOS and Windows until workers get real-time priority there.

### Editing, mixing and small fixes

- Freeze and Bounce in Place refuse an instrument with output auxes; Export Region renders only its main output.
- Pattern regions restart their pattern where they're cut, so an Arrangement move or a section edit that cuts across one changes
  what plays (splitting by hand does the same, by design).
- Arrangement moves and section edits don't move the regions of a track's inactive alternatives.
- MIDI Learn has no VCA level target.
- The Compressor's "Lookahead" and the Classic Opto's "Peak Reduction" labels may overlap their knobs by a pixel or two.
- Only one Channel EQ analyzer per session: a detached EQ and another strip's EQ take turns.

### UI and design

What's left from the reviews and the UI audits of 2026-11-04. An independent re-audit after the round-6 fixes
graded the UI **7/10** (41 findings; report at `~/Desktop/marbble-ui-audit/ui-audit-2.html` on the owner's
machine). Its top items:

- The Piano Roll's toolbar runs off the edge at 1280 px (Height, Zoom and the detach button out of reach); the
  docked Mixer cuts off its faders at 1480×920.
- Plug-in panels have three different headers (Compressor/Limiter, Channel EQ, instruments); in the dock, instrument
  tabs pile up, × closes only the front one, and "Float in its own window" vs "Open in Separate Window" is unclear.

- **Design system, round 6 (not seen in the app yet):** a six-size type scale (body 11.5 → 12 px, small text
  9–10.5 → 11 px), every hex colour in a token, a focus ring for Tab, knobs and faders as keyboard sliders,
  `aria-label`s on icon-only buttons, a grey Read and power button, labels on Count-in and the zoom sliders, a
  CPU percentage. Look over the dense views (track headers at all three heights, the Mixer, the effect panels,
  the control bar) for text that now clips, in Studio, Light and High Contrast. Left over:
  - Canvas text (ruler, region names, notes, the effect panels' graphs) is drawn at 8–11 px in TypeScript and
    isn't on the scale.
  - 36 `rgba()` colours remain in area files (white or black at an opacity: hover tints, zero lines, recessed
    edges). Some are wrong on Light (white lines on a pale ground); worth tokens of their own.
  - Only the track switches and the control bar's view buttons say they're pressed (`aria-pressed`): Cycle,
    Metronome, Catch and the rest don't. Slots, menus and region lanes are still mouse-only.
  - Autopunch and Replace are still icon-only mode buttons.
- **Instrument panels are generic grids:** labels repeat their section ("Osc 1 Range" inside "Oscillator
  1"), toggles read "Osc 1 On", and sections leave large gaps (Tre's Grid at the docked height scrolls to its
  Voices and Touch rows). Custom panels (Tre next) fix this one instrument at a time.
- **Track inspector, what's left:** Logic's also has per-track MIDI transpose, velocity offset, key and
  velocity limits, delay, Q-reference and a freeze mode (Pre/Source); none of these exist in the engine yet, so the
  section shows what the track has (name, colour, input, output, freeze). The instrument is picked on the strip
  below it, not in the Track section. Track icons follow the track's kind and can't be chosen.
- **The Tempo track adds a point on a click near the line** (6 px) or a double-click anywhere; a double-click on
  the line still adds and then removes a point (the second click finds the new one). Worth a look in the app.
- **The global tracks take about 200 px by default** (Marker, Arrangement, Signature, Tempo) on a project
  that uses only the Arrangement track; showing only the ones in use, or a shorter Tempo track, would give
  the tracks back.
- **The CPU meter's** red state (a late block) hasn't been seen.

- **From the UI audit of 2026-11-04** (all menus, Settings, dialogs, Mixer, editors, Light theme):
  - The menu bar on macOS doesn't show the key commands' keys (R, X, Space…): AppKit can't draw a key without binding
    it, and a bound bare letter would be taken from text fields. Linux and Windows show them (menu.rs).
  - Two pop-up styles (Settings vs toolbars).

### Larger

- The settings folder is `~/.config/marbble` on macOS too; `~/Library/Application Support` is the native place, but moving it
  needs a migration.
- VST3 editor windows on Linux don't pass unused keys to Marbble, so Ctrl+Z and Space do nothing with focus in one; macOS does
  (`src/pluginKeys.ts`). Tried on 2026-10-30, not implemented: "forward unless X focus is in a plug-in child" would steal typing
  from LSP's text fields, which don't take X focus (from reading sources, unmeasured). Likely mechanism: a sync `XGrabKey` on the
  frame with `XAllowEvents` replay, on its own thread. Rule to be chosen by measuring whether the I-beam cursor is readable on JUCE
  and LSP; the fallbacks are chords only, or a per-window toggle. It needs a test under XWayland on a real display.
- CI: run `scripts/headless-checks.sh` on macos-latest too (it runs on macOS now), and open a draft PR from `dev` so the Linux
  headless step runs on GitHub's runners.
- Signing and notarization on macOS (the entitlements are ready in `src-tauri/`).
- Code style: about 60 clippy style warnings in the engine (`needless_range_loop`, `manual_is_multiple_of`, argument counts) and
  no rustfmt configuration (the code uses long lines, so plain `cargo fmt` would rewrite most files). A `rustfmt.toml` with a wide
  `max_width`, one formatting pass and a CI `cargo fmt --check` step would settle it.

- Native Wayland is an opt-in. Before it could be the default: a run of the app itself on Hyprland and on GNOME,
  NVIDIA's driver, and plug-ins beyond the five tried (anything that draws with GL inside a call that isn't wrapped
  in `gl_isolated`, or runs a GTK dialog of its own). Editors as native Wayland windows would need plug-ins with a
  Wayland embedding type, which VST3 doesn't have; out-of-process editors (Bitwig's approach) would sidestep the GL
  question entirely but are a much larger change.
- Plug-in editor scale isn't updated while an editor is open (no notification of an XSETTINGS or `Xft.dpi` change is
  watched); reopening applies it.

### Website

- `.github/workflows/pages.yml` hasn't run yet: it can only be tested by pushing to `main`, with Settings › Pages › Source on GitHub
  Actions.
- Screenshots are real captures of the showcase project (`cargo run -p daw-engine --example showcase_project -- <out.marbble>`)
  on a private Xvfb display at 2x; `site/public/screenshots/manifest.json` lists each slot. Retake them when the UI changes.

## Stage 1: Release hygiene

Goal: prereleases that install cleanly and that people can trust with their projects.

- [ ] Code signing on Windows (Authenticode) and signing plus notarization on macOS
- [x] A LICENSE file (MIT, shipped in every bundle)
- [ ] Hands-on testing of the Windows and macOS builds: devices, MIDI, plug-in scanning,
      installers, file dialogs
- [x] Automation lanes follow a plug-in moved to another track
- [x] Make `takes_check` "autopunch trims the take" deterministic (it was a real race: a
      position read right after a locate could return the old playhead)
- [x] Run the headless checks that need no plug-ins in CI on Linux (`scripts/headless-checks.sh`)

## Stage 2: Plug-ins on every platform

Goal: a VST3 user on Windows or macOS gets the same workflow as on Linux.

- [ ] VST3 editor windows on Windows (HWND embedding)
- [x] VST3 editor windows on macOS (NSView embedding)
- [x] VST3 editors with the interface on native Wayland (opt-in, `MARBBLE_WAYLAND=1`; the
      editors stay XWayland windows)
- [x] HiDPI scaling for plug-in editors (`IPlugViewContentScaleSupport`, Linux)
- [ ] ASIO on Windows
- [x] Side-chain inputs for VST3 plug-ins
- [x] Multi-output instruments

## Stage 3: Audio editing

Goal: clean, fast audio editing and comping without workarounds.

- [x] Crossfades between adjacent regions, and automatic crossfades on edits
- [x] Fade curve shapes and fade types (equal power, speed-up, slow-down)
- [x] Drag modes: No Overlap, Shuffle Left and Right (Overlap and X-Fade are done)
- [x] Crossfades for take folders and looped regions; Logic's Crossfade Time and Curve settings
- [x] Marquee tool, a Tool menu in the tracks area, and Scissors, Glue, Fade, Pencil and Eraser
- [x] Region gain drag; snap edits to zero crossings
- [x] Region drags in bars and beats across tempo changes
- [x] Several comps per take folder; MIDI cycle recording into take folders
- [x] Move automation with regions
- [x] Trim AAC/M4A encoder delay exactly on import
- [x] Logic's arranging commands: select by criteria, section edits between the locators, Trim,
      Remove Gaps, Paste Replace, Strip Silence

## Stage 4: MIDI and control

Goal: precise MIDI editing and hands-on control from hardware.

- [x] MIDI learn and controller assignments for plug-in and mixer parameters
- [x] Event List
- [x] Step Input (Musical Typing, MIDI keyboard, rests)
- [x] Region playback parameters: non-destructive quantize, transpose, velocity, delay
- [x] Record MIDI in Replace mode
- [x] Chase notes that started before the playhead
- [x] Controller curve shapes

## Stage 5: Mixing and delivery

Goal: finish a mix and deliver it without leaving Marbble.

- [x] Channel strip settings; copy and paste a whole strip
- [x] Several automation lanes per track, automation curves, automation for folder stacks and
      Stereo Out
- [x] Undo for parameter changes made in a VST3's own editor
- [x] VCA faders
- [x] A pre-fader side-chain option for built-in effects
- [x] Clip indicators and a gain-reduction meter on the strip
- [x] FLAC and AIFF bounce; export the selected tracks
- [ ] MP3 and AAC bounce (needs an encoder whose licence suits)
- [x] Delay compensation changes while playing without clicks
- [x] More built-in effects: modulation (chorus, flanger, phaser, tremolo), distortion, a Gain
      utility
- [x] A linear-phase EQ

## Stage 6: Advanced production

Goal: the larger Logic features that set a full DAW apart.

- [ ] Flex Time
- [ ] Flex Pitch
- [x] Track alternatives; project alternatives and backups; user project templates
- [x] Arrangement track; markers with their own lengths
- [x] Step Sequencer: pattern regions, rows of steps, edit modes, polymeter, swing
- [x] Multi-core track processing: Settings › Audio › Processing Threads, bit-identical to one thread
- [ ] Score Editor

## Stage 7: Formats and reach

Goal: more plug-ins, more people.

- [ ] CLAP hosting
- [ ] Audio Units on macOS
- [ ] Screen reader support
- [ ] Translated interface
