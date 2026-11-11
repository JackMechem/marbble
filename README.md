# Marbble

A digital audio workstation modelled on Logic Pro's workflow and layout, with its audio engine and
VST3 host in Rust and its interface in Tauri 2, React and TypeScript.

**Website, User Guide and developer docs:** <https://marbble.dev/>

## Features

- Audio and MIDI recording, cycle recording into take folders, swipe comping and Autopunch.
- Logic's tools, drag modes, fades and crossfades in the tracks area; track stacks, groups,
  Freeze, markers and an Arrangement track.
- A Piano Roll, Event List, Step Sequencer and Audio Track Editor.
- A Mixer with eight insert and eight send slots per strip, aux buses, VCA faders, plug-in delay
  compensation, and automation in Logic's Read, Touch, Latch and Write modes.
- VST3 instruments and effects, with side-chains and multi-output instruments.
- Six built-in synth models, a sampler and seventeen built-in effects.
- MIDI Learn, and Mackie Control and HUI surfaces (so far tested only with simulated ones).
- Offline bounce and export as WAV, AIFF or FLAC; templates, backups, autosave and crash recovery.

The [feature list](docs/FEATURES.md) has every Logic Pro feature by area and how far Marbble has
it, and the [User Guide](docs/USER_GUIDE.md) describes how each one works.

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
| Status | Main platform, tested | Early, not yet run on a real machine | Early, run on a real Mac but less tested |
| Packages | AppImage, .deb, .rpm from stable releases (x86_64, aarch64); Nix flake | NSIS installer (x64) | .dmg (Apple Silicon, Intel) |
| Audio drivers | PipeWire, PulseAudio, JACK, ALSA | WASAPI | Core Audio |
| MIDI input | Yes | Yes | Yes |
| VST3 instruments and effects | Yes | Yes | Yes |
| VST3 editor windows | Yes (X11; through XWayland on Wayland) | Not yet | Yes |
| Built-in effects and instruments | Yes | Yes | Yes |
| Multi-core processing | Yes | One thread for now | One thread for now |
| Input level check | Yes (PipeWire, PulseAudio) | No | No |

On Windows, VST3 plug-ins still load, play, save their state and can be automated; only their
own editor window is missing. Both build and pass the engine tests in CI, and macOS the headless
checks on a silent stand-in device. The macOS build has been run on a real Mac, where it works but
uses one processing thread ([#59](https://github.com/JackMechem/marbble/issues/59)); the Windows
build hasn't been tried on real hardware yet. The
[Windows](https://github.com/JackMechem/marbble/labels/platform%3A%20windows) and
[macOS](https://github.com/JackMechem/marbble/labels/platform%3A%20macos) issues list what to try.

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
npm ci
npm run tauri dev
```

Without Nix, [DEVELOPING.md](docs/DEVELOPING.md#set-up) lists what to install on Debian, Fedora,
Arch, Windows and macOS, then the same `npm ci` and `npm run tauri dev`. It also covers tests and
packaging.

## First steps

1. Create a project from the Project Chooser (a template such as Record Audio or Software
   Instrument).
2. Open Settings › Audio (Ctrl+, or ⌘, on macOS), choose a driver (PipeWire is recommended on
   Linux) and your devices, and click Apply Changes.
3. Record-enable a track with its R button and press R to record, Space to stop.

## Documentation

| Document | For |
|---|---|
| [Website](https://marbble.dev/) | The User Guide, Architecture, Developing, the roadmap, the feature list and the changelog as a searchable site, built from them by [site/](site/) (`cd site && npm ci && npm run build`). |
| [docs/USER_GUIDE.md](docs/USER_GUIDE.md) | Using Marbble: recording, instruments, editing, mixing, exporting, every setting and key command, troubleshooting. |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | How the code is organised and works: the source tree, threads and real-time rules, the life of an edit and of a block of audio, the project model and its files, plug-ins, MIDI, and the frontend. |
| [docs/DEVELOPING.md](docs/DEVELOPING.md) | Setting up, building, testing, debugging, the code conventions, common changes, CI and releases. |
| [docs/logic-reference.md](docs/logic-reference.md) | The Logic Pro research behind the layout and behaviour. |
| [ROADMAP.md](ROADMAP.md) | What beta needs, how far along it is, the current focus and the roadmap's stages, each a milestone. |
| [docs/FEATURES.md](docs/FEATURES.md) | Every Logic Pro feature by area, done, partial or missing in Marbble (generated from [`features/`](features/README.md)). |
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
- The release builds are unsigned (see [Install](#install)). The Windows build hasn't been run
  on real hardware, and the macOS build has had little hands-on testing.
- The synth models haven't been tuned by ear or compared with the instruments they follow.
- Regions stay at fixed times when the tempo changes (Move MIDI Regions with Tempo keeps MIDI
  on its bars).
- No Score editor, Flex Time or Flex Pitch; MP3 and AAC aren't offered as bounce formats.

The User Guide's [Known limitations](docs/USER_GUIDE.md#20-known-limitations) has the full list,
the [issues](https://github.com/JackMechem/marbble/issues) what's open, [ROADMAP.md](ROADMAP.md) the plan to beta, and
[CHANGELOG.md](CHANGELOG.md) what each release contains.

## Versions

Marbble uses [semantic versioning](https://semver.org). Prereleases are tagged
`vX.Y.Z-alpha.N` (the first is `v0.1.0-alpha.1`) and published as prereleases on GitHub; see
[DEVELOPING.md](docs/DEVELOPING.md#versioning-and-releases) for how a release is made.

## Licence

Marbble is released under the MIT licence; see [LICENSE](LICENSE). The release bundles include
a copy.
