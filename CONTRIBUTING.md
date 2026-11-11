# Contributing to Marbble

Thanks for your interest. Marbble is alpha software with a lot still open, and bug reports,
testing on real hardware and code are all welcome. Everyone taking part follows the
[code of conduct](CODE_OF_CONDUCT.md).

## Questions and ideas

Ask in [Discussions](https://github.com/JackMechem/marbble/discussions): Q&A for how something
works or how to build it, Ideas for something you'd like Marbble to do before it's a concrete
feature request.

## Reporting a bug or asking for a feature

Open an [issue](https://github.com/JackMechem/marbble/issues/new/choose). For a bug, say which
version and platform you're on, your audio setup, and the steps that reproduce it. The User
Guide's [known limitations](https://marbble.dev/guide/known-limitations/) lists what's already
known not to work. A security problem goes through [SECURITY.md](SECURITY.md) instead, not a
public issue.

## Testing

Testing is some of the most useful help right now. Many features pass their automated checks
but haven't been heard through a real interface or tried on Windows and macOS:

- [#32](https://github.com/JackMechem/marbble/issues/32) lists what to try by hand on Windows
  and macOS: devices, playback and recording, MIDI, plug-in scanning, the file dialogs, windows.
- [#16](https://github.com/JackMechem/marbble/issues/16) asks for listening for dropouts and
  clicks with several processing threads, on a real interface.

A comment on the issue saying what you tried, on what, and what you heard or saw is a
contribution in itself. For "it works (or doesn't) on my setup" in general, start a
[discussion](https://github.com/JackMechem/marbble/discussions) with your system, interface and
driver, or open a bug if something's wrong.

## Your first 30 minutes

1. `git clone https://github.com/JackMechem/marbble && cd marbble`, and install what it needs
   to build: [DEVELOPING.md › Set up](docs/DEVELOPING.md#set-up) has the packages for Debian,
   Fedora, Arch, Windows and macOS, or `nix develop` with Nix.
2. `npm ci`, then `cargo test -p daw-engine --lib`: the engine's unit tests, which need no audio
   device.
3. `npm run tauri dev` builds and opens the app. The first build compiles over 500 crates and
   takes a while; `target/` grows to several GB, and to tens of GB once the tests and headless
   checks are built too.
4. Open Settings › Audio, choose a driver and your devices, and click Apply Changes.
5. For a project to try things on, write the demo the website's screenshots come from:
   `cargo run -p daw-engine --example showcase_project -- ~/Showcase.marbble`, then open it with
   File › Open (or start with `npx tauri dev -- -- ~/Showcase.marbble`). It uses only built-in
   instruments and effects.

From there, [ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the code fits together, and a
[`good first issue`](https://github.com/JackMechem/marbble/labels/good%20first%20issue) is a
small place to start.

## How the work is tracked

- **Issues** hold everything that's open: bugs, features, and checks that still need a person.
- **Milestones** are the roadmap's stages, in the order they matter for a usable release, and
  [Beta: core features](https://github.com/JackMechem/marbble/milestone/8) gathers the features
  beta still needs. [ROADMAP.md](ROADMAP.md) describes each stage's goal, what beta needs and the
  current focus.
- **The [feature list](docs/FEATURES.md)** has every Logic Pro feature by area, and how far
  Marbble has it. It's generated from [`features/`](features/README.md), one TOML file per area:
  a pull request that builds or extends a feature changes its entry there and runs
  `python3 scripts/features.py`. A feature someone takes on gets an issue, named in its entry.
- **Labels** say what an issue is (`bug`, `enhancement`, `documentation`), which part of
  Marbble it touches (`area: engine`, `area: ui`, `area: plug-ins` and so on), and which
  platform, if only one (`platform: macos`). `flaky test` marks checks that fail now and then.
  [`good first issue`](https://github.com/JackMechem/marbble/labels/good%20first%20issue) marks
  small, well-scoped ones.
- An issue labelled `in progress` is being worked on; its pull request is linked from it.

Before starting on something sizeable, comment on its issue (or open one), so two people don't
do the same work.

## Building and testing

[docs/DEVELOPING.md](docs/DEVELOPING.md) covers building on each platform, the tests and
headless checks, debugging, the code conventions, and the steps for common changes.
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the code fits together. Read its
real-time rules before touching the engine.

## Pull requests

- Branch from `main`, named for the issue: `fix/123-short-name` for a bug,
  `feat/123-short-name` for a feature.
- Keep a pull request to one issue, and say `Fixes #123` in its description.
- Run what [DEVELOPING.md › What to run before sending a change](docs/DEVELOPING.md#what-to-run-before-sending-a-change)
  lists for the parts you changed. CI builds and runs the tests on Linux, Windows and macOS,
  runs rustfmt and clippy on Linux and the headless checks on Linux and macOS, and builds the
  Nix flake.
- Add a line to `CHANGELOG.md` under `[Unreleased]` for anything a user would notice, and
  update the docs that describe the behaviour you changed.
- Say in the description what you tested and how, and what you couldn't try (a platform, a
  real interface, listening).
- Say whether you used AI assistance; see [below](#ai-assisted-contributions).

Marbble has one maintainer, Jack Mechem (`JackMechem` on GitHub), who reviews every pull request
and aims to respond within a few days. A pull request is merged into `main` once CI passes and
the review is done.

## AI-assisted contributions

AI-assisted contributions are welcome, on the same terms as any other:

- Say in the pull request that you used AI assistance, and for what.
- You have read and understood every line you send, and you have run it and tested it as the
  description says. You're the author and answer for it in review.
- No pull requests opened by an agent working unattended, and no bulk changes (reformatting,
  renaming or "fixing" across the codebase) that nobody asked for.
- The review bar is the same as for any code: a change that doesn't follow the conventions or
  isn't tested goes back, however it was written.

## Licence

Marbble is under the [MIT licence](LICENSE). By contributing, you agree that your contribution
is released under it too.
