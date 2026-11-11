# Notes for coding agents

Marbble is a Logic Pro–style DAW in Rust, Tauri 2 and React. If you're an AI coding agent working
on it for a contributor, these are the project's rules; they're the same as for people.

## Start here

- `docs/ARCHITECTURE.md` explains how the code fits together. Read its real-time rules before
  touching anything the audio thread runs.
- `docs/DEVELOPING.md` covers building, the tests and headless checks, the code conventions and
  the steps for common changes, including what to run before sending a change.
- `CONTRIBUTING.md` has the workflow and the policy on AI-assisted contributions.
- `ROADMAP.md` says what beta needs and what's being worked on; `features/*.toml` how far each
  Logic Pro feature is.

## Rules

- Work from an issue. A branch is named `fix/<n>-name` or `feat/<n>-name`, from `main`; a pull
  request covers one issue and says `Fixes #<n>`.
- A change that builds or extends a Logic Pro feature updates its entry in `features/` and runs
  `python3 scripts/features.py`; CI fails if `docs/FEATURES.md` and `ROADMAP.md` are stale.
- Anything a user would notice gets a line in `CHANGELOG.md` under `[Unreleased]`, and the docs
  that describe it are updated in the same change.
- Styles live in `src/styles/`, one file per area; `src/styles/index.css` imports them and its
  order is the cascade. When merging two branches that add `@import` lines or append to the same
  area file, check the merged result by hand and run `npm run build`.
- Parallel work goes in separate git worktrees, each with its own `CARGO_TARGET_DIR`. Don't
  switch branches in a checkout while `npm run tauri dev` runs from it: overlapping rebuilds
  corrupt the incremental cache.
- Never change the user's global git configuration.
- Say in the pull request what you tested, how, and what you couldn't (a platform, a real audio
  interface, listening).
