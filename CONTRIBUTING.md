# Contributing to Marbble

Thanks for your interest. Marbble is alpha software with a lot still open, and bug reports,
testing on real hardware and code are all welcome.

## Reporting a bug or asking for a feature

Open an [issue](https://github.com/JackMechem/marbble/issues/new/choose). For a bug, say which
version and platform you're on, your audio setup, and the steps that reproduce it. The User
Guide's [known limitations](https://marbble.dev/guide/known-limitations/) lists what's already
known not to work.

Testing is some of the most useful help right now. Many features pass their automated checks
but haven't been heard through a real interface or seen on Windows and macOS. Those issues
carry the [`needs listening`](https://github.com/JackMechem/marbble/labels/needs%20listening)
and [`needs a look`](https://github.com/JackMechem/marbble/labels/needs%20a%20look) labels. A
comment saying what you tried and what you heard or saw is a contribution in itself.

## How the work is tracked

- **Issues** hold everything that's open: bugs, features, and checks that still need a person.
- **Milestones** are the roadmap's stages, in the order they matter for a usable release.
  [TODO.md](TODO.md) describes each stage's goal.
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

Pull requests are merged into `main` once CI passes and the change has been reviewed.

## Licence

Marbble is under the [MIT licence](LICENSE). By contributing, you agree that your contribution
is released under it too.
