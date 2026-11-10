# Marbble

A digital audio workstation modelled on Logic Pro's workflow and layout. The audio engine and
VST3 host are written in Rust; the interface is Tauri 2 with React and TypeScript.

Marbble is in alpha. It is developed and tested on Linux; the Windows and macOS builds pass their
tests (macOS its headless checks too, on a stand-in device) but haven't been run on a real
machine yet (see [Platform support](#platform-support)).

**Website, User Guide and developer docs:** <https://marbble.dev/>

## Features

- **Recording:** audio from any input (mono or stereo per track) with live waveforms, latency
  compensation and a manual recording delay; MIDI with controllers, pitch bend and aftertouch;
  count-in and pre-roll; cycle recording into take folders; Autopunch; Logic's Replace mode and
  a choice of what happens to overlapped audio and MIDI; Step Input; crash-safe takes.
- **Take folders and comping**, after Logic's Quick Swipe Comping: swipe across take rows to
  build a comp, keep several comps per folder, crossfaded boundaries; MIDI take folders;
  Flatten, Flatten and Merge, Pack and Unpack.
- **Tracks area:** move, trim, split, join, loop, fade, gain, reverse, colour, copy, paste,
  repeat and nudge regions with Smart snap, musically across tempo changes; Logic's tools and
  Tool menu; the Overlap, No Overlap, X-Fade and Shuffle drag modes; fade and crossfade shapes;
  region playback parameters (Quantize, Transpose, Velocity, Dynamics, Gate Time, Delay);
  Logic's arranging commands (Select, Section Between Locators, Trim, Move, Strip Silence, Name
  and Color).
- **Tracks and arrangement:** track stacks (folder and summing), groups, hidden tracks, Track
  Freeze, Track On/Off, track alternatives; markers with their own lengths, an Arrangement track whose
  sections move and copy with their content, and an overview of the whole project.
- **Editors:** a Piano Roll with eight tools, Time Quantize with strength and swing, scale
  quantize, humanize, note-length functions and a velocity / controller lane with curved
  ramps; Logic's Event List; a Step Sequencer for drum patterns and riffs (pattern regions);
  an Audio Track Editor with range edits, normalizing, fades to selection and Split at
  Transients; Standard MIDI File import and export.
- **Mixing:** channel strips with an EQ thumbnail that opens (or inserts) an eight-band Channel
  EQ with an analyzer, eight Audio FX slots, eight pre- or post-fader sends, aux buses, output
  routing and VCA faders; clip indicators and gain-reduction meters; channel strip settings to
  copy, paste and save; plug-in delay compensation on every path, with a Low Latency Mode.
- **Automation:** volume, pan, mute, send levels and plug-in parameters, with Logic's Off,
  Read, Touch, Latch and Write modes, several lanes per track, curved segments, folder stack
  and Stereo Out (Output track) automation, and automation that can move with its regions.
- **MIDI Learn:** assign a controller's knobs, faders, buttons and pads to mixer controls, plug-in
  parameters, the selected track, the transport and any key command, with feedback to its lights
  and motor faders (Learn Assignment, Controller Assignments window); Mackie Control and HUI
  control surfaces.
- **Plug-ins:** VST3 instruments and effects, scanned out of process, with side-chain inputs,
  multi-output instruments, and undo for a knob moved in a plug-in's own window. Copy, paste and
  drag plug-ins (VST3 state included) between slots, strips and projects.
- **Built-in synths:** six models with panels, factory presets, automation and MIDI learn.
  Vintage Synths, modelled on the documented behaviour of real instruments (no affiliation, none
  compared with the hardware): **Marbble Tre** (after the Minimoog Model D), **Sei** (Juno-106),
  **Cinque** (Prophet-5), **Uno** (TB-303) and **Sedici** (six-operator FM, after the DX7).
  Modern Synths: **Onda**, two wavetable oscillators with eight modulation slots. The simple
  Marbble Synth is the default on new instrument tracks.
- **Marbble Sampler:** instruments from your own sounds: drop files on it (files named by note
  map themselves), record straight in, or convert an audio region, whole or sliced at its
  transients. Zones across keys and velocities, loops, round robin, envelopes, a filter, an LFO
  and glide; a subset of SFZ loads. Samples are copied into the project and held in memory, up
  to 4 GB.
- **Multi-core processing:** independent tracks play at the same time on a small pool of real-time
  threads, sample for sample the same as on one (Settings › Audio › Processing Threads; Automatic
  stays on one thread on Windows and macOS for now). Track Off frees what a muted track keeps using.
- **Built-in effects:** Compressor (five modelled circuits), Limiter, Noise Gate, De-Esser,
  Reverb, Delay, Chorus, Flanger, Phaser, Tremolo, Overdrive, Distortion, Channel EQ, Linear
  Phase EQ, Gain, Tuner and Loudness Meter, in any Audio FX slot, with Logic-style panels,
  factory and user presets, automation and side-chain input from any track.
- **Bounce and export:** offline bounce of the project or a section, Export All or Selected
  Tracks, Export Region, Bounce in Place, as WAV, AIFF or FLAC at 16-bit (dithered), 24-bit or
  32-bit float.
- **Tempo and signature:** tempo changes and smooth ramps, time signature changes, in Tempo and
  Signature global tracks; the grid, click, quantize, plug-ins and MIDI files all follow them.
- **Projects:** `.marbble` folders, a Project Chooser with built-in and your own templates,
  project alternatives, backups with Revert to, autosave and crash recovery (interrupted
  recordings included), and undo for every edit (100 steps by default).
- **Interface:** Logic Pro 11's layout, the Mixer, editors and effect panels in separate
  windows, five themes, interface scaling from 90 to 200 %, four waveform styles, rebindable key
  commands, Musical Typing, and a click in the tracks area that moves the playhead
  (configurable).
- **Robustness:** every plug-in's output is checked, so NaNs and runaway levels are silenced
  and the plug-in is named in a message; an internal engine error skips one block instead of
  crashing; the audio thread never waits on a lock; saves are atomic and a damaged project
  falls back to its autosave or a backup.

The [User Guide](docs/USER_GUIDE.md) describes all of it in detail.
[LOGIC_FEATURES.md](LOGIC_FEATURES.md) lists every Logic Pro feature by stage and marks what
Marbble already has.

## Install

Prebuilt binaries are on the [GitHub Releases page](https://github.com/JackMechem/marbble/releases),
in two channels:

- **Stable:** releases with a plain version, `v1.2.3`. GitHub marks the newest as Latest.
- **Unstable:** every release, prereleases included (`v1.2.3-alpha.1`, `-beta.1`, `-rc.1`): new
  features sooner, with less testing. Every release so far is a prerelease, so for now only
  Unstable has any.

Each release has:

- **Linux** (x86_64 and aarch64): `.AppImage` (any distribution), `.deb` (Debian, Ubuntu) and
  `.rpm` (Fedora, openSUSE; stable releases only, since RPM can't version a prerelease).
  They're built on Ubuntu 24.04, so they need glibc 2.39 and
  PipeWire 1.0 or newer (Ubuntu 24.04, Debian 13, Fedora 40 and later).
- **Windows** (x64): the NSIS installer, `_x64-setup.exe`.
- **macOS:** `_aarch64.dmg` for Apple Silicon, `_x64.dmg` for Intel.

The builds are not signed with a developer certificate yet, so the system warns about them:

- **macOS:** the first time, right-click the app and choose Open. On macOS 15 or later, open it
  once, then choose **Open Anyway** in System Settings › Privacy & Security. Or run
  `xattr -cr /Applications/Marbble.app`.
- **Windows:** in the SmartScreen prompt, choose **More info › Run anyway**.

On NixOS or with Nix, use the flake instead (below).

## Platform support

| | Linux | Windows | macOS |
|---|---|---|---|
| Status | Main platform, tested | Early, not yet run on a real machine | Early, not yet run on a real machine |
| Packages | AppImage, .deb, .rpm from stable releases (x86_64, aarch64); Nix flake | NSIS installer (x64) | .dmg (Apple Silicon, Intel) |
| Audio drivers | PipeWire, PulseAudio, JACK, ALSA | WASAPI | Core Audio |
| MIDI input | Yes | Yes | Yes |
| VST3 instruments and effects | Yes | Yes | Yes |
| VST3 editor windows | Yes (X11; through XWayland on Wayland) | Not yet | Yes |
| Built-in effects and instruments | Yes | Yes | Yes |
| Multi-core processing | Yes | One thread for now | One thread for now |
| Input level check | Yes (PipeWire, PulseAudio) | No | No |

On Windows, VST3 plug-ins still load, play, save their state and can be automated; only their
own editor window is missing. The Windows and macOS columns say what's built: both build and
pass the engine tests in CI, and macOS the headless checks on a silent stand-in device, but none
of it has been tried on real hardware yet (the [Windows](https://github.com/JackMechem/marbble/labels/platform%3A%20windows) and
[macOS](https://github.com/JackMechem/marbble/labels/platform%3A%20macos) issues list what to try).

## Build from source

On NixOS or any system with Nix flakes:

```sh
git clone https://github.com/JackMechem/marbble && cd marbble
nix run .                                 # build and launch
```

To follow a channel from a NixOS or home-manager flake, pin the input to its branch, and
`nix flake update marbble` moves to the channel's newest release:

```nix
inputs.marbble.url = "github:JackMechem/marbble/unstable";   # or /stable
# then: inputs.marbble.packages.${pkgs.stdenv.hostPlatform.system}.default
```

To develop:

```sh
nix develop                               # Rust, Node, WebKitGTK, ALSA/JACK/PipeWire
pnpm install                              # or npm install
pnpm tauri dev                            # or npm run tauri dev
```

On other Linux distributions, install Rust, Node.js, the Tauri 2 prerequisites
(`webkit2gtk-4.1`, `gtk3`, `libsoup-3`) and `alsa-lib`, `libpipewire-0.3`, `jack`, `dbus` and
`libclang`, then run `pnpm install && pnpm tauri dev`. See [DEVELOPING.md](docs/DEVELOPING.md)
for Windows and macOS, tests and packaging.

## First steps

1. Create a project from the Project Chooser (a template such as Record Audio or Software
   Instrument).
2. Open Settings › Audio (Ctrl+, or ⌘, on macOS), choose a driver (PipeWire is recommended on
   Linux) and your devices, and click Apply Changes.
3. Record-enable a track with its R button and press R to record, Space to stop.

## Documentation

| Document | For |
|---|---|
| [Website](https://marbble.dev/) | The three documents below and the changelog as a searchable site, built from them by [site/](site/) (`cd site && npm ci && npm run build`). |
| [docs/USER_GUIDE.md](docs/USER_GUIDE.md) | Using Marbble: recording, instruments, editing, mixing, exporting, every setting and key command, troubleshooting. |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | How the code is organised and works: the source tree, threads and real-time rules, the life of an edit and of a block of audio, the project model and its files, plug-ins, MIDI, and the frontend. |
| [docs/DEVELOPING.md](docs/DEVELOPING.md) | Setting up, building, testing, debugging, the code conventions, common changes, CI and releases. |
| [docs/logic-reference.md](docs/logic-reference.md) | The Logic Pro research behind the layout and behaviour. |
| [LOGIC_FEATURES.md](LOGIC_FEATURES.md) | The feature scope: every Logic Pro feature by stage, with Marbble's status. |
| [TODO.md](TODO.md) | The roadmap in stages; what's open is in the [issues](https://github.com/JackMechem/marbble/issues). |
| [CHANGELOG.md](CHANGELOG.md) | What changed in each release. |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Reporting bugs, testing, how issues and milestones are used, and sending a pull request. |

## Status and known limitations

Marbble is alpha software under active development; keep backups of projects that matter. The
main limitations:

- Plug-in editor windows open on Linux (X11) and macOS, not yet on Windows. On Linux the
  interface runs through XWayland by default; running it natively on Wayland
  (`MARBBLE_WAYLAND=1`) is newer and less tried.
- VST3 only (no CLAP, AU or VST2). On Windows and macOS only the system's default audio driver
  is available (no ASIO).
- The release builds are unsigned (see [Install](#install)), and the Windows and macOS builds
  haven't been run on real hardware.
- The synth models haven't been tuned by ear or compared with the instruments they follow.
- Regions stay at fixed times when the tempo changes (Move MIDI Regions with Tempo keeps MIDI
  on its bars).
- No Score editor, Flex Time or Flex Pitch; MP3 and AAC aren't offered as bounce formats.

The User Guide's [Known limitations](docs/USER_GUIDE.md#20-known-limitations) has the full list,
the [issues](https://github.com/JackMechem/marbble/issues) what's open, [TODO.md](TODO.md) the staged roadmap, and
[CHANGELOG.md](CHANGELOG.md) what each release contains.

## Versions

Marbble uses [semantic versioning](https://semver.org). Prereleases are tagged
`vX.Y.Z-alpha.N` (the first is `v0.1.0-alpha.1`) and published as prereleases on GitHub; see
[DEVELOPING.md](docs/DEVELOPING.md#versioning-and-releases) for how a release is made.

## Licence

Marbble is released under the MIT licence; see [LICENSE](LICENSE). The release bundles include
a copy.
