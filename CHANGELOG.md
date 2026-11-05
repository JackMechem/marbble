# Changelog

All notable changes to Marbble are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Marbble uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Window and Help menus.** Window has Minimize, the Mixer, Editor and Event List in Separate
  Window items (moved from View) and a list of the open windows to bring one to the front. Help
  opens the User Guide, the changelog, a new GitHub issue or the website in the browser, and
  Settings › Key Commands.
- **Snap: Division and Ticks.** Division is a quarter of a beat (the position display's third
  number: a sixteenth in 4/4, a thirty-second in 6/8), Ticks 1/240 of that. In the Snap pop-up,
  the ruler's menu and Settings › Editing.
- **The Step Sequencer picks rows for the instrument.** A new pattern region on a track that
  already plays notes gets a row for each pitch it plays (up to 16); on a kit (the Sampler, or an
  instrument or track named Drums, Kit, Perc…) the General MIDI kit; on any other instrument an
  octave of the project key's scale from C3. A pitched instrument's rows are named by note, not
  by drum, and + adds the next note down.

- **HiDPI plug-in editors on Linux:** each editor is told the display's scale as it opens
  (`GDK_SCALE`, the desktop's GTK scale, or `Xft.dpi`), and its window takes the size the plug-in
  gives at that scale. LSP, Dragonfly, Zam, CHOW Tape and TONE3000 draw at 2x; nothing changes at
  1x. `scripts/editor-scale-check.sh` checks it on a private Xvfb display.
- **Native Wayland, opt-in:** `MARBBLE_WAYLAND=1` runs the interface natively on Wayland.
  Plug-in editors still open through XWayland, with their OpenGL kept apart from GTK's, so DPF
  editors (Zam, Dragonfly) open there too. The default stays XWayland.
- **Keyboard control of knobs and faders.** Tab reaches the channel strip's fader, pan and send
  knobs, the track headers' volume sliders, the master volume, VCA faders and the built-in
  effects' knobs; the arrow keys step them (Shift for fine), Page Up and Page Down step further,
  Home and End go to the ends. A change goes the way a drag does, so it writes automation in
  Touch and Latch and a run of presses undoes as one step. Screen readers hear each control as a
  slider with its name and value, and buttons that show only an icon by their tooltip.
- **A keyboard focus ring.** What Tab reaches shows a ring in the accent colour; clicks never do.
- **The CPU meter shows its load as a percentage** beside the label.
- **Double-click a region to open its editor:** the Piano Roll for a MIDI region, the Audio Track
  Editor for an audio region, as the Step Sequencer already opened for a pattern region.
- **The ruler has a right-click menu:** Move Playhead Here, Create Marker Here, Cycle, Autopunch,
  Set Locators by Regions and Snap.
- **Rename the old projects folder:** while `~/Music/Fermata` is the projects folder, Settings ›
  General offers Rename to Marbble. It refuses while a project from it is open or anything is
  called `~/Music/Marbble`, never copies or overwrites, and takes Recent Projects, the folder
  settings and the projects' track heights along.
- **The Inspector's Track section edits the track:** Name, Color (the colour grid), Input (audio
  tracks) and Output pop-ups, and Freeze, besides the type, instrument, region count and group.

### Changed

- **One kind of button wording in sheets:** Bounce in Place confirms with **Bounce** and Strip
  Silence with **Strip Silence** instead of OK; the Project Chooser's dismiss button is
  **Cancel** at launch too. The recovery sheet's Cancel is **Decide Later**, and a line for each
  button says what it does.
- **The menu bar's one-of-several choices are radio items on Linux** (View › Theme, File ›
  Project Alternatives), and a submenu whose items all don't apply is greyed out (Repeat, Move,
  Name and Color with no regions selected), as in the Tracks area's menus.
- **Tooltips show the key of every control that has one**, from Settings › Key Commands, so a
  reassigned key shows: Stop, Rewind, Forward, Catch Playhead and the track headers' Mute, Solo
  and Record Enable now do too.
- **The Audio FX menu has a VST3 heading** over the VST3 categories, so Delay, Dynamics, EQ
  and Reverb don't appear to be listed twice; the VST3 list by vendor is **By Manufacturer**.
- **A send slot greys out the buses the strip already sends to**, marked "Already sent".
- **Project Settings keeps one height** on every tab, as Settings does.
- **The editors' bars fit narrow panes.** At 1280 px, at 125 %, or with the Inspector and Library
  open, the Piano Roll, Event List, Step Sequencer and Audio Track Editor drop their captions
  (each control keeps its tooltip), then shorten their sliders, then move what's left to a
  second row; nothing runs off the edge. The control bar tightens at 1280 px and 125 % so its
  view buttons stay in the window.
- **The Mixer opens with its faders.** Its default height is 560 px (was 400), and when it's
  shorter than a strip the Audio FX and Sends sections show fewer rows before the fader shrinks
  below 120 px; when even that doesn't fit, the strips scroll from the bottom so faders, meters
  and names stay in view. The selected track's strip has its name plate lit.
- **The Piano Roll gives the notes more room.** The Editors pane opens at 360 px (was 300), the
  velocity lane takes a third of the height and keeps that share as the pane is resized, and
  opening a region scrolls its whole pitch range into view (it centred on the average pitch,
  which could leave notes below the view).
