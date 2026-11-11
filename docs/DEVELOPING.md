# Developing Marbble

How to set up, build, run and test Marbble, the conventions the code follows, and how a release is
made. Read [ARCHITECTURE.md](ARCHITECTURE.md) first for how the pieces fit together. The repository
is <https://github.com/JackMechem/marbble>; the main branch is `main`, and
[CONTRIBUTING.md](../CONTRIBUTING.md) has how issues and pull requests are used.

Contents:

- [Set up](#set-up)
- [Build and run](#build-and-run)
- [Tests](#tests)
- [Debugging](#debugging)
- [Conventions](#conventions)
- [Common changes](#common-changes)
- [CI](#ci)
- [Packaging and releases](#packaging-and-releases)

## Set up

### With Nix

The flake has the package, an app, a dev shell, an overlay and a check, for `x86_64-linux` and
`aarch64-linux`. The shell (`nix/shell.nix`) takes its inputs from the package, so `nix develop`
and `nix build` use the same libraries, and adds cargo, clippy, rustfmt, rust-analyzer and Node.

```sh
nix develop               # or nix-shell
npm ci
```

Real-time audio scheduling needs rtkit (`security.rtkit.enable = true;` on NixOS). Flakes only see
files git tracks, so `git add` new files before `nix build`.

### Without Nix (Linux)

Install Rust (stable, through [rustup](https://rustup.rs)) and Node.js 22.18 or newer, then the
libraries: WebKitGTK 4.1 and GTK 3 for Tauri, ALSA, JACK, PipeWire and D-Bus for audio and MIDI,
and libclang, which bindgen uses for the PipeWire bindings.

```sh
# Debian 13, Ubuntu 24.04 or newer
sudo apt install build-essential pkg-config libclang-dev libwebkit2gtk-4.1-dev librsvg2-dev \
  libssl-dev libasound2-dev libjack-jackd2-dev libpipewire-0.3-dev libdbus-1-dev
# Fedora
sudo dnf install gcc gcc-c++ make pkgconf-pkg-config clang-devel webkit2gtk4.1-devel \
  librsvg2-devel openssl-devel alsa-lib-devel pipewire-jack-audio-connection-kit-devel \
  pipewire-devel dbus-devel
# Arch
sudo pacman -S --needed base-devel clang webkit2gtk-4.1 librsvg openssl alsa-lib pipewire \
  pipewire-jack dbus
```

Then `npm ci`. Real-time audio scheduling needs rtkit.

### Windows and macOS

Install Rust, Node.js 22.18 or newer and the Tauri 2 prerequisites for the platform (WebView2
and the MSVC build tools on Windows, the Xcode command line tools on macOS), then `npm ci`. The
engine, MIDI, VST3 loading, the built-in effects and the UI are cross-platform. Plug-in editor
windows exist for Linux (X11) and macOS only, and the input level check is Linux and macOS only. CI tests all
three platforms. The macOS build has been run on a real Mac, with little hands-on testing so far;
the Windows build hasn't been run on a real machine yet
([#32](https://github.com/JackMechem/marbble/issues/32)).

Use npm: CI and the Nix package install from `package-lock.json`. A `pnpm-lock.yaml` is kept too,
so pnpm works; after changing dependencies with pnpm, run `npm install --package-lock-only`.

## Build and run

```sh
npm run tauri dev                           # build and launch
npx tauri dev -- -- path/to/Song.marbble    # open a project at launch
nix run .                                   # build and launch the package
```

`tauri.conf.json` starts Vite (`npm run dev`, port 1420) before a dev build. The dev profile in the
root `Cargo.toml` builds `daw-engine` and `vst3-host` at `opt-level = 3`, so `tauri dev` has a
release build's headroom for audio.

From the repository root (inside `nix develop` on NixOS):

```sh
cargo build -p daw                         # the app
cargo test -p daw-engine --lib             # engine unit tests
cargo test -p daw                          # app unit tests (settings, the menu bar)
cargo run -q -p daw-engine --example <name> -- <args>   # a headless check
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
npm run build                              # type check and Vite build
npm test                                   # frontend tests
```

## Tests

### What to run before sending a change

| You changed | Run |
|---|---|
| The frontend | `npm test` and `npm run build` |
| Styles | the same; the guard tests check tokens and the type scale |
| The engine | `cargo test -p daw-engine --lib`, `scripts/headless-checks.sh`, and the checks for your area that need real plug-ins |
| A built-in effect or instrument | also `fx_check` or `instrument_check`, and its bench with `--release` |
| Real-time code, the worker pool | also `rt_alloc_check`, `parallel_check` and `soak_check` |
| `src-tauri` | `cargo test -p daw` |
| Any Rust | `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` |
| The docs | `cd site && npm ci && npm run build` (it fails on a broken link) |
| A feature's scope | its entry in `features/`, then `python3 scripts/features.py` |
| Anything visible | a look in the running app |

### Frontend tests

`npm test` runs `node --test "tests/**/*.test.ts"` with Node's own runner; Node 22.18 or newer
runs the TypeScript directly. `tests/` mirrors `src/`: the test of `src/tracks/takes/comps.ts` is
`tests/tracks/takes/comps.test.ts`. Most tests load one pure module (one that imports no Tauri or
React at run time) and check its logic: edit maths, hit testing, layouts, formats.

**Guard tests** read the sources instead and enforce rules that would otherwise only fail in the
running app: `tests/codebase/` (ARIA states, button keys, no browser dialogs, the Nix source set, the
site's release links), `tests/styles/styles.test.ts` (tokens, type scale, imports), and others
beside the module they guard (`tests/app/commands.test.ts`, `tests/engine/tauriCommands.test.ts`,
`tests/state/storeSelectors.test.ts`, `tests/keyboard/sheetKeys.test.ts`,
`tests/theme/contrast.test.ts`). Each says at the top what it checks.

### Engine and app unit tests

`cargo test -p daw-engine --lib` needs no audio device. Pure modules test themselves in a `tests`
module or a `tests.rs` beside them; `session/tests/` holds the session tests that span areas, and
`dsp/tests.rs` drives a small `Engine` through its `rig()`, which the tests in `dsp/`'s folders
use too. The worker pool's tests include workers refused real-time priority and a worker stuck in
a plug-in; where rtkit grants real-time priority, one makes a worker spin past `RLIMIT_RTTIME`, so
a test binary killed by SIGKILL there is a failure. On macOS its twin checks that a worker gets a
Mach time-constraint policy and loses it once given up on.

`cargo test -p daw` covers settings files (old and malformed ones load), the menu bar's spec and,
on Linux, that GTK draws each key in `tests/keyboard/gtkKeyLabels.json` as the app writes it. On
Linux `schemas.rs`'s tests need GTK's file chooser schema, as the dev shell has it.

### Headless checks

`crates/engine/examples/*_check.rs` run a real `Session` on ALSA's `null` output (silent, no clock,
so as fast as the CPU allows) and print a `PASS` or `FAIL` line per check. Each file says at the top
what it checks and what arguments it takes; most take an audio file and a scratch folder, which must
be an absolute path and is used as the project folder. Never point it at your real projects.

```sh
scripts/headless-checks.sh                               # every check that needs no third-party plug-in
scripts/headless-checks.sh takes_check fx_check          # just these
scripts/headless-checks.sh --scratch /tmp/marbble-checks # keep the projects and logs
cargo run -q -p daw-engine --example stack_check -- /path/to/tone.wav /tmp/marbble-scratch
```

The script builds the examples once, writes a test tone (`test_tone`), and runs each check in its
own folder as on a fresh CI runner: `HOME` and the `XDG_*` folders point into the scratch folder and
the audio server variables are cleared, so nothing reads or touches your settings. A check fails
when it exits non-zero, prints a `FAIL` line, or runs longer than `HEADLESS_CHECK_TIMEOUT` (600 s),
when it and anything it started are killed. The limit uses coreutils' `timeout` (`gtimeout` on a Mac
with Homebrew's coreutils), or a few lines of Perl where neither is installed, as on a stock Mac.
Its output is in `<scratch>/<check>/output.log`. On macOS the engine runs on a silent stand-in
device. Register a new check in the script's `CHECKS` list.

- **Need real plug-ins, so run by hand:** `pdc_check` and `freeze_check` (ZaMaximX2),
  `onoff_vst_check` (ZaMaximX2, TONE3000), and the plug-in parts of `fx_check`,
  `automation_check` and `bounce_check`, which print `SKIP` without them.
  `vst_sidechain_check`, `multiout_check`, `multiout_freeze_check` and `vst_undo_check` find LSP's
  plug-ins themselves, so they're in the script and `SKIP` where they aren't installed.
- **Needs a real output device, so run by hand on macOS:** `workgroup_check` plays silence to the
  default output (never an input) on three real-time workers and checks they joined the device's IO
  workgroup, before and after reopening it. It prints `SKIP` without an output device or elsewhere
  than macOS, so it stays out of the script, which never opens a real device.
- **LSP's Multi-Sampler loses a sample now and then** while it loads, through a bug of LSP's
  (issue #37): its VST3 wrapper's task queue (`vst3::Executor::submit` in lsp-plugin-fw) reports
  a task it failed to queue as queued, so the sampler waits for that file's load or render for
  ever. It fails when two threads reach the queue at once, so it's likelier with several
  instances loading on several threads. `multiout_check`, `multiout_freeze_check` and
  `parallel_check` load a fresh instance when it happens and say so. vst3-host's
  `lsp_sampler_loads` measures it without the engine: 12 of 600 loads, four at a time, against
  none of 1200 with that one line fixed.
- **The broad ones:** `restore_check` (every feature undone, redone, saved and reopened with
  bit-identical renders), `soak_check` (hundreds of seeded random edits with the pool playing,
  invariants after each, and a replay that must match), `rt_alloc_check` (no block allocates),
  `parallel_check` (the pool matches the serial loop bit for bit). They build on
  `examples/common/all_features.rs`; a new feature belongs there.
- **Read, not checked:** `undo_check` and `import_check` print what they did.
- **Benches** (run with `--release`): `fx_bench`, `instrument_bench`, `sampler_bench`,
  `parallel_bench`.
- **The stress check** (run with `--release`): `stress_check` plays one seeded heavy project
  (32 instrument and 8 audio tracks of built-in instruments and effects, summing stacks, two aux
  returns; no plug-ins or sample files) on the real default output, never an input, at each
  buffer size and thread count, 20 s each, and prints per configuration the late blocks, the
  device gaps (callbacks that came after the previous one's audio ran out), the System Overloads
  and the blocks' p50 / p99 / max load in % of their deadline. `--threads` takes counts and `auto`,
  Settings › Processing Threads › Automatic resolved as the app resolves it (shown as `auto(4)`);
  by default it runs 1 to what Automatic is on the machine (at least 2), then `auto`. `--json` writes the results; `--max-overloads`, `--max-gaps`, `--max-p99` or
  `--baseline` an earlier JSON make it a gate that exits 1. Measure on a quiet machine, before and
  after a change to the engine's hot paths or the worker pool:

  ```sh
  cargo run --release -p daw-engine --example stress_check -- --json before.json
  cargo run --release -p daw-engine --example stress_check -- --baseline before.json --buffers 128,256 --threads 1,auto
  ```

  The options are at the top of the file. The script runs it briefly on the null device
  (`--null`), only to see that it runs: with no clock there, its loads mean little.
- **Tools:** `test_tone`, `demo_project`, `showcase_project` (the website's screenshots), `hosts`
  (audio devices), `input_level`, `smoke` (records from the real default input).
- `crates/vst3-host/examples/`: `probe` scans and runs installed plug-ins; `editor` opens one
  plug-in's editor; `editor_load` measures what opening one costs the thread playing it;
  `lsp_sampler_loads` counts the samples LSP's Multi-Sampler loses. `scripts/editor-scale-check.sh` checks on a private Xvfb display that editors
  double at `GDK_SCALE=2`.

A new check follows `stack_check.rs`: a `Session` on `alsa:null`, its folder set to the scratch
folder, the cycle on, and playing and reading meters through `examples/common/transport.rs`
(`transport::measure` plays until the sound shows up, so a slow runner doesn't read silence).

### ThreadSanitizer

The worker pool's tests and checks can run under ThreadSanitizer, with a toolchain that has its
runtime and the standard library's source (a rustup stable has both):

```sh
RUSTC_BOOTSTRAP=1 RUSTFLAGS='-Zsanitizer=thread -Cunsafe-allow-abi-mismatch=sanitizer' \
  cargo -Zbuild-std test --target x86_64-unknown-linux-gnu -p daw-engine --lib dsp:: -- --test-threads=1
```

The same flags build `parallel_check` and `soak_check`. LSP's plug-ins aren't instrumented and
report races inside their own loading and teardown.

## Debugging

- **Logs** go to the terminal that started the app, at `info`. `RUST_LOG=debug` (or
  `RUST_LOG=daw_engine=debug`) shows more; `RUST_BACKTRACE=1` adds backtraces to a caught engine
  panic. The same lines are kept in memory for Window › Diagnostics.
- **The Diagnostics window** (Window › Diagnostics) shows the device, the engine's load and
  counts of what went wrong, the threads and which one played each track, memory, MIDI and the
  log, live; Copy Report gives it all as text for an issue.
- **A failed command** shows its error as a toast. Everything the UI knows is `getState().snap`
  and the last tick (`currentTick()`, `src/state/store.ts`).
- **Blank window on Linux:** some GPU drivers need `WEBKIT_DISABLE_DMABUF_RENDERER=1`. Leave it
  unset otherwise; WebKit then paints on the CPU.
- **Wayland:** the app runs through XWayland unless `MARBBLE_WAYLAND=1` (or your own
  `GDK_BACKEND`). On native Wayland a black OpenGL editor with "X11 error from a plug-in GUI
  ignored" in the log is a call into the plug-in that isn't wrapped in `gl_isolated`.
- **File dialogs** need GTK's `org.gtk.Settings.FileChooser` GSettings schema; without it GLib
  would abort the process, so the app shows a message instead (`src-tauri/src/schemas.rs`). The dev
  shell provides it, and a dev binary started outside the shell finds it again by itself.
- **Audio:** the status line shows device, rate and buffer; Settings › Audio the measured
  latencies; `cargo run -p daw-engine --example hosts` what cpal sees. "System overload" means the
  callbacks ran late for 60 ms in a row; the CPU meter's peak and late-block count show how close
  you are.
- **Plug-ins:** `cargo run -p vst3-host --example probe -- <name>` loads and runs plug-ins outside
  the app. Settings › Plug-ins shows the last scan's report. Delete `plugins.json` in the config
  folder (or Rescan) to scan again.
- **The update check** in a debug build compares against `MARBBLE_FAKE_VERSION` when set.
- **Projects** are plain JSON in `project.json`, safe to read, and editable by hand while the app is
  closed.

## Conventions

### Where code goes

- **By feature.** Find the folder whose name says what the code is about (the trees are in
  [ARCHITECTURE.md › The source tree](ARCHITECTURE.md#2-the-source-tree)). Engine edits go in
  `crates/engine/src/session/<area>/` as another `impl Session`, pure logic on the document in
  `edit/`, document types in `model/`, real-time code in `dsp/`. A Tauri command goes at the end of
  `src-tauri/src/commands/<area>.rs` and of the list in `main.rs`. Frontend code goes in the
  feature's folder under `src/`, styles in its area's file in `src/styles/`.
- **One responsibility per file.** A file that grows past about 400 lines of non-test code, or
  whose functions fall into separate groups, splits along those groups; a cohesive DSP kernel can
  stay whole. A Rust module that splits becomes a folder: types stay in `mod.rs`, where children
  can reach private fields, and the code moves into children. Tests move to a `tests.rs` of their
  own.
- **Names say what a thing is.** No `utils`, `misc` or `ops`. A pure model beside its component is
  `<thing>Model.ts`, so no two files in a folder differ only by case.
- **Small wiring edits.** New code goes in a new file, wired in with small edits elsewhere. New
  struct fields go at the end, new commands at the end of their list, new CSS at the end of its
  area's file, new menu items beside related ones.
- **No re-exports for old paths.** When something moves, update its callers.

### Comments

- **Every file starts with a comment** of a few lines saying what it holds and, where useful, where
  the neighbouring concern lives. A folder's entry point (`mod.rs`, or the main file) says what the
  folder holds. Detail about one file belongs in its top comment, not in these docs.
- Public items get doc comments. Comments say why, not what.
- Behaviour and labels follow Logic Pro unless there's a reason not to, and the reason goes in a
  comment.

### Rust

- **Real-time code** follows [ARCHITECTURE.md › Real-time rules](ARCHITECTURE.md#4-real-time-rules):
  nothing on the audio thread allocates, locks, blocks, does I/O or waits on another thread.
- **Undo.** Every user edit goes through a checkpointed command (`edit`, `try_edit`, `edit_made`).
  Anything sent to the engine is also handled in `restore`, `build_track` and `rebuild_graph`.
- **Compatibility.** Every new field in `model/` or `prefs.rs` gets `#[serde(default)]`, and
  optional ones `skip_serializing_if = "Option::is_none"`, so older projects and settings load.
- **Main thread for plug-ins.** VST3 controllers and editors are touched only on the main thread.
  Long work goes in an async command, which hands edits back with `run_on_main_thread`.
- **Every platform.** Platform code stays behind `cfg`. VST3 enum casts stay explicit even where
  clippy calls them a no-op on Linux: the C enums are `i32` in the MSVC bindings.
- **Clippy** is clean with `-D warnings` on Linux and macOS, on the toolchain pinned as
  `LINT_TOOLCHAIN` in `.github/workflows/ci.yml`. To move to a newer one, bump it there and fix
  what it reports in the same commit. A narrow `#[allow]` with a one-line reason is fine where a lint would make DSP or a
  table worse.
- **Formatting** is `cargo fmt --all` (`rustfmt.toml`: lines up to 160 columns).
  `.git-blame-ignore-revs` lists the one project-wide pass; `git config blame.ignoreRevsFile
  .git-blame-ignore-revs` makes blame skip it.

### TypeScript and React

- TypeScript is strict, with no unused locals or parameters.
- **A module with no screen in it stays testable**: it imports no Tauri or React at run time, its
  relative imports carry the extension (`from "./midiEdit.ts"`), and it uses no syntax Node must
  transform (no `enum`: use a `const` object). `tests/theme/themes.test.ts` shows how to test a
  module that imports the store.
- **Store selectors** return the same value while nothing changed; never mutate the snapshot or a
  synced field. See [ARCHITECTURE.md › The frontend](ARCHITECTURE.md#12-the-frontend).
- **A canvas editor** is a component that owns the refs and effects, with drawing and pointer
  handling in plain functions beside it. Hooks stay in the component; imperative code reads refs,
  not render closures.
- **Never `window.prompt`, `confirm` or `alert`** (macOS's web view lacks them): use `askName`
  (`src/ui/NameSheet.tsx`) or the dialog plug-in. File dialogs go through
  `src/project/fileDialog.ts`.

### Styles and the design system

- **One file per area in `src/styles/`.** `index.css` imports them, and its order is the cascade.
  When two branches append to the same file or to the end of `index.css`, check the merged tail by
  hand and run `npm run build`.
- **Colours come from tokens.** No hex, `rgb()` or `hsl()` colour outside `tokens.css` (a test
  fails on one). A colour that's the same in every theme (a piano key) still gets a token, and so
  does a tint (`--scrim`, `--shade`…), since a white line that reads on Studio vanishes on Light.
- **Type scale.** Six sizes in `tokens.css`: `--fs-xs` 10 px, `--fs-sm` 11, `--fs-md` 12 (the body),
  `--fs-lg` 13, `--fs-xl` 15, `--fs-xxl` 17. Text uses `--fs-sm` and up; `--fs-xs` only for numbers
  and captions in fixed-size readouts. Display numerals sized to their box (the LCD) use 18 px or
  more. Canvas text uses `canvasFont("sm", 600)` (`src/ui/canvasText.ts`), never a px literal.
- **Tooltips:** give the element a `title`; `src/ui/tooltip.ts` shows it in Marbble's own box.
  Add a command's key with `tip()` (`src/keyboard/keymap.ts`). Don't draw tooltips of your own.
- **Quiet defaults.** Colour marks what differs from the default: Read automation is grey, Touch
  and Latch are coloured; the power button stands out only when a track is off.
- **Icons, not words,** on buttons, with an `aria-label` (usually the tooltip).
- **Keyboard and ARIA.** `:focus-visible` draws `--focus-ring`, after Tab only; no bare
  `outline: none`. Knobs and faders use `sliderAria` and `sliderKeyDown`; toggles say they're on
  with `aria-pressed`; a button that acts on mouse-down spreads `onPress(handler)` so Return and
  Space press it too (`src/keyboard/keyNav.ts`). Slots use `slotProps`, lists `useListKeys`.
- **Sizes.** Design for 1280×800 and 1920×1080, at interface sizes 100 % and 125 %, in the Studio,
  Light and High Contrast themes. Bars keep to one row by giving way in steps (`useBarFit`).

## Common changes

**A feature, or more of one.** Change its entry in `features/<area>.toml` (`status`, and a `note`
saying what's still missing if it's partial; the fields are in `features/README.md`), then run
`python3 scripts/features.py` to regenerate `docs/FEATURES.md` and `ROADMAP.md`'s beta progress.

**An undoable command.**

1. A method on `Session` in the fitting `session/<area>/` file: change `self.project`, send the
   engine what it needs (`push_regions`, `push_all_params`, `sync_tracks`), set `self.dirty`.
2. A command at the end of `src-tauri/src/commands/<area>.rs`, wrapped in `edit`, `try_edit` or
   `edit_made` with the label Edit › Undo shows; a `key` to merge a drag into one step; `plugins`
   when it can remove plug-ins. Register it at the end of the list in `src-tauri/src/main.rs`.
3. Call it with `act("name", { camelCaseArgs })` (`src/engine/commands.ts`).
4. Check undo and redo play the right thing, best with a headless check.

**An engine `Cmd`.** Add the variant at the end of `enum Cmd` (`dsp/cmd.rs`), with small `Copy`
payloads or something the session built (`Box`, `Arc`). Handle it in `Engine::apply`
(`dsp/apply.rs`), sending anything replaced back with `self.discard(Garbage::…)`. Use it in
`dsp/process.rs` or `dsp/strip.rs` without allocating; preallocate in `TrackDsp::new` or
`Engine::new`. Send it from `build_track` and `sync_globals` (opening a project) and from
`restore` (undo), and if renders must save it, from `Engine::render` (`dsp/offline.rs`). Test it in
`dsp/tests.rs` or a headless check.

**A project field.** Add it at the end of `Project` (`model/project.rs`) with
`#[serde(default)]`, mirror it in `src/engine/project.ts`, edit it through an undoable command, and
fix the struct literals tests and examples build by hand.

**A setting.** Engine behaviour goes in the right struct in `crates/engine/src/prefs.rs`,
interface-only options in `UiPrefs` (`src-tauri/src/settings.rs`), with their defaults. Mirror it in
`src/settings/appSettings.ts`, add the control to its pane in `src/settings/panes/` (changed with
`setPrefs` or `setUi` from `parts.tsx`), and apply it in `Session::set_prefs`
(`session/project/settings.rs`) if the engine must react.

**A key command.** An entry in `KEY_COMMANDS` (`src/keyboard/keymap.ts`: id, label, group,
default keys), and its action under the same id in `KEY_ACTIONS` (`src/app/keyCommands.ts`). When a
menu item does the same, call `handleMenu` with its id. `tests/app/commands.test.ts` fails on a
missing action or a clashing key.

**A menu item.** Add it in `src-tauri/src/menu/spec.rs` beside related items, and a `case` in
`handleMenu` (`src/app/menuCommands.ts`). Give it an accelerator only for ⌘ shortcuts that should
work in text fields (and list it in `MENU_SHORTCUTS`); other keys belong to a key command with the
item's id, which the menu bar then shows. Its tick and greyed-out state come from the Tracks area's
menus when they have the item (name it from `src/app/menuBar/menuLabels.ts`), else from
`ownStates` in `src/app/menuBar/menuBar.ts`. List it in the User Guide's menu table.

**A built-in effect.** `crates/engine/src/fx/<name>.rs` (or a folder): a struct implementing
`Effect`, its `PARAMS`, `PRESETS` and `info()` with the id `builtin:<name>`; list it in
`fx::registry()` and `fx::factory_presets()`. Test silence, latency, presets in range and stability
at extremes (see `fx/limiter/tests.rs`). A panel is optional: `src/plugins/effects/<Name>.tsx` taking
`FxPanelProps` (start from `Gain.tsx`), registered in `FX_PANELS` (`src/plugins/builtinEffects.ts`);
call `touch(id, on)` around drags so automation records them. Run `fx_check` and `fx_bench`.

**A built-in instrument.** `crates/engine/src/instrument/<name>/` with `mod.rs` (implementing
`Instrument`, `info()` with the id `builtin:<name>`), `params.rs`, `presets.rs` and a `tests.rs` on
`instrument/test_kit.rs`; list it in `instrument::registry()`. `uno/` is a small complete model.
**Naming:** an original name in the product and presets, with no maker's trademark; the top comment
may say what it's modelled on and that Marbble isn't affiliated. Add the model to
`tests/plugins/instruments/builtinInstruments.test.ts`, which scans for trademarks, and to `MODELS`
in `examples/common/all_features.rs`; run `instrument_check`, `rt_alloc_check`, `parallel_check`
and `instrument_bench`.

**A panel.** A sheet uses `Sheet` (`src/ui/Sheet.tsx`), shown by a store flag. A Settings pane is
a file in `src/settings/panes/` and an entry in `tabs` in `src/settings/Settings.tsx`. A plug-in
panel registers in `FX_PANELS`. A bottom-section pane is a `BottomPane` value in
`src/state/store.ts`, rendered in `src/app/App.tsx`, and detachable once registered in
`src/app/detach/panelKinds.tsx` with a `DetachButton` in its bar.

**A theme colour.** A CSS variable goes in `studioCss` (`src/theme/studio.ts`) and the same value in
`tokens.css`, overridden in the other themes only where it must differ (check Light and High
Contrast). A canvas colour is a field of `CanvasTheme` (`src/theme/canvasTheme.ts`) with values in
`studioCanvas` and where needed the others.

## CI

`.github/workflows/ci.yml` runs on every push to `main` and every pull request, on Ubuntu, Windows
and macOS: `npm ci`, `npm run build`, `npm test`, `cargo test -p daw-engine --lib`,
`cargo test -p daw` and `cargo check --workspace --all-targets` (so the examples must compile). On
Linux it also checks `cargo fmt` and that `docs/FEATURES.md` and `ROADMAP.md` match `features/`;
on Linux and macOS it runs clippy on the pinned toolchain (so code behind `cfg(target_os = "macos")`
is linted too) and `scripts/headless-checks.sh`. A `nix` job runs `nix flake check`, which builds
the package and runs the engine's unit tests. Checks that need real plug-ins don't run in CI.

`.github/workflows/pages.yml` builds the website from `docs/`, `ROADMAP.md`, `CONTRIBUTING.md` and
`CHANGELOG.md` when they change on `main` and publishes it to <https://marbble.dev/>. `site/docs.config.mjs` groups each document's
`##` sections into the sidebar and splits long ones into pages; a new section needs a place there,
and a broken link fails the build.

## Packaging and releases

```sh
nix build .               # result/bin/marbble, a desktop entry and icons
npm run tauri build       # native bundles for this platform (deb, rpm, AppImage, NSIS, dmg…)
```

The Nix package (`nix/package.nix`) builds the frontend from `package-lock.json`, vendors the
crates from `Cargo.lock`, runs the engine's unit tests and installs `daw` with a `marbble` symlink.
Its source is a file set, so editing docs doesn't rebuild it (`tests/codebase/nixSource.test.ts`
checks the set). Every bundle carries `LICENSE` and needs GTK's file chooser schema at run time: the
Nix wrapper, the AppImage's GTK plug-in and the .deb and .rpm's GTK dependency provide it, and a new
packaging route must too.

### Versioning and releases

Marbble follows [semantic versioning](https://semver.org); prereleases carry a suffix
(`0.1.0-alpha.12`), and each release is tagged `v<version>`. [CHANGELOG.md](../CHANGELOG.md) has
an `[Unreleased]` section at the top, where every change a user would notice gets a line.

`scripts/set-version.sh <version>` writes the version everywhere it's recorded (the workspace
`Cargo.toml` and `Cargo.lock`, `package.json` and `package-lock.json`, `tauri.conf.json`,
`nix/package.nix`). It needs GNU sed. To release:

1. Move the `[Unreleased]` entries under `## [<version>] - <date>`; the release notes come from it.
2. `scripts/set-version.sh <version>`, then commit.
3. `git tag v<version> && git push origin main v<version>`.

The tag runs `.github/workflows/release.yml`: a draft release with the version's changelog section
(a prerelease when the version has a suffix), bundles built on five runners (Linux x86_64 and
aarch64: .deb, .AppImage and, without a suffix, .rpm; Windows: the NSIS installer; macOS: .app and
.dmg for Apple Silicon and Intel), then publishing. macOS bundles are ad-hoc signed with the hardened
runtime and `src-tauri/Entitlements.plist`; nothing is notarized yet. Push to the `release-check`
branch to try the workflow without releasing anything.

### Release channels

- **Stable:** versions without a suffix.
- **Unstable:** every release, prereleases included, so it's never behind Stable.

Each channel is also a branch, `stable` and `unstable`, that the release workflow moves to the
release's commit when it is the newest on that channel. Nix users pin
`github:JackMechem/marbble/stable` or `/unstable`. Nothing is committed to those branches by hand.
