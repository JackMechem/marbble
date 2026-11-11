# Marbble

A Logic Pro–style DAW in Rust, Tauri 2 and React. Start with `docs/ARCHITECTURE.md` and
`docs/DEVELOPING.md`. `ROADMAP.md` says what beta needs and what's being worked on, `features/*.toml` how far each
Logic Pro feature is (it generates `docs/FEATURES.md`), and what's open is in the GitHub issues, by milestone and label (`needs a look` is what still needs checking in the running app).

## How the user likes to work

- Run independent features as parallel agents, each in its own git worktree with its own
  `CARGO_TARGET_DIR`. Never switch branches in the main checkout while `pnpm tauri dev` runs
  from it: overlapping rebuilds corrupt the incremental cache.
- Agents never launch the GUI; the coordinator does the visual checks.
- Every piece of work goes through GitHub, as in an open-source project (see `CONTRIBUTING.md`):
  an issue first (labelled `in progress` and assigned while it's worked on), a branch named
  `fix/<n>-name` or `feat/<n>-name` from `main`, and a PR into `main` that says `Fixes #<n>`.
  Bugs found along the way get issues of their own. The open backlog is the issues. A PR that builds or extends a
  feature updates its entry in `features/` and runs `python3 scripts/features.py`; update
  `ROADMAP.md`'s current focus when a round of work starts or ends.
- Merge a PR once CI is green, then pull `main` into the repo the user runs `pnpm tauri dev`
  from, and say when the app restarts. Report progress as each piece lands.
- One commit per issue with a descriptive body. Sign and push with the key the user set up for
  this (see the memory notes); never change the global git config.
- Styles live in `src/styles/`, one file per area; `src/styles/index.css` imports them and its
  order is the cascade. When two branches add `@import` lines to the end of it or append to the
  same area file, check the merged tail by hand and run the vite build before trusting the merge.