- **Secondary text is easier to read:** the dim and faint text colours are at least 4.5:1 on
  every surface in every theme (Studio's faint text was 2.4:1 on some), checked by a test.
- **One pop-up style.** Settings and the dialogs use the toolbars' neutral pop-up instead of a
  blue chevron box.

- **A type scale.** The interface uses six text sizes (10 to 17 px) instead of twelve with half
  pixels among them. Text is 11 px or more (the body went from 11.5 to 12 px, the smallest
  labels from 9–10.5 to 11 px); 10 px is kept for numbers inside fixed readouts such as meter
  scales and the LCD's captions.
- **Quieter defaults.** The automation mode button is grey in Read, as in Off, and keeps its
  colour for Touch, Latch (orange) and Write (red). The track power button is grey while the
  track plays and stands out only when the track is off.
- **Labels instead of bare icons:** the Count-in button reads "Count-in" (it read "1234"), the
  zoom sliders in the Tracks area, Piano Roll and Audio Track Editor read "Height", "Zoom" and
  "Wave", and the Piano Roll's velocity readout says "No notes" instead of "–" when nothing is
  selected.
- **Every colour comes from the theme.** The colours written into the stylesheets moved into
  theme variables. In High Contrast, text on a selection, a menu highlight, a default button and
  the Mute button is black for legibility; the effect panels' peak-hold lines show on the Light
  theme; Marbble Sampler's record and level colours follow the theme's red and green.
- **The metronome has one home, Project Settings › Metronome,** with its defaults for new
  projects (Use for New Projects, Revert to Default, Factory Settings) under New Projects. The
  Settings window no longer has a Metronome tab holding the same per-project settings.
- The interface no longer compares Marbble to Logic Pro (the metronome's footer and the About
  box).
- **The menu bar agrees with the app's own menus.** Every item with a key command shows its key
  (Space, R, C, K, X, Y, Ctrl+C…), taken from Settings › Key Commands, so a reassigned key shows
  there too. The keys are only shown, not bound by the menu bar, so typing in a text field or
  with Musical Typing is untouched and nothing runs twice. Toggles are ticked (View's panes,
  Record's Cycle, Autopunch, Metronome, Count-in and Low Latency Mode, Snap Edits to Zero
  Crossings, Track On, the theme), items that don't apply are greyed out as in the Tracks area's
  menus, and Undo and Redo name the edit. Both Edit menus now have the same items in the same
  order (Strip Silence, Pack Take Folder and Name and Color in both; Edit › Repeat is a submenu)
  and the same names (Cut, Copy, Paste; Track › Toggle Track On/Off is now Track On); a region's
  right-click menu drops "Regions" from them.
- **Shortcuts are written as the menu bar writes them:** on Linux GTK's order and names
  ("Shift+Ctrl+Z", "Ctrl+Right"), the same in the menu bar, the in-app menus, tooltips, Settings ›
  Key Commands and the User Guide.
- **Processing threads that can't be relied on are left out.** On Linux, a thread the system
  refuses real-time priority no longer plays tracks at normal priority, where it could be late:
  the others play on, or the audio thread alone, and Settings › Audio says so under Processing
  Threads. A thread given up on in a plug-in that stops responding is taken off real-time priority
  at once and left out until Reset Audio Engine, so the system can't kill Marbble for it, and the
  other threads play on once the plug-in returns.
- **The Mixer's rows line up.** Strips side by side show the same number of insert and send
  slots (as many as the strip that needs the most, up to 6 and 4), so faders, meters, pan knobs
  and automation buttons share one line; a strip with more scrolls its slots and its label shows
  "+N". Stereo Out, folder and VCA strips keep blank rows in place of sections they don't have.
  The Inspector's two strips line up the same way.
- **Track header buttons line up.** Each button has a column, with a gap where a track has no
  such button (I on instrument tracks, R, I and Freeze on auxes), on one-line headers too.
- **Mixer header:** the Audio, Inst and Aux filters have a light that shows whether they're on;
  "Mixer" reads as a title; Options also has New Aux, the filters, Show Hidden Tracks and Reset
  All Clip Indicators.
- **Slots show more of their names:** built-in instruments show their model (Tre, Sampler), with
  the full name in the tooltip, and a filled slot's power and pop-up buttons appear on hover, so
  "Compressor" fits. Empty insert and send slots show a faint "+".
- **The Settings window keeps one size:** a long tab scrolls, the footer stays at the bottom
  with Done on the right, and every tab's footer says how its changes apply. Appearance's five
  themes fit in one row.
- **Instrument panels** show a fade on the edge where the sections go on out of view (below in
  Grid, to the right in Row), with a line above the keyboard bar.
- **Click Moves Playhead defaults to Empty space only** for new installs, so clicking a region
  selects it without moving the playhead (Split at Playhead after a selection click works as in
  Logic). Settings saved before keep their choice; Anywhere and Never are still in Settings ›
  Editing.
- **Show Automation keeps M, S, R and I in the track headers.** On headers under 76 px the
  automation mode, the lane's parameter (no longer cut to "Volu…") and + take the volume
  slider's place instead of the buttons'.
- **Track Color is a grid** of 20 swatches, five to a row, each named in its tooltip and for
  screen readers, instead of a 20-row list. Region, marker, VCA and arrangement colours use it
  too.
- **The Marker row** above the track headers is a row of its own, level with the ruler's marker
  lane, like the other global tracks, with + and H above it.
- **A muted track's regions are drawn grey,** as a muted region is.
- **Region length is in bars and beats everywhere:** the Inspector's Length shows "4 0 0 0 ·
  00:08.000" like the Audio Track editor, where it showed only the time.
- **A single click in the Tempo track adds a tempo change only on the line;** away from it a
  double-click does, so a click that just lands in the lane no longer changes the tempo.
- **One header for every plug-in panel.** Effects, instruments, the Sampler and the Channel EQ,
  docked, floating or in their own window, show the same controls in the same order: power,
  name, track, presets, Side Chain, then the window buttons and ×. The window buttons keep their
  order and their names say where the panel goes: Float in Main Window or Dock in Bottom Section,
  then Open in Separate Window or Attach to Main Window, then Close (its tooltip names the
  panel). The Channel EQ's On button is now the power button, its presets moved into the header,
  and Reset, Copy and Paste into the preset menu (Reset to “Flat”, Copy Channel EQ, Paste Channel
  EQ); the Linear Phase EQ's Reset is Reset to “Default” there too.
- **Docked panels no longer pile up.** Up to five stay docked; opening a sixth closes the one
  looked at longest ago. Tabs keep their names whole and scroll sideways when they don't fit. The
  header's × closes the front tab and Shift-click closes them all; the other tabs have their own
  × on hover, a middle-click closes a tab, and a tab's right-click menu has Close, Close Other
  Tabs and Close All Tabs.

### Fixed

- **File › Save As (or any file dialog) no longer ends the app on Linux when GTK's file chooser
  schema isn't installed.** A binary started outside the dev shell finds the schema itself, and
  where it can't, a message says what to install and the project stays open.
- **The Light theme's menu bar on Linux** was near-black with dark, disabled-looking titles: the
  window's background behind the menu bar now follows the theme.
- **Keys read the same in the menu bar and the app's own menus on Linux** ("Backspace", not
  "BackSpace" in one and "Backspace" in the other): the app takes GTK's names, in the
  interface's language.
- **About Marbble shows the app icon** instead of a missing-image glyph.
- **Settings panes open at their top** rather than at the last pane's scroll position, and the
  empty Notes field in Project Settings no longer shows a horizontal scroll bar.
- **Plug-in meters read −∞ until audio runs.** A built-in effect's or instrument's meters started
  at zero, which the Compressor's and Limiter's In and Out meters showed as 0.0 dBFS while no
  audio device was open or before a new effect's first block. Each instance now starts with its
  own idle readings.
- **Knobs in a row line up** on instrument and Sampler panels: a name on two lines (Accent at
  Velocity, Wheel to Amp) no longer pushes its knob down. Knob names there take two lines' height.
