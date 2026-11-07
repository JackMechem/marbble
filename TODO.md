# Marbble roadmap

The stages below are the order things matter in for a usable release; each builds on the one before.
Each stage is a [milestone][milestones] on GitHub, and every open item is an [issue][issues], so
this file only gives the shape of the plan. The full Logic Pro feature scope is in
[LOGIC_FEATURES.md](LOGIC_FEATURES.md); known limitations of what exists are in the [User
Guide](docs/USER_GUIDE.md#20-known-limitations). [CONTRIBUTING.md](CONTRIBUTING.md) says how issues,
labels and milestones are used.

## Open work

Besides each stage's milestone, a few labels gather the open work that doesn't belong to one stage:

- [`bug`][bug]: known bugs and unexplained failures.
- [`needs a look`][needs a look]: built and tested, but not yet checked by eye in the running app.
- [`needs listening`][needs listening]: passes its checks, but nobody has listened to it or tried it
  on a real interface yet.
- [`platform: windows`][platform: windows] and [`platform: macos`][platform: macos]: what still
  needs a real Windows or Mac.
- [`flaky test`][flaky test]: checks that fail now and then.
- `area: …` labels group the rest by part of Marbble ([instruments][area: instruments],
  [sampler][area: sampler], [multi-core][area: multi-core], [ui][area: ui] and so on).

## Stage 1: Release hygiene

Goal: prereleases that install cleanly and that people can trust with their projects.

- [ ] Code signing on Windows (Authenticode) and signing plus notarization on macOS ([#97], [#98])
- [x] A LICENSE file (MIT, shipped in every bundle)
- [ ] Hands-on testing of the Windows and macOS builds: devices, MIDI, plug-in scanning,
      installers, file dialogs ([#32], [#33], [#34])
- [x] Automation lanes follow a plug-in moved to another track
- [x] Make `takes_check` "autopunch trims the take" deterministic (it was a real race: a
      position read right after a locate could return the old playhead)
- [x] Run the headless checks that need no plug-ins in CI on Linux (`scripts/headless-checks.sh`)

## Stage 2: Plug-ins on every platform

Goal: a VST3 user on Windows or macOS gets the same workflow as on Linux.

- [ ] VST3 editor windows on Windows (HWND embedding) ([#99])
- [x] VST3 editor windows on macOS (NSView embedding)
- [x] VST3 editors with the interface on native Wayland (opt-in, `MARBBLE_WAYLAND=1`; the
      editors stay XWayland windows)
- [x] HiDPI scaling for plug-in editors (`IPlugViewContentScaleSupport`, Linux)
- [ ] ASIO on Windows ([#100])
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
- [ ] MP3 and AAC bounce (needs an encoder whose licence suits) ([#101])
- [x] Delay compensation changes while playing without clicks
- [x] More built-in effects: modulation (chorus, flanger, phaser, tremolo), distortion, a Gain
      utility
- [x] A linear-phase EQ

## Stage 6: Advanced production

Goal: the larger Logic features that set a full DAW apart.

- [ ] Flex Time ([#102])
- [ ] Flex Pitch ([#103])
- [x] Track alternatives; project alternatives and backups; user project templates
- [x] Arrangement track; markers with their own lengths
- [x] Step Sequencer: pattern regions, rows of steps, edit modes, polymeter, swing
- [x] Multi-core track processing: Settings › Audio › Processing Threads, bit-identical to one thread
- [ ] Score Editor ([#104])

## Stage 7: Formats and reach

Goal: more plug-ins, more people.

- [ ] CLAP hosting ([#105])
- [ ] Audio Units on macOS ([#106])
- [ ] Screen reader support ([#107])
- [ ] Translated interface ([#108])

[milestones]: https://github.com/JackMechem/marbble/milestones
[issues]: https://github.com/JackMechem/marbble/issues
[bug]: https://github.com/JackMechem/marbble/labels/bug
[needs a look]: https://github.com/JackMechem/marbble/labels/needs%20a%20look
[needs listening]: https://github.com/JackMechem/marbble/labels/needs%20listening
[platform: windows]: https://github.com/JackMechem/marbble/labels/platform%3A%20windows
[platform: macos]: https://github.com/JackMechem/marbble/labels/platform%3A%20macos
[flaky test]: https://github.com/JackMechem/marbble/labels/flaky%20test
[area: instruments]: https://github.com/JackMechem/marbble/labels/area%3A%20instruments
[area: sampler]: https://github.com/JackMechem/marbble/labels/area%3A%20sampler
[area: multi-core]: https://github.com/JackMechem/marbble/labels/area%3A%20multi-core
[area: ui]: https://github.com/JackMechem/marbble/labels/area%3A%20ui
[#32]: https://github.com/JackMechem/marbble/issues/32
[#33]: https://github.com/JackMechem/marbble/issues/33
[#34]: https://github.com/JackMechem/marbble/issues/34
[#97]: https://github.com/JackMechem/marbble/issues/97
[#98]: https://github.com/JackMechem/marbble/issues/98
[#99]: https://github.com/JackMechem/marbble/issues/99
[#100]: https://github.com/JackMechem/marbble/issues/100
[#101]: https://github.com/JackMechem/marbble/issues/101
[#102]: https://github.com/JackMechem/marbble/issues/102
[#103]: https://github.com/JackMechem/marbble/issues/103
[#104]: https://github.com/JackMechem/marbble/issues/104
[#105]: https://github.com/JackMechem/marbble/issues/105
[#106]: https://github.com/JackMechem/marbble/issues/106
[#107]: https://github.com/JackMechem/marbble/issues/107
[#108]: https://github.com/JackMechem/marbble/issues/108