- **Switching themes clears the last theme's own colours:** after Light, the R and I letters kept
  Light's darker red and orange in the other themes until a restart.
- **Long paths** in the Project Chooser and Settings › General start with "…/" at a folder
  boundary instead of being cut mid-name; the tooltip has the whole path.
- **Labels no longer wrap:** "Record Enable" in New Tracks sits beside its checkbox, Count-in
  stays on one line at 125 %, and "No notes" in the Piano Roll's bar stays on one line. The
  Step Sequencer's row names have room for "Closed Hi-Hat" and "Crash Cymbal 1".

- **The Compressor's floating panel opens wide enough** for its knobs; it opened with a sideways
  scrollbar and its Mix and Output knobs cut off. A floating panel that hasn't been resized now
  opens as wide as its controls need, and the Noise Gate's groups wrap in a narrow window.
- **Light theme:** the window's own theme follows Marbble's, so the GTK menu bar draws light under
  the Light theme. The R and I buttons' letters are darker there, so the I no longer fades into
  its button.
- **Stereo Out** no longer shows an empty Input slot.
- **The Inspector no longer shows one track's region over another track.** Choosing a track (its
  header, its Mixer strip, a right-click on its empty lane, Up or Down) deselects the regions on
  other tracks, as a click on an empty lane already did.
- **Marbble › Quit Marbble works and has Ctrl+Q.** It drew greyed out on Linux; it now closes the
  main window as its close button does, asking first about unsaved changes (also on macOS, where
  the old item quit without asking).
- **The bottom section can no longer cover the Tracks area.** A Mixer, editor or docked panel
  remembered taller than the window allows shows at the height there is room for, so the Tracks
  area keeps its menu bar, ruler, overview and some track rows, and the divider stays in reach.
  Dragging the divider starts from the height shown. This holds when the window is made smaller
  and when a panel opens, and a size saved too tall recovers without clearing anything.
- **A wide Mixer scrolls** instead of widening the whole window's layout: with more strips than
  fit, the control bar and the rest of the window stay on screen.
- **A preset loaded before shows by name.** Which preset a built-in instrument, effect or Channel
  EQ has loaded is now saved in the project, so the preset box shows it after reopening the
  project (it showed "Manual" for presets loaded by an earlier session or by a script), and undo
  takes it back with the settings. Renaming or deleting a user preset updates the slots that
  show it.
- An X error raised while a plug-in's editor first opened could end Marbble: the handler that
  logs such errors is now installed before the editor attaches.
- The first bounce or export after adding a send could differ slightly from the next one, as the
  send's level was still gliding up from wherever playing had left it. Renders now start every
  send at its level, as they did the faders.
- `parallel_check` failed now and then on a busy machine: its live runs play a debug build flat out,
  slower than real time when the cores are shared, and the engine shed load as for a real overload,
  at a block that depended on the load. Checks that ask for it (`ParallelTest::patient`) no longer
  shed load, and the check fails plainly if an overload happens anyway.
- `parallel_check`'s runs with workers refused real-time priority failed now and then, more often
  without LSP's plug-ins (as on CI): their output differed from the serial run's from the first
  block on. Not the pool: every block went through it, and the run with every worker refused,
  serial throughout, differed the same way. The live runs give a compressor a lookahead, and with
  an audio device running its latency reaches delay compensation only at the next `idle` after
  the engine has the new value. The refused runs call `idle` while their workers start, so
  whether they began with 240 frames of compensation or 76 depended on whether the null device
  had played a block by then. Every live run now lets the engine take the value and calls `idle`
  before it plays.
- **The Region inspector's rows no longer overlap** with an audio region selected: the rows
  (Delay, Gain, Fade In, Curve, Fade Out) were 19 px tall around 24 px number fields. Rows now
  fit their controls, "Fade Out" keeps to one line, and a style test guards it.
- **Clicking the global tracks or the ruler ends typing in a text field,** so the next key is a
  key command again (Y after the Library's search typed "y" into it).
- **Up and Down scroll the newly selected track into view.**
- **A pattern region made at the playhead no longer lands on top of a region:** Track › New
  Pattern Region and the Step Sequencer's Create Pattern Region use the first free bar from the
  playhead.

## [0.1.0-alpha.6] - 2026-11-03

### Changed

- **Fermata is now Marbble,** with a new icon. New projects are `.marbble` folders, and `.fermata`
  projects open and save as they are. At the first launch, settings, recent projects, the plug-in
  list, presets, saved instruments and view preferences move across from their old folders. An
  existing `~/Music/Fermata` stays the projects folder until a `~/Music/Marbble` exists. The
  website moves to <https://jackmechem.github.io/marbble/>.

### Added

- **Screenshots** on the website and in the User Guide: the main window, all six synth panels, Marbble Sampler, the Mixer
  during playback, the Compressor, the Piano Roll and a comped take folder, captured from the new `showcase_project`
  example. The logo's source is `scripts/logo.py`.

## [0.1.0-alpha.5] - 2026-11-02

### Added

- **Website:** a landing page and the User Guide, Architecture, Developing and Changelog as searchable
  pages, at <https://jackmechem.github.io/marbble/>. They're built from the Markdown in this repository,
  so they stay in step with it.

### Changed

- The code is reorganised into smaller modules, with no change in behaviour: the engine's session,
  processing, project model and the largest effects, the app's commands, and on the frontend the
  stylesheet, Piano Roll, Tracks area, Audio Track Editor, Settings, automation and the Compressor panel.
  Renders are bit-identical to alpha.4's, and the CPU cost is the same.
- Every module says what it's for, and DEVELOPING has a map of where things live and recipes for adding
  an instrument and a panel.
- CI checks formatting and runs clippy with warnings as errors, on a pinned toolchain.

### Fixed

- A project with a sampler saved on Windows now finds its samples on Linux and macOS: sample paths
  are kept with forward slashes.
- The User Guide lists Linear Phase EQ among the built-in effects.

## [0.1.0-alpha.4] - 2026-10-31

### Added

- **Marbble Sampler:** make instruments from your own sounds. Drop or choose audio files and play them across the
  keyboard at once, record straight into it from the audio input, or turn an audio region into one with Convert to
  Sampler Track, whole or sliced at its transients. Map samples across keys and velocities with loops and
  crossfades, one-shot, reverse, velocity layers and round robin, an amp envelope, filter with envelope, LFO, glide and
  polyphony. Several files map themselves by the note in their names, or chromatically. Save an instrument with its
  samples to use in any project; it appears in the preset menu, from `~/Music/Marbble Instruments`. A useful subset of SFZ loads, and what it
  doesn't do is listed. Samples are copied into the project, so it survives being moved or the originals deleted, and a
  missing sample shows plainly while the rest play. Samples are held in memory, up to 4 GB. It hasn't been heard or seen yet.
- **Built-in instruments and Marbble Tre:** a framework for instruments with settings, on the same
  footing as the built-in effects: a panel with sections and a small keyboard, docked or floating, factory
  and user presets, automation, MIDI learn, undo, saving, Freeze, Bounce and copy and paste. The first is
  Marbble Tre, a three-oscillator monophonic synth with a mixer that overdrives, a 24 dB transistor-ladder
  filter that self-oscillates in tune, two contours with a Decay switch, glide, oscillator drift, and osc 3 and noise as
  modulators, plus Unison and Poly. Modelled on the Minimoog Model D's documented behaviour (no affiliation), with
  19 factory presets; it hasn't been compared with the hardware. Marbble Synth stays the default for new instrument
  tracks, and its projects render exactly as before.
- **Five more synth models:** under Vintage Synths, **Marbble Sei** (six voices of one DCO with saw, pulse and sub, a 24 dB
  filter, one envelope with VCA gate, a four-position high-pass and a stereo bucket-brigade chorus with modes I, II and I+II;
  modelled on the Juno-106 family), **Marbble Cinque** (five voices of two oscillators with Poly Mod from the filter envelope and Osc B to
  Osc A's pitch and width and the filter; modelled on the Prophet-5), **Marbble Uno** (one oscillator, a diode ladder, accent with its sweep, and slide; from a MIDI region overlapping notes slide and high velocity accents;
  modelled on the TB-303) and **Marbble Sedici** (six-operator FM with the 32 algorithms, four-rate envelopes, keyboard scaling, 16 voices, at the original's 49.096 kHz with an optional converter character; modelled on the DX7). Under
  Modern Synths, **Marbble Onda**: two wavetable oscillators with position scan, band-limited tables generated in code, unison, seven filters, three envelopes,
  two LFOs and eight modulation slots. 14 to 16 factory presets each. Marbble isn't affiliated with any of those makers, and none has been compared with the hardware or heard yet.
- **Multi-core track processing:** independent tracks play at the same time on a small pool of
  real-time threads, with the audio thread working too, so a heavy project finishes each buffer
  sooner. Settings › Audio › Processing Threads chooses Automatic (the default) or a number; 1 keeps
  everything on the audio thread, and the change applies without restarting audio. The output is the
  same sample for sample however many threads play it. A light project stays on the audio thread alone,
  and bounces are unchanged. The CPU meter still shows how much of each buffer's time it takes to
  finish the buffer, and its tooltip now says how many threads are playing.
- **Undo for changes made in a VST3's own editor:** a knob moved in a plug-in's window is an undo
  step, labelled with the plug-in and the parameter ("TONE3000: Gain"), and Undo and Redo put the
  plug-in and its open window back. A plug-in that doesn't announce its drags gets a step per
  burst of edits; a preset loaded inside a plug-in that reports it is one step. Moves captured by
  automation being written stay in that pass's step, and neither automation playing, a controller
  assignment nor a plug-in moving its own parameters adds any. An edit on a bypassed plug-in now
  reaches it at once rather than when it's next switched on.
- **Dragging regions below the last track** creates a new track, as in Logic: the region is
  drawn on a new lane there while you drag, and the drop makes a track with the source track's
  settings (audio or software instrument, as the region). A selection spanning tracks gets one
  new track per source lane past the end. Alt-drag copies do the same, and one undo removes it
  all.
- **Track On/Off** (Logic's power button): in the track header, the Mixer, the track menu and
  Track › Toggle Track On/Off. An Off track makes no sound and runs nothing: no regions, no
  instrument, no EQ, no plug-ins, no sends, no delay compensation, so it frees the CPU that a
  muted track keeps using. Its plug-ins stay loaded and carry on when it's back on, without a
  tail or a click. A stack's Off reaches its subtracks. An Off track can't record, and doesn't
  appear in bounces or exports. It is saved with the project.
- **Side-chain inputs for VST3 plug-ins:** a plug-in with an aux input (LSP's Sidechain
  Compressor and Gate, say) gets a **Side Chain** item in its slot's menus, with the same choice
  of source and Pre / Post as a built-in effect's pop-up. With a source its aux input turns on
  and hears that track; with none it stays off and the plug-in uses its internal key. It works
  through Off tracks, Freeze, bounces, undo, duplicating and channel strip settings, and is
  saved with the project.
- **Multi-output instruments**, Logic's way: the strip of a VST3 instrument with extra outputs
  (a drum sampler's direct outputs, say) has a **+** that adds an aux for the next output, named
  after the track and the output, with its own fader, pan, plug-ins and sends. The aux stays in
  step with the main output through delay compensation, and bounces and exports include it. An
  output only runs while an aux uses it. Freeze and Bounce in Place refuse an instrument with such
  auxes for now.

### Changed

- **The CPU meter says what it measures and shows what the average hides.** Its tooltip explains
  that it's the audio thread's load, not the system monitor's figure, with the average and recent
  peak. A thin tick marks the worst single block for two seconds, and the label stays red once
  a block has gone over 100%, with a count in the tooltip, until you click the meter.
- **A System Overload message says what happened:** how long the buffer took and which track took
  longest, or which track a stuck processing thread is in.
- **Built-in instrument panels open in the bottom section,** with their sections in one row that the
  mouse wheel moves along. Row and Grid beside the panel's keyboard choose the layout; a floating panel
  starts in Grid, and each place remembers its own choice.

### Fixed

- Undoing or redoing the bypass of an instrument now reaches the running one, and a built-in instrument
  that gets bypassed lets go of the notes it was holding, rather than keeping them when it comes back.

- Holding a key no longer repeats toggles and one-shot commands: holding Space started and
  stopped the transport over and over. Moving, zooming and nudging still repeat.
- Dragging a region onto a frozen track lost it, and dragging one off a frozen track doubled
  it. Both now leave the region where it was, with the existing "is frozen" message.
- On Linux, one audio block taking longer than the buffer's length ended the app with SIGXCPU.
  Opening a heavy plug-in's editor could do it. It's now a dropout at worst.
- Editing during playback no longer restarts held notes. Adding, deleting or reordering a track,
  adding an aux, changing a send, moving a fader, selecting a track, switching the metronome or
  moving the cycle each released every instrument's notes and chased them again, which a sampler
  that ignores note-offs played doubled. Now a track's notes only restart when its own instrument
  or regions change, or when a key held on it loses its way: moving the MIDI target or turning
  MIDI Thru off with a key down releases that key, which with a VST3 instrument used to stick.
- Changing or removing an instrument during playback now releases the held notes and starts them
  on the new one, and so does editing a region, where the note used to stop until the next one.

## [0.1.0-alpha.3] - 2026-10-30

### Fixed

- With snapping on, moving a region failed with "invalid args `moves` for command
  `move_regions`" at most tempos other than 120 BPM, where a beat isn't a whole number of
  samples. Trimming regions, dragging a loop's end or an Arrangement section's end, setting
  the cycle in the ruler and adding a marker at the playhead could fail the same way. Snapped
  positions are now whole samples.

## [0.1.0-alpha.2] - 2026-10-30

### Added

- **Autosave after recording** (Settings › General › After Recording, on by default): the
  autosave is written as soon as a take is placed, and after Bounce in Place, Freeze and
  imports.
- **Recovering an interrupted recording:** takes being recorded when Marbble crashed, was
  killed or lost power are repaired and offered in the Recover sheet, placed where they would
  have landed (cycle takes as a take folder).
- Saves are atomic; a damaged project file falls back to the autosave, then the newest backup
  (or `project.json.bak` when Backups is set to 0). A panic on the main thread writes an
  emergency autosave.
- **Comps and MIDI take folders:** a take folder keeps several comps (Create New Comp,
  Duplicate Comp, Rename Comp, Delete Comp and the comp list in its menu; the name bar shows
  the comp heard). Settings › Recording › MIDI › Create take folders makes each MIDI cycle pass
  a take, and MIDI recordings over regions take folders; MIDI folders play whole takes. Take
  folders crossfade with their neighbours, loops crossfade at their loop end and optionally at
  every repetition (Crossfade Loop Repetitions), and Settings › Editing › Crossfade Time and
  Crossfade Curve set the take folder, X-Fade and Crossfade menu crossfades (comp boundaries
  are now 20 ms by default instead of 10 ms).
- **Record MIDI in Replace mode:** Settings › Recording › Overlapping Recordings › MIDI ›
  Replace clears the notes and controllers in the recorded range of the regions underneath,
  and a **Replace** button in the control bar makes every recording, audio and MIDI, replace.
  Merge now merges recorded controllers too, not only notes.
- **Chase: Notes:** starting playback, jumping or cycling back in the middle of a long note
  starts it, as in Logic. Settings › MIDI › Chase switches note and controller chase.
- **Tracks-area tools:** Left-click and Ctrl-click (⌘-click) Tool pop-ups in the Tracks
  area's bar and Logic's Tool menu (T, then the tool's letter), with the Pointer, Pencil
  (empty MIDI regions), Eraser, Scissors (Alt+click for equal pieces), Glue, Mute, Fade (fades
  and crossfades by dragging) and Marquee tools, each with its own pointer.
- **Marquee selection:** a time range across tracks (the Ctrl-click tool by default). Delete,
  Cut, Copy and Paste act on the part of the regions inside it, and a Pointer click inside
  splits the regions at its edges.
- **Drag modes No Overlap, Shuffle R and Shuffle L** (the Drag pop-up in the Tracks area):
  No Overlap shortens a region that an edit overlaps; the Shuffle modes keep a track's regions
  butted together, closing gaps on delete and trim and swapping neighbours on a drag. Drags
  are drawn where they will land.
- **Settings › Editing › Move Automation with Regions** (Never, Always, Ask): a region moved
  along its track takes the automation under it along, in the same undo step.
- **Fades and crossfades:** a curve for each fade, Speed Up and Slow Down fades, and X, EqP
  and X S crossfades between adjacent or overlapping regions (Inspector, the region menu's
  Crossfade item, or the handle on the canvas). The X-Fade drag mode crossfades overlaps
  automatically. Bounce, export, freeze and Join render them as playback does.
- **Region gain handle:** hovering over an audio region shows its gain at the top centre of
  its name bar; drag it up or down (Shift for fine steps) to change the gain of every selected
  audio region by the same amount, with the waveform following, or double-click it for 0 dB.
  A drag is one undo step.
- **Snap Edits to Zero Crossings** (Edit menu, and a key command): splits, trims and range
  edits of audio regions move to the nearest zero crossing within 5 ms. Saved with the project.
- **Arranging commands** from Logic's Edit menu (also in the Tracks area's Edit and Functions
  menus, with Logic's keys where free): **Select** All Following (Shift+F), Inside Locators,
  Muted, Overlapped, Same Colored, Equal or Similar Regions, Invert Selection (Shift+I);
  **Section Between Locators**: Insert Silence, Cut Section, Repeat Section and Insert Cut
  Section across every track (regions cut at the locators, automation, markers and arrangement
  sections move with their time), or within the marquee; **Trim**: region start or end to the
  playhead, end to the next region, Fill Within Locators, Remove Overlaps, Resize to Same
  Length, Crop Outside Locators or Marquee; **Move Region to Playhead** (;), Remove Gaps, Delete
  and Shuffle, Paste at Original Position, Paste Replace, Join Regions per Track, and naming or
  colouring regions by their track and tracks by their region.
- **Strip Silence** (Edit › Strip Silence…): splits audio regions around the parts above a
  threshold, with Logic's minimum time, pre-attack and post-release settings and a preview of
  the cuts on the region's waveform.
- **Arrangement track** (the top global track, View › Show Global Tracks): named sections
  (Intro, Verse, Chorus…) that tile the timeline, added with +, renamed from Logic's name
  pop-up, coloured by name, and resized by their right edge. Dragging a section moves it with
  its content (regions on every track cut at its edges, automation and markers) and Alt-drag
  copies it; a section can be deleted with or without its content. Clicking one selects its
  regions.
- **Markers with their own lengths:** set from the marker menu's Length submenu, the Marker
  List's new Length column, or by dragging the end of a marker's bar. Clicking a marker selects
  it (Delete removes it); with Cycle on, or dragged into the cycle strip, it sets the locators.
- **Overview:** the Tracks area's horizontal scroll bar is now a miniature of the whole
  project (regions, playhead, cycle, markers, project end) with a box for the visible area:
  drag it to scroll, drag its edges to zoom, click elsewhere to jump there (View › Show
  Overview).
- **Scroll to Playhead** button beside Catch (Alt+\`); a second Stop also brings the playhead into view.
- **Settings › Editing › Mouse Wheel Scrolls:** Horizontally (time) makes a mouse's vertical
  wheel scroll through time in the Tracks area and Piano Roll, with Shift for vertical.
- **Region playback parameters** in the Inspector's Region section, as in Logic: Quantize
  (straight and triplet values) with Q-Swing, Transpose, Velocity, Dynamics, Gate Time and
  Delay (ticks or ms) for MIDI regions, and Delay for audio regions. They apply at playback
  and in every render, the Tracks area draws notes as they play, and region menu › Apply
  Region Parameters writes them into the regions.
- **Event List** (D, View › List Editors, or the Event List tab beside the Piano Roll): the
  notes, controllers, pitch bend and channel pressure of the selected MIDI regions as a list,
  with Position and Length in bars, beats, divisions and ticks through the tempo map. Drag any
  value or double-click to type it, filter by event type, add events at the playhead with +,
  delete with Backspace; the event at the playhead is highlighted and Catch follows it. The
  note selection is shared with the Piano Roll, and the list can have a window of its own.
- **Step Sequencer:** pattern regions on software instrument tracks (Track › New Pattern
  Region, or Create Pattern Region in an instrument track's right-click menu), edited as rows
  of steps in a new editor tab beside the Piano Roll and Event List (also in its own window).
  Note On, Velocity, Gate and Tie edit modes with click-and-drag painting, per-row pitch (with
  the General MIDI drum names and Learn), mute, solo, step rate and length (polymeter),
  pattern length, rate, swing and loop, and the playing step lit during playback. Pattern
  regions show their steps in the Tracks area, play in every render, and convert to and from
  MIDI regions.
- **Step Input Keyboard** (View › Step Input Keyboard, Ctrl+Alt+Shift+K, or the Piano Roll's
  bar): enter notes one step at a time at the playhead, from its own keyboard, a MIDI keyboard
  or Musical Typing, with note values 1/1 to 1/128 (dotted, triplet), velocities ppp to fff or
  as played, Rest, Back, Chord, Sustain and Quantize, and key commands for them. Notes go into
  the Piano Roll's region under the playhead, which grows or is made as needed; each step is
  one undo step.
- **Controller curves** in the Piano Roll's controller lane: the Line tool draws ramps along a
  Linear, Exponential, Logarithmic or S-Curve shape at a chosen Ramp Density, joins the
  points it starts and ends on, and Create Ramp and Thin Out fill and thin the selected points.
- **MIDI Learn and controller assignments** (Marbble › Control Surfaces): Learn Assignment
  for Last Touched Parameter (Ctrl+L) or a control's right-click Learn MIDI Assignment assigns
  the next control change or pitch bend to a fader, pan, send, Mute, Solo, Record, Stereo Out,
  Channel EQ setting, built-in effect or VST3 parameter, or a transport button. Assigned
  controllers move their parameter like the mouse (written as automation in Touch, Latch and
  Write) and don't reach the instrument. The Controller Assignments window (Ctrl+Shift+L) lists,
  edits (Jump or Pickup, range) and deletes them; they're saved with the settings.
- **More built-in effects:** **Chorus**, **Flanger**, **Phaser** and **Tremolo** (Modulation;
  the Tremolo syncs to the tempo, locked to the bar), **Overdrive** and **Distortion**
  (Distortion; Soft, Hard, Tube and Fuzz curves, 4× oversampled, level-compensated drive) and
  **Gain** (Utility: gain, phase invert, swap, mono, balance). Each has its panel, automation,
  factory presets and freeze and bounce support.
- **Linear Phase EQ** (EQ category): the Channel EQ's eight bands with linear phase, in the
  Channel EQ's own editor, with a **Quality** pop-up (Low, Medium, High: 21, 43 or 85 ms of
  latency at 48 kHz, made up by delay compensation). Settings change without clicks, follow
  automation, and render identically in bounces and freezes.
- **Delay compensation changes glide:** inserting or removing a latent plug-in, changing a
  plug-in's latency or switching Low Latency Mode while playing now crossfades each compensated
  path (track outputs, sends and auxes) to its new delay over 15 ms instead of clicking.
- **Channel strip settings:** the **Setting** button at the top of each strip (and the strip
  name's right-click menu) copies and pastes a strip's instrument, Audio FX (VST3 state
  included) and Channel EQ, resets the strip, and saves settings to load in any project.
  Paste, Reset and loading apply to all selected strips as one undo step.
- **Clip indicators:** each strip's peak readout holds its highest peak until reset, turns red
  over 0 dBFS and lights the top of the meter (and the end of the track header's meter); click
  to reset, Alt-click (or the key command Reset All Clip Indicators) to reset all.
- **Gain-reduction meter** beside a strip's level meter when it has a Compressor, Limiter,
  Noise Gate or De-Esser.
- **VCA faders:** Mixer › Options › Create New VCA, or a strip's new **VCA** slot. A VCA strip's
  fader offsets the level of every strip assigned to it without moving their faders (an
  outlined cap on each fader shows where it really plays), its Mute mutes them, and VCAs can be
  nested. Volume automation, bounces, exports and frozen tracks follow the VCA.
- **Pre-fader side-chains:** a **Pre / Post** switch beside the Side Chain pop-up of the
  Compressor, Noise Gate and De-Esser keys the effect from the source before its fader, so a
  trigger track pulled all the way down still works. Existing projects stay post-fader.
- **Automation lanes, curves, stacks and Stereo Out:** the **+** on a lane header opens more
  lanes below a track, each with its own parameter; **Show Used Automation** (Shift+A) shows
  every lane with data. Ctrl+Shift-drag (Control-Shift on macOS, or Alt+Shift) bends a segment
  into a curve; double-click straightens it. Folder stacks automate their volume and mute,
  and **Track › Show Output Track** shows Stereo Out with lanes for its volume and plug-ins.
  Bounces and exports render all of it.
- **Project alternatives** (File › Project Alternatives): versions of a project in one
  folder, sharing its audio files: New Alternative…, switching (the current one is saved
  first), and Edit Alternatives… to rename or delete them.
- **Backups and File › Revert to:** each save keeps the version it replaces, per alternative
  (Settings › General › Backups, 10 by default), listed by date and time; reverting keeps the
  current state as a backup too. **File › Revert to Saved.**
- **User project templates:** File › Save as Template… saves the tracks, routing, plug-ins and
  settings without regions; they appear under My Templates in the Project Chooser, where a
  right-click renames or deletes them.
- **Track alternatives** (Track › Track Alternatives and a pop-up in the track header): New,
  Duplicate, Rename and Delete sets of regions for a track, and switch between them, with undo.
- **AIFF and FLAC bounces:** Bounce, Export All Tracks and Export Region can write AIFF (16,
  24-bit or 32-bit float as AIFF-C) and lossless FLAC (16 or 24-bit) as well as WAV; the
  format is remembered with the other bounce options.
- **File › Export Selected Tracks as Audio Files…** (Ctrl+E): Export All Tracks for just the
  selected tracks.
- **Separate windows:** the Mixer, the Editors (Piano Roll, Audio Track Editor), built-in
  effect panels and the Channel EQ can be detached into windows of their own, for example on
  another monitor (the pop-out button in their bar, or View › Mixer / Editor in Separate
  Window). They stay in sync with the main window, reopen where they were, and come back onto
  a connected monitor if theirs is gone.
- **Docked effect panels:** built-in effect panels and the Channel EQ can dock in the bottom
  section, with a tab for each, or float (Settings › General › Open Effect Panels; Floating by
  default), or open in separate OS windows. Floating panels resize from their sides and bottom corners, their graphs and meters
  grow with them, and each effect type remembers its last size.
- **VST3 editor windows on macOS:** a plug-in's own interface opens in a window that floats
  above Marbble's, resizes within the plug-in's limits and hides while another app is in front.
  Keys the plug-in doesn't use (Space, key commands, Musical Typing) reach Marbble.
- A plug-in whose window can't open when it's inserted now says why, instead of only logging it.
- **Missing plug-ins:** a project opened where a plug-in isn't installed opens and plays
  without it, says which are missing, shows their slots struck through and keeps their
  settings. A plug-in installed in another folder than on the computer that saved the project
  (another OS, say) is found by itself.
- macOS: recording asks for microphone access, and builds carry the entitlements a signed app
  needs to load third-party plug-ins.
- The headless checks run on macOS too.
- A LICENSE file (MIT), shipped in every bundle.
- CI runs the headless checks that need no third-party plug-ins on Linux
  (`scripts/headless-checks.sh`).

### Changed

- Tracks can be as short as 22 px (View › Track Height has a new Mini size). Below 52 px the
  header goes onto one line and its M, S, R, I and Freeze buttons move into a ⋯ menu; the ones
  that are on stay visible.
- The track header's volume slider and level meter are one taller control: the meter fills
  the slider's groove.
- Automation lanes move with a plug-in dragged to another track's strip; one undo reverts both.
- Regions moved across tempo changes keep their bars and beats: with Snap on, the other
  regions of a drag (and Alt-drag copies) follow the grabbed one in bars and beats rather than
  in samples, and nudges by a bar, beat or note value count through the tempo map.
- Waveforms in the tracks area are drawn at their region's gain.

### Fixed

- The Mixer cut off the bottom of its channel strips when it was too short, with no way to
  scroll to them; it now scrolls, and every strip ends on the same line.
- A plug-in that failed to load reported only "dlopen failed"; the report now gives the reason,
  including a plug-in built only for Intel on Apple Silicon.
- The Loudness Meter drew "LUFS" and "LU" over its values.
- The Compressor's circuit tabs and other segmented buttons didn't show the current choice.
- Opening Group Settings (including from New Group from Track) blanked the window.
- Play or Record right after moving the playhead could start from the old position.
- M4A/MP4 imports started about 40 ms late: the encoder delay and padding are now trimmed
  exactly, from the iTunes gapless tag or the file's edit list.
- With Musical Typing on, Shift with a note key played the note instead of running its key
  command (Shift+F, Shift+S and the other Select commands, Shift+A, Shift+K, Shift+R).
- Step Input's keys (note values, Rest, Back…) did nothing in a detached Piano Roll's window.
- Controller Assignments… in a detached Mixer's or effect panel's right-click menu did nothing;
  the sheet now opens in the main window.
- Step Input, the Piano Roll and the Event List put notes into pattern regions, which play their
  steps instead, so the notes were never heard; they now leave pattern regions alone.
- Clicking the current project alternative in File › Project Alternatives, or cancelling a
  switch, left the menu's tick on the wrong alternative.
- Edit › Delete removed the selected regions even when automation points, a tempo change or a
  marker were selected; it now deletes those first, as the Delete key does.
- On macOS, renaming a region, take, comp, marker or arrangement marker, Save Channel Strip
  Setting As… and the Piano Roll's Transpose by…, Set Velocity…, Scale Velocity…, Custom
  Humanize… and Other Controller… did nothing: macOS's web view has no `window.prompt`. They now
  ask in Marbble's own name sheet (which detached windows show too).
- Key commands reached the app behind an open sheet once the focus was outside its text fields:
  Backspace deleted regions while Strip Silence was open after clicking its slider. Sheets
  (Strip Silence, Group Settings, the name sheet, Settings, Bounce…) now hold back key
  commands, Esc closes them and Return presses the default button. Controller Assignments
  stays non-modal, as in Logic: Learn Assignment and the transport keys still work.
- Copy Channel Strip Settings, Copy Plug-in and Copy Channel EQ in a detached Mixer couldn't be
  pasted in the main window (or the other way round): the clipboards are now shared.
- Edit › Select commands (Shift+I, Shift+F, Shift+M…) selected regions while the Piano Roll or
  Event List had focus; Invert Selection, All Following, Inside Locators, Muted, Same Colored and
  Equal now select its notes or events.
- Delete or Backspace with the Step Sequencer focused deleted the pattern region being edited;
  they now do nothing there.
- A VCA on a track inside a folder stack was ignored (level and mute) while the folder's
  volume or mute automation played.
- A folder stack's automation played early by the delay compensation (up to 85 ms with the
  Linear Phase EQ on High) instead of in line with its subtracks.
- With delay compensation, the last moments of a bounce or export (the plug-in latency) ignored
  automation and Stereo Out's lanes, so a fade to silence could end with a blip.
- Bounces and exports could play a plug-in parameter at a value set by hand instead of its
  automation in Read, on tracks and on Stereo Out; a plug-in put back on Stereo Out by undo
  waited for its lane to change. Bounces now also chase notes and controllers held over their
  start, as playback does.
- After undo, a plug-in could inherit the latency of a Linear Phase EQ (or other built-in
  effect) that had been undone, delaying the rest of the mix to compensate for nothing.
- Duplicate Track (and Move Regions to New Track) shared the original's track alternatives:
  the copy's alternatives held the same regions, so editing one changed the other. The copy now
  gets its own (or none, without content).
- Moving, copying or deleting an Arrangement section left Stereo Out's automation behind.
- Low Latency Mode turned off the wrong plug-in after inserts were reordered.
- Switching a track's or Stereo Out's automation to Off or Write left a built-in effect's
  automated parameter at the lane's last value, so the project sounded different once reopened;
  it now goes back to its own value.

### Known issues

- VST3 editor windows open on Linux (X11, through XWayland on Wayland) and macOS; on Windows
  plug-ins load, play and can be automated, but show no editor.
- Windows and macOS builds are early and have had little hands-on testing. Only the system's
  default audio driver is available there (WASAPI, Core Audio; no ASIO).
- The builds are unsigned: macOS needs right-click › Open (or `xattr -cr`), Windows
  SmartScreen needs More info › Run anyway.
- A region's length stays in samples when it is moved across a tempo change; only its start
  follows the bars and beats.
- VST3 side-chain buses are not connected, and VCA levels can't be automated or assigned to a
  controller.
- Pattern regions restart their pattern where they're cut, so an Arrangement move or section
  edit across one changes what plays. Arrangement edits don't move the regions of a track's
  inactive alternatives.
- The `takes_check` headless check "autopunch trims the take" fails about one run in five,
  from the null audio device's timing.

The full list is in the User Guide, [Known limitations](https://github.com/JackMechem/marbble/blob/main/docs/USER_GUIDE.md#20-known-limitations).

## [0.1.0-alpha.1] - 2026-10-29

The first prerelease. Everything below is new.

### Added

- **Recording:** audio from any input, mono or stereo per track, with live waveforms and
  latency compensation from the measured round trip plus a manual Recording Delay; MIDI with
  controllers, pitch bend and aftertouch; count-in and pre-roll; cycle recording into take
  folders; Autopunch; on top, new track or replace for overlapping recordings.
- **Take folders and comping:** Quick Swipe Comping with crossfaded boundaries; Flatten,
  Flatten and Merge, Pack and Unpack.
- **Tracks area:** move, trim, split, join, loop, fade, gain, reverse, colour, copy, paste,
  repeat, nudge and Alt-drag copy; rubber-band selection; Smart snap; a click without a drag
  moves the playhead (Settings › Editing › Click Moves Playhead).
- **Tracks:** audio, instrument and aux tracks; folder and summing stacks; groups; hidden
  tracks; Track Freeze.
- **Tempo and markers:** Tempo and Signature global tracks with tempo changes, linear ramps and
  signature changes, followed by the grid, click, quantize, plug-ins and MIDI files; Move MIDI
  Regions with Tempo; a Marker track and Marker List.
- **Editors:** Piano Roll with eight tools, Time Quantize with strength and swing, scale
  quantize, humanize and a velocity/controller lane; Audio Track Editor with range edits,
  normalize, fades to selection and Split at Transients; Standard MIDI File import and export.
- **Mixing:** channel strips with an eight-band Channel EQ and analyzer, eight Audio FX slots,
  eight pre- or post-fader sends, aux buses and output routing; plug-in delay compensation on
  every path, with Low Latency Mode.
- **Automation:** volume, pan, mute, send levels and plug-in parameters, in Off, Read, Touch,
  Latch and Write modes.
- **Plug-ins:** VST3 instruments and effects, scanned out of process with a scan report;
  Marbble Synth; copy and paste of plug-ins (VST3 state included) and Channel EQ settings from
  the right-click menus; dragging plug-ins between Audio FX slots to reorder them, move them to
  another strip, or copy them with Alt.
- **Built-in effects:** Compressor (Platinum Digital, Studio VCA, Studio FET, Classic Opto and
  Vintage Vari-Mu circuits), Limiter, Noise Gate, De-Esser, Reverb, Delay, Tuner and Loudness
  Meter, with their own panels, automation, side-chain input from any track (Compressor, Noise
  Gate, De-Esser), latency reported to delay compensation, and freeze support.
- **Presets:** factory and user presets for every built-in effect and the Channel EQ. User
  presets are files in the config folder's `presets/` and are shared by every project.
- **Bounce and export:** offline Bounce of the project or a section, Export All Tracks, Export
  Region, Bounce in Place; 16-bit dithered, 24-bit or 32-bit float WAV.
- **Projects:** `.marbble` folders with readable `project.json`; Project Chooser with templates;
  autosave and crash recovery; undo for every edit (100 steps by default).
- **Interface:** Logic Pro 11's layout; five themes; interface scaling from 90 to 200 %; four
  waveform styles; rebindable key commands, shown the way each platform writes them; Musical
  Typing.
- **Real-time safety:** wait-free `rtrb` queues between the session and the audio thread; a
  panic in the engine silences one block instead of crashing; every plug-in's output is
  checked, NaNs are silenced, runaway levels are clamped, and a message names the plug-in.
- **Denormals:** the audio thread runs with flush-to-zero (and denormals-are-zero on x86_64),
  so decaying signals no longer slow DSP down about a hundredfold.
- **Overload protection:** when callbacks run late back to back for 60 ms, the engine outputs
  silence for 250 ms without processing, stops playback or recording, and shows "System
  overload…". Before, the kernel killed the app (SIGXCPU, `RLIMIT_RTTIME`). Offline renders are
  exempt.
- **Tuner:** gates on its DC-blocked level, so an interface's DC offset doesn't run the
  detector.
- **About Marbble** in the Marbble menu, with the version.
- **Platforms and packaging:** Linux x86_64 and aarch64 (AppImage and .deb; .rpm from the first
  stable release, since RPM versions can't carry a prerelease suffix) and a Nix
  flake, Windows (NSIS installer, x64) and macOS (.dmg for Apple Silicon and Intel), built and
  published by GitHub Actions; CI on all three; `scripts/set-version.sh` sets the version
  everywhere.

### Known issues

- VST3 editor windows open only on Linux (X11, through XWayland on Wayland). On Windows and
  macOS plug-ins load, play and can be automated, but show no editor.
- Windows and macOS builds are early and less tested. Only the system's default audio driver is
  available there (WASAPI, Core Audio; no ASIO).
- The builds are unsigned: macOS needs right-click › Open (or `xattr -cr`), Windows
  SmartScreen needs More info › Run anyway.
- Automation lanes of a plug-in dragged to another track stay on the old track and no longer
  drive it.
- No crossfades, fade curve shapes, drag modes (X-Fade, Shuffle) or Marquee tool.
- Regions stay at fixed sample positions when the tempo changes; only MIDI can follow (Move MIDI
  Regions with Tempo).
- A built-in effect's side-chain key is always post-fader; VST3 side-chain buses are not
  connected.
- AAC/M4A imports can start about 40 ms late (encoder delay not fully trimmed).
- The `takes_check` headless check "autopunch trims the take" fails about one run in five,
  from the null audio device's timing.

[Unreleased]: https://github.com/JackMechem/marbble/compare/v0.1.0-alpha.6...HEAD
[0.1.0-alpha.6]: https://github.com/JackMechem/marbble/compare/v0.1.0-alpha.5...v0.1.0-alpha.6
[0.1.0-alpha.5]: https://github.com/JackMechem/marbble/compare/v0.1.0-alpha.4...v0.1.0-alpha.5
[0.1.0-alpha.4]: https://github.com/JackMechem/marbble/compare/v0.1.0-alpha.3...v0.1.0-alpha.4
[0.1.0-alpha.3]: https://github.com/JackMechem/marbble/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/JackMechem/marbble/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/JackMechem/marbble/releases/tag/v0.1.0-alpha.1
