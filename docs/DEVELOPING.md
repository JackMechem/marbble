# Developing Marbble

How to build, run and test Marbble, how to debug it, the conventions the code follows, and
step-by-step recipes for the most common changes. Read [ARCHITECTURE.md](ARCHITECTURE.md)
first for how the pieces fit together.

The repository is at <https://github.com/JackMechem/marbble>; the main branch is `main`.

Contents:

- [Build and run](#build-and-run)
- [Tests](#tests)
- [Debugging](#debugging)
- [Where things live](#where-things-live)
- [Conventions](#conventions)
- [The frontend: where things live](#the-frontend-where-things-live)
- [Recipes](#recipes)
- [Packaging](#packaging)
- [Versioning and releases](#versioning-and-releases)

## Build and run

The workspace has three crates: `vst3-host`, `daw-engine` (`crates/engine`) and `daw`
(`src-tauri`, the application). The frontend is in `src/` and builds with Vite.

### NixOS and Nix (flake)

The flake provides the package, an app, a dev shell, an overlay and a check, for
`x86_64-linux` and `aarch64-linux`.

```sh
nix develop               # or `nix-shell`: Rust, Node, WebKitGTK, ALSA/JACK/PipeWire
pnpm install              # or `npm install`
pnpm tauri dev            # or `npm run tauri dev`
pnpm tauri dev -- -- path/to/Song.marbble   # open a project at launch
```

- `nix/shell.nix` takes its inputs from the package (`inputsFrom = [ marbble ]`), so the shell
  and `nix build` use the same libraries. It adds cargo, rustc, clippy, rustfmt, rust-analyzer
  and Node, sets `GIO_MODULE_DIR` and the GSettings schemas for GTK file dialogs, and puts ALSA,
  JACK, PipeWire and D-Bus on `LD_LIBRARY_PATH` for dev builds. A dev binary started outside
  the shell finds the schemas again by itself (it remembers the shell's `XDG_DATA_DIRS` from
  the build; see ARCHITECTURE.md › File dialogs); where it can't, file dialogs show a message
  instead of opening, and the app keeps running.
- `tauri.conf.json` runs `npm run dev` (Vite on port 1420) before a dev build and
  `npm run build` before a release build, whichever package manager you use to start Tauri.
- Both `package-lock.json` and `pnpm-lock.yaml` are kept. The Nix package installs npm
  dependencies from `package-lock.json`, so keep it current when you change dependencies (see
  [Packaging](#packaging)).
- For real-time audio scheduling, rtkit must be available (`security.rtkit.enable = true;`
  on NixOS; PipeWire setups usually have it).

Other flake outputs:

```sh
nix run .                 # build and launch Marbble
nix build .               # result/bin/marbble (and result/bin/daw), desktop entry and icons
nix profile install .     # install it for your user
nix flake check           # builds the package, which runs the engine's unit tests
```

To use it in a NixOS or home-manager configuration, take the flake as an input and use
`marbble.packages.${system}.default`, or add `marbble.overlays.default` to
`nixpkgs.overlays` and use `pkgs.marbble`.

Flakes only see files that git tracks, so `git add` new files before `nix build`.

### Other Linux distributions

Install Rust (stable), Node.js (22 or newer; see [Tests](#frontend-tests)), the Tauri 2
prerequisites (`webkit2gtk-4.1`, `gtk3`, `libsoup-3`, `librsvg`, `openssl`), and `alsa-lib`,
`libpipewire-0.3`, `jack` (libjack2), `dbus`, `pkg-config` and `libclang` (bindgen uses it to
generate the PipeWire bindings). Then:

```sh
pnpm install && pnpm tauri dev
```

### Windows and macOS

The audio engine (cpal: WASAPI on Windows, Core Audio on macOS), MIDI (midir), VST3 module
loading, the built-in effects and the UI are cross-platform, and `pnpm tauri build` produces
native bundles. Install Rust, Node.js and the Tauri 2 prerequisites for the platform (the
WebView2 runtime and the MSVC build tools on Windows, the Xcode command line tools on macOS).
Plug-in editor windows are implemented for Linux/X11 and macOS so far
(`crates/vst3-host/src/editor_other.rs` is the Windows placeholder), and the input level check
(`sysvol.rs`) is Linux-only. The Nix package is Linux-only. CI runs the unit tests on all
three platforms and the release workflow builds bundles for them (see
[Versioning and releases](#versioning-and-releases)), but the app gets far less hands-on testing
on Windows and macOS.

### Useful cargo commands

Run them from the repository root (inside `nix develop` on NixOS).

```sh
cargo build -p daw                       # the whole app, including the Tauri crate
cargo test -p daw-engine --lib           # engine unit tests
cargo test -p daw                        # Tauri crate tests (settings parsing, the menu bar)
cargo run -q -p daw-engine --example <name> -- <args>   # headless checks, below
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all                          # rustfmt.toml sets the rules
npx tsc --noEmit -p .                    # type-check the frontend
```

The dev profile (root `Cargo.toml`) builds the app crate at `opt-level = 1`, dependencies at
`opt-level = 2`, and `daw-engine` and `vst3-host` at `opt-level = 3`, so `tauri dev` has the
same CPU headroom for audio as a release build.

## Tests

### Rust unit tests

`cargo test -p daw-engine --lib` runs the engine's unit tests. They need no audio device.
They cover, among others: loops and real copies, routing order and loop pruning (`model/tests.rs`, `model/routing_tests.rs`);
the click's levels, accent, divisions and delay compensation (`dsp/tests.rs`); delay line taps and the
PDC solver; take folder comping, crossfades and recording plans (`takes.rs`); fade curves,
crossfade gain sums and placement (`fades.rs`, `session/crossfades.rs`); automation
evaluation and thinning; Channel EQ filters; resampling; transient detection; Standard MIDI
Files; bounce trimming, normalizing and 16-bit dither; range edits; autosave and recovery,
undo limits and recording placement (`session/options.rs`); crash-recovery journals,
recovering interrupted takes, atomic saves with a backup and falling back from a damaged
project.json (`session/recovery.rs`); repairing the WAV headers a crash leaves, for every format
the recorder writes (`wav_repair.rs`); preferences parsing; the recorder's bit depths; the tempo map; `contain()`; preset files and names (`presets.rs`)
and the preset a slot or Channel EQ has loaded, through undo, saving and renamed or deleted user presets (`session/preset_tests.rs`);
parameter normalization and dynamics effects' gain reduction (`dsp/builtin.rs`); channel
strip settings (copy and paste through JSON, onto audio and instrument tracks, missing plug-ins,
Reset, setting files: `strip_settings.rs`, `session/strip_settings.rs`); the clip indicators'
peak holds (`session/strip_meters.rs`); and each built-in effect's DSP (`fx/*.rs`: the
Limiter's ceiling and latency, the Reverb's stability, the Tuner's accuracy, the Loudness
Meter's BS.1770 weighting and true peak, the Linear Phase EQ's kernels, and more). The
modules added since cover their own pure parts too: region playback parameters
(`region_params.rs`), patterns (`pattern.rs`), comps and MIDI take folders (`takes.rs`), the
drag modes and Move Automation with Regions (`drag_modes.rs`, `auto_move.rs`), the
Arrangement track's and the arranging commands' time maps (`arrangement.rs`,
`arrange_cmds.rs`), Strip Silence, VCAs (`vca.rs`), controller assignments
(`controllers.rs`), and the session modules that apply them (`session/*.rs`). The worker pool's
tests (`dsp/pool.rs`, `dsp/parallel.rs`) include workers refused real-time priority and a worker
stuck in a plug-in, left there while the rest plays on (its thread stays blocked until the test
lets the plug-in return, or for good in the one that drops the pool around it). Where the system grants real-time priority (rtkit), one of them makes a
real-time worker spin without sleeping past RLIMIT_RTTIME's 200 ms: if the pool failed to demote
it, the kernel would kill the test binary (SIGKILL), so a run that ends that way is a failure.

`cargo test -p daw` runs the Tauri crate's tests (old and partly malformed `settings.json`
files load with defaults; the menu bar's spec, reading key specs, and, on Linux, that GTK draws
each key in `tests/gtkKeyLabels.json` as the app writes it). On Linux `schemas.rs`'s tests need
GTK's file chooser schema on the search path, as the dev shell and an Ubuntu with GTK have it;
two of them rerun the test binary with `XDG_DATA_DIRS` changed, to see that a missing schema
is reported and that startup finds it again.

### Headless examples

`crates/engine/examples` holds integration checks that run a real `Session` with the ALSA
`null` output device (silent, no clock, so it runs as fast as it can). The `*_check` examples
print one `PASS <what>` or `FAIL <what>` line per check, and all of them except `tempo_check`
exit with status 1 when a check fails; for `tempo_check`, look for `FAIL` in the output
(`... | grep -c FAIL`). `undo_check` and `import_check` print what they did for you to read
instead.

Most take an audio file and a scratch directory, which must be an absolute path and is used as
the project folder (never point it at your real projects):

```sh
cargo run -q -p daw-engine --example stack_check -- /path/to/tone.wav /tmp/marbble-scratch
```

`scripts/headless-checks.sh` builds the examples once and runs every check that needs no
third-party plug-ins (its `CHECKS` list; register a new check there), each in its own folder
under a scratch directory, with a test tone from the `test_tone` example. A check that runs
longer than `HEADLESS_CHECK_TIMEOUT` seconds (600 by default) fails instead of stalling. It's what CI runs, so run it the same way before sending an engine
change. It also runs on macOS: where there is no ALSA, the engine runs on a silent stand-in
device at the configured rate and buffer size instead of ALSA's `null` output:

```sh
scripts/headless-checks.sh                       # all of them, in a temporary folder
scripts/headless-checks.sh takes_check fx_check  # just these
scripts/headless-checks.sh --scratch /tmp/marbble-checks   # keep the projects and logs there
```

It runs the checks as on a fresh CI runner: `HOME` and the `XDG_*` folders point into the
scratch folder, and the PipeWire, PulseAudio, JACK and D-Bus session variables are cleared,
so the checks find no audio server, no settings and no plug-in cache, and never touch yours.
That makes the plug-in parts of `fx_check`, `automation_check` and `bounce_check` print `SKIP`;
`pdc_check`, `freeze_check` and `onoff_vst_check` need real plug-ins, and `undo_check` and `import_check` only
print what they did for you to read, so those four aren't in the script; run them (and the
plug-in parts) by hand. `multiout_check` and `vst_undo_check` also want real plug-ins but find them by themselves, so they're
in the script, running where the plug-ins are installed and printing `SKIP` elsewhere. Every other `*_check` in the table below is. A check counts as failed when it exits non-zero or prints a line
starting with `FAIL`; each one's output is in `<scratch>/<check>/output.log`, and the scratch
folder is kept when anything failed.

| Example | Arguments | What it checks |
|---|---|---|
| `fx_check` | audio file, scratch dir | Every built-in effect in the registry: it loads and processes, parameters set, save, reload, undo and duplicate, automation drives it, latency reaches PDC, side-chains key it (live and in isolated renders), meters reach the tick, freezing bakes it in; plug-in and Channel EQ copy and paste, presets. The VST3 paste checks need **ZaMaximX2**, and print `SKIP` without it. |
| `fx_play_check` | audio file, scratch dir | The modulation, distortion and Gain effects: each plays live, renders sanely, and a bounce of its frozen track matches the live bounce; a synced Tremolo locks to the song position; the Distortion's oversampling latency is compensated (at 0 % mix it is the dry signal, in time); Gain's invert and mono switches null a centred source. |
| `insert_order_check` | audio file, scratch dir | Reordering Audio FX inserts: the engine runs the new order, instances keep running (same ids, nothing reloaded), undo and redo restore it, moving or copying to another strip makes room there. |
| `stack_check` | audio file, scratch dir | Track stacks (folder and summing), groups and region edits, by the levels the meters report. |
| `vca_check` | audio file, scratch dir | VCA faders and pre-fader side-chain keys in offline renders: two tracks on a VCA at -6 dB render 6 dB lower, muting the VCA silences them, undo restores the level, automated member volume combines with the VCA, a frozen track and Bounce follow it; a compressor keyed pre-fader from a track with its fader at -∞ still compresses. |
| `loop_check` | audio file, scratch dir | Looping regions (playback, trim, move, convert, split, join) and hidden tracks. |
| `sends_check` | audio file, scratch dir | Sends pre and post fader, aux buses, output routing, solo through routing, loop refusal. |
| `send_auto_check` | audio file, scratch dir | A send-level automation lane changes what reaches the aux. |
| `bounce_check` | audio file, scratch dir | Bounce, Export All Tracks, Export Region and Bounce in Place: ranges, tails, formats, normalize, placement. |
| `strip_check` | audio file, scratch dir | Channel strip settings: a saved setting loads onto two tracks (new instances, same values, the EQ) as one undo step; the gain-reduction meter of compressed strips reaches the tick; a clipping track's clip indicator holds a peak over 0 dBFS through quiet playback until it (or every one) is reset, and a bounce leaves them alone. Saved settings go to the scratch folder. |
| `export_check` | audio file, scratch dir | AIFF and FLAC bounces decode to the same samples as the WAV bounce (and within quantization of a float one); FLAC refuses float; Export Selected Tracks writes just the selection, folder stacks standing for their subtracks. |
| `dragmode_check` | audio file, scratch dir | The No Overlap, Shuffle L and Shuffle R drag modes and Move Automation with Regions through the session calls the Tracks area makes: where regions end up, that moved automation is what plays, and that each edit is one undo step. |
| `crossfade_check` | audio file, scratch dir | Fade curves, Speed Up and Slow Down, and X, EqP and X S crossfades, measured in offline renders of synthetic files (X sums to unity, EqP to constant power); butt fades and gaps; the X-Fade drag mode with undo and redo; and that playback, Bounce, Export, Bounce in Place, Freeze, Join and save/reopen all sound the same. |
| `range_edit_check` | audio file, scratch dir | Audio Track Editor range edits and Split at Transients: what plays after each edit, and that one undo reverts it. |
| `automation_check` | audio file, scratch dir, optional plug-in name and parameter name | Reading volume and pan lanes, Off mode, writing in Write and Touch, undo; with a plug-in from the cache, one of its parameters too (`SKIP` when it isn't in the cache). |
| `autolanes_check` | audio file, scratch dir | Automation curves, Stereo Out automation and stack automation in offline renders: a curved volume ramp follows its curve sample by sample, a straight one is unchanged by curves existing, Stereo Out's volume lane shapes the bounce, summing and folder stacks' lanes shape their subtracks, and each edit undoes. |
| `pdc_check` | audio file, scratch dir | Plug-in delay compensation with a real latent plug-in, measured by cross-correlating offline renders. Needs **ZaMaximX2** (zam-plugins) in the plug-in cache. |
| `onoff_vst_check` | audio file, scratch dir | Track On/Off with real VST3s (ZaMaximX2, TONE3000), live: suspended plug-ins play again, fast switching, delay compensation. |
| `freeze_check` | audio file, scratch dir | Freeze: a frozen instrument track with a latent plug-in sounds as it did live, plug-ins are unloaded, edits are refused, undo, redo, unfreeze and reopening restore the right state. Needs **ZaMaximX2**. |
| `takes_check` | audio file, scratch dir | Take folders, comping, cycle and Autopunch recording layout (the null device has no input, so takes are silent), and markers. |
| `comping_check` | scratch dir | Several comps per folder (create, switch, rename, delete, with undo; each renders as it should); a MIDI cycle recording with "Create take folders" makes a take per pass, and choosing a take changes what plays; a take folder crossfaded with the next region, and a loop with Crossfade Loop Repetitions, render without a click; Crossfade Time sets the X-Fade mode's crossfades and the comp boundaries. |
| `recovery_check` | audio file (unused), scratch dir | Crash recovery: a child process records (24-bit plain with two tracks, then 32-bit cycle) and is killed with SIGKILL mid-take; reopening offers and restores the takes at the right position and length (a take folder for the cycle), repairs the WAVs and clears the journal. A normal stop autosaves at once without touching project.json. |
| `countin_check` | scratch dir | Recording with a count-in from bar 1, where the transport runs at negative positions, three times without an engine error. |
| `tempo_check` | scratch dir | The tempo map and signature changes: the click across changes, plug-in transport positions, bar numbering, Move MIDI Regions with Tempo, quantize, a MIDI file round trip, undo, and that a project without changes keeps the old timing exactly. |
| `midi_check` | scratch dir | Piano Roll edits, muted notes, controller playback and chasing, split/join/trim with controllers, MIDI file export and import. |
| `controller_check` | audio file, scratch dir | Controller assignments with messages fed through the hardware input path (`inject_midi_from`): learning the last touched parameter, volume on the fader taper, a built-in effect parameter, Pickup, ranges, buttons (Mute, Cycle, Play), automation written in Touch, assigned messages kept from the instrument, a missing target, saving and loading, delete and cancel. |
| `chase_check` | scratch dir | Chase through the built-in synth: playback started inside a long note sounds at once, and so do a jump into one and a bounce that starts inside one, but not with Chase: Notes off; a controller before the playhead is in effect only with Chase: Controllers on. |
| `midi_replace_check` | scratch dir | MIDI recorded (through the Musical Typing path) over a region: Replace in a cycle clears the cycle's notes and controllers and restores the controller value after it, undoes in one step; Merge keeps the old notes; the control bar's Replace button overrides "Create a new region"; Replace without a cycle clears from the record start to the stop. |
| `region_params_check` | audio file, scratch dir | Region playback parameters in offline renders of the built-in synth and the tone: quantize, transpose, velocity, dynamics and gate play exactly the notes worked out by hand; delays in ms and ticks (across a tempo change, and before a region's start); quantize after a tempo change; an audio region's delay; Apply Region Parameters plays the same and undoes in one step; coalesced undo; saving. |
| `projects_check` | audio file, scratch dir | Track alternatives (new, duplicate, switch, rename, undo and redo, what plays, save and reopen); backups kept on save and pruned, Revert to and undoing it; project alternatives (new, switch saving first, reopen, rename, delete, backups per alternative); a user template round trip. |
| `arrangement_check` | scratch dir | The Arrangement track: three sections with regions on two tracks (one across two section edges) and volume automation. Moving section 3 before section 2, copying section 1 and deleting section 2 with its content cut and move the regions and automation as expected; offline renders play the old audio in the new order (sample for sample without automation, within 0.5 dB with it); one undo restores each. |
| `arrange_cmds_check` | scratch dir | The arranging commands with the locators at 1–1.5 s over regions and volume automation on two tracks: Insert Silence, Cut Section and Repeat Section move the content in time and the render plays the old audio with a gap, without the section or with it twice; Insert Cut Section after a cut restores the project; Remove Gaps, Remove Overlaps and Paste Replace lay regions out as Logic does; Strip Silence splits a tone with gaps into one region per note, playing as before; one undo reverts each. |
| `pattern_check` | scratch dir | Pattern regions (Step Sequencer) in offline renders of the built-in synth: straight, swung, polymetric and other-rate rows, ties, a looped region and a tempo change play exactly the notes worked out by hand; a painted run of steps undoes in one step; Convert to MIDI Region and back render identically; saving. |
| `editing_check` | audio file, scratch dir | Snap Edits to Zero Crossings (a split off and on lands within 5 ms on a near-zero sample; a range split at both ends), a region gain drag sent as the Tracks area does (heard at -12 dB, one undo step), and a nudge in bars and beats across a 120 to 90 BPM change. |
| `pdc_glide_check` | scratch dir | Delay compensation changes while playing: a sine plays on one track while the Overdrive (38 frames), then the Linear Phase EQ (2047), are inserted on another and then removed, mid-render with the transport running (`Session::render_with_edit`), and a pre-fader send into an aux changes delay the same way. No step may exceed the sine's own (the output reports what an instant jump would have stepped), and after the 15 ms glide the output is the sine exactly as far moved as the delay. |
| `instrument_check` | scratch dir | A built-in instrument (Marbble Tre) through the real session: loading, a MIDI region's render, two renders bit-identical, parameter edits, automation, undo and redo, factory and user presets, save and reopen, duplicates having differences of their own, Freeze with its tail, Bounce in Place, a bounce's tail, Track Off, copy and paste, strip settings, MIDI learn, chase and live MIDI, a held key when the target moves or it's bypassed. That Marbble Synth still loads and plays. |
| `linphase_check` | scratch dir | The Linear Phase EQ: flat, at each Quality, it reports its latency and plays the dry signal in time; with the same settings as a Channel EQ its magnitude at eleven frequencies matches within 0.25 dB, and each sine burst comes out as the dry one scaled (no phase shift), unlike the Channel EQ's. |
| `restore_check` | scratch dir | Every feature in one project (`examples/common/all_features.rs`): each step (new effects, Linear Phase EQ and its latency, a pre-fader key, VCAs, folder, curved and Stereo Out automation, pattern regions, region parameters, take folders and comps, a MIDI take folder, loop crossfades, an arrangement move, marquee delete, Strip Silence, a zero-crossing split, track alternatives, a section cut) undone and redone with bit-identical renders and documents; all undone and all redone; save and reopen identical; an older project (the new fields stripped) opens with their defaults and renders the same. |
| `soak_check` | scratch dir, optional edit count, seed and thread count (4; 1 for the callback alone) | Feature interactions: the all-features project, then a few hundred edits drawn at random (fixed seed) from the session's public edit methods, all with the null device's callback playing every block through the worker pool. After each edit the task graph must fit the engine's routes, and every 40 a render on the pool must match the serial loop's bit for bit. After each the project must hold together (unique ids, positive lengths, comps in order, arrangement tiling, VCAs and keys that exist, delay compensation that matches the plug-ins); some are undone and redone with renders; every 50 the project saves and reopens with the same render; at the end undoing everything gives the first render back bit for bit. |
| `onoff_check` | audio file, scratch dir | Track On/Off in offline renders: an Off track leaves the mix (and comes back bit for bit), with Mute independent; a folder and a summing stack take their subtracks; an Off aux passes nothing while its senders play on; a side-chain key from an Off track is silence; a latent track leaves delay compensation, also mid-render with no click; a frozen track plays nothing; Freeze and Bounce in Place refuse and exports skip; saved and reopened; live, an Off track's meter falls to nothing. |
| `vst_sidechain_check` | audio file, scratch dir | Side-chain inputs of real VST3s, skipped when LSP's plug-ins aren't installed: bus layouts printed; Sidechain Compressor and Gate rendered with a loud key, a silent key and no source; a mono aux's mix-down; no click and unchanged settings around the `setActive` cycle; switching while playing, off and bypassed; Off sources, loops, undo, a deleted source, freeze, Bounce in Place, duplicating, channel strip settings, Stereo Out, delay compensation, reopening, a missing plug-in and a project from before side-chains. |
| `multiout_check` | scratch dir | Multi-output instruments with a real plug-in, LSP's **Multi-Sampler x12 DirectOut** with sines loaded through its state; prints `SKIP` without it (it looks in the standard VST3 folders, so it needs no cache). A note reaches the aux on its direct output, sample for sample as the main output, and not the other aux; the aux's fader, mute and solo; main and aux add up in phase with latency on either side and with a later feeder; in Low Latency Mode, played live with the instrument monitored, the aux keeps up with the main output; Track Off, bypass, Bounce, Export; Freeze and Bounce in Place refused; deleting and undoing both ways; a one-output instrument in the slot; pasted strip settings; duplicates; reopening with the plug-in and without; live on its own thread, buses switched on and off while playing with no click, the other tracks unbroken and no allocation on the engine's thread. |
| `vst_undo_check` | audio file, scratch dir | Undo for edits in a VST3's own editor with LSP's Compressor, ZamCompX2 and TONE3000, each `SKIP` when it isn't installed. No editor can open, so it calls `IComponentHandler` as one would (`drive_vst_handler`), and reads back the parameter and the saved state and measures renders and live meters. One gesture is one step with no `getState` during it; bursts without begin and end; a preset load's `restartComponent`; echoes, host-set values, automation playing, read-only and self-moving parameters make none; Touch's pass keeps the move; Off, bypassed, frozen and removed plug-ins; undo and redo fifty times while playing. Prints what `getState` costs. |
| `parallel_check` | scratch dir | Multi-core processing: a project with everything the engine has (audio and instrument tracks, summing stacks, auxes, sends before and after the fader, side-chain keys in built-in effects and VST3s, a multi-output aux, delay compensation, Off, frozen and muted tracks, automation, VCAs, solo, the metronome, input monitoring, Stereo Out's inserts) plays through renders and through `Engine::process`, with edits at fixed blocks, on 2 to 8 threads at four buffer sizes, and must match the serial loop bit for bit. Two serial runs must match too, one of them after the null device played elsewhere in the song first. With workers refused real-time priority (`PoolOptions::refuse_for_checks`) the others must play the same, or the serial loop with none left, and Settings › Audio's `left_out` must say so. VST3s are LSP's, skipped where not installed; TONE3000 isn't used, since it depends on timing. Real-time workers, where the system grants them, must live through blocks back to back. Works on any core count. |
| `rt_alloc_check` | scratch dir | Real-time safety: plays the all-features project through `Engine::process` on its own thread with a counting allocator, then again with every block on four threads, counting the workers as well, and checks that no block allocates or frees once warmed up, while playing, with a real VST3 keyed by another track where LSP's plug-ins are installed, through a latency change and its glide, edits, plug-ins and tracks coming and going, undo and redo, locating, cycling and stopping. On the pool, a plug-in then stops responding on a worker: leaving the worker in it, its track and stack sitting out, and the strip and then the track coming back once it returns allocate nothing either; the transport plays on, a render is refused meanwhile, and the session says what happened each time. |
| `undo_check` | audio file | Undo and redo through a sequence of edits. |
| `import_check` | project dir, files... | Imports files onto new tracks and reports what was created. |
| `test_tone` | output `.wav` path | Not a check: writes the test tone the checks play (3 s of 440 Hz at -21.5 dBFS, stereo, 32-bit float, 48 kHz). |
| `demo_project` | output `.marbble` path | Writes a small demo project (synthesized drums, a plucked part, a MIDI region) for trying the UI. |
| `showcase_project` | output `.marbble` path | Writes the 24-bar project the website's screenshots show: synthesized drums, shaker and guitar, all six synth models and Marbble Sampler, inserts, two aux returns with sends, a VCA, arrangement markers, an automation lane and an audio take folder with a swiped comp. Built-in plug-ins only. |
| `sampler_check` | scratch dir | Marbble Sampler through the real session: files at several rates and depths mapped by their names or chromatically, renders in tune and repeating, velocity layers, edits with undo and redo, old maps freed, recording from a simulated input (through `feed_input_for_checks`, as a device's callback would call the engine: this shows the path from the engine's input to a playable zone, not a real interface), trimming, Convert to Sampler Track whole and sliced, save and reopen, originals deleted, the project moved, Save As, a missing sample, reusable instruments, Freeze, Track Off, copying, an SFZ it generates. Sets its own HOME inside the scratch folder. |
| `sampler_bench` | scratch dir, optional seconds per measurement (default 3) | Not a check: times Marbble Sampler per block at full polyphony, at 512 and 128 frames, and how long loads take. Run it with `--release`. |
| `instrument_bench` | optional seconds per measurement (default 4) | Not a check: times the built-in instruments per block, at 512 and 128 frames, with one voice and many. Run it with `--release`. |
| `parallel_bench` | optional seconds per measurement (default 3) | Not a check: times `Engine::process` per block at real-time pace on the null device, on 1, 2, 4 and 6 threads, for 16 synths with a reverb and a chorus each and for 2 bare synths, at 512 and 128 frames, with the engine deciding when the workers help ("gated") and with every block on the pool ("forced"). Run it with `--release`, before and after a change to the pool. |
| `fx_bench` | optional block size (default 512) | Not a check: times every built-in effect per block at 48 kHz (worst and average) against the block's real-time budget. Run it with `--release`. |
| `hosts` | none | Lists every audio host and its devices. |
| `input_level` | `[--reset] [device-id]` | Prints (or resets to 100%) the system mixer volume of a recording device (Linux). |
| `smoke` | none | Lists devices, records two seconds from the default input through a plug-in and reports the region. Uses real devices. |

`crates/vst3-host/examples`:

| Example | Arguments | What it does |
|---|---|---|
| `probe` | optional name filter | Scans installed VST3 plug-ins, instantiates each and runs a few blocks through it. |
| `editor` | name substring, seconds | Opens one plug-in's editor with a small `poll()` run loop (no GTK), for debugging editor embedding. |

`scripts/editor-scale-check.sh [plug-in name ...]` runs `editor` on a private Xvfb display at
scale 1 and with `GDK_SCALE=2` and checks that each editor's window doubles (HiDPI, see
[Editor windows](ARCHITECTURE.md#editor-windows)). By default it tries LSP's Compressor Mono,
Dragonfly Hall Reverb, CHOWTapeModel and TONE3000 and skips those not installed. It needs Xvfb
and xwininfo: `nix shell nixpkgs#xorg.xorgserver nixpkgs#xorg.xwininfo -c nix develop -c
scripts/editor-scale-check.sh`. It never opens the app.

#### ThreadSanitizer

The worker pool's tests and checks can run under ThreadSanitizer. It takes a toolchain with its runtime
and the standard library's source (a rustup stable has both, `RUSTC_BOOTSTRAP=1` allows the unstable
flags), since an uninstrumented standard library gives false alarms:

```sh
RUSTC_BOOTSTRAP=1 RUSTFLAGS='-Zsanitizer=thread -Cunsafe-allow-abi-mismatch=sanitizer' \
  cargo -Zbuild-std test --target x86_64-unknown-linux-gnu -p daw-engine --lib dsp:: -- --test-threads=1
```

The same flags build `parallel_check` and `soak_check`. LSP's plug-ins aren't instrumented, and report
races inside their own loading and teardown, on no worker thread. Miri can't run this: it isn't in the
flake, and real plug-ins and threads at real-time pace aren't its kind of work.

When writing a new headless check, copy the pattern in `stack_check.rs`: a `Session` on ALSA
`alsa:null` output, `s.dir` pointed at the scratch folder, the cycle turned on so playback
loops, and about 60 ms of sleep after `stop()` before the next `play()`.

### Frontend tests

```sh
npm test                  # node --test tests/*.test.ts
npx tsc --noEmit -p .     # type check (strict, no unused locals or parameters)
```

The tests use the built-in `node:test` runner. Node 23.6 or newer runs TypeScript directly;
on Node 22 add `--experimental-strip-types`. Most test a pure frontend module (one with no
Tauri or React imports, so Node can load it); the guard tests at the end read the sources
instead and enforce rules that would otherwise only fail in the running app.

| Test | What it covers |
|---|---|
| `midiEdit.test.ts` | The Piano Roll functions in `src/midiEdit.ts`: grid lengths, snapping, quantize with strength, swing and ends, and the rest. |
| `ccCurves.test.ts` | Controller lane curves (`src/ccCurves.ts`): curve shapes, ramps, Create Ramp, Thin Out. |
| `eventList.test.ts` | The Event List's positions across tempo and signature changes, typed values and edits. |
| `noteSelect.test.ts` | Edit › Select on notes and events in the Piano Roll and the Event List. |
| `stepInput.test.ts` | Step Input: step lengths through the tempo map, the chord and advance state machine, the region edits. |
| `stepSeq.test.ts` | The Step Sequencer: step edits, rows, paint values for each edit mode, names, the rows of a new pattern by instrument, the playing step of each row. |
| `snap.test.ts` | The snap grid (`snapStep`, `snapPos` in `src/time.ts`): Bar, Beat, Division, Ticks and Smart in each signature. |
| `textFit.test.ts` | Long paths shortened at a separator (`src/textFit.ts`). |
| `tempo.test.ts` | `src/tempo.ts`, the mirror of the engine's tempo map. |
| `musicalMove.test.ts` | Region moves and nudges in bars and beats through the tempo map. |
| `audioEdit.test.ts` | The Audio Track Editor's range arithmetic (`src/audioRange.ts`). |
| `regionSelect.test.ts` | Edit › Select's region criteria (`src/regionSelect.ts`). |
| `regionParams.test.ts` | The Region inspector's playback parameter values. |
| `regionGain.test.ts` | The region gain handle's geometry and arithmetic. |
| `fades.test.ts` | Fade curves and crossfades for drawing, mirroring `fades.rs`. |
| `dragModes.test.ts` | The drag preview's copy of `drag_modes.rs`, with the Rust module's cases. |
| `newTrackDrag.test.ts` | Which source lanes a drag past the last track makes new tracks for (`src/newTrackDrag.ts`). |
| `comps.test.ts` | Take folder comp names and the folder's name bar label. |
| `arrangement.test.ts` | The Arrangement track's drawing and drag helpers. |
| `tools.test.ts` | The Tracks area's tools (`src/tools.ts`) and the marquee (`src/marquee.ts`). |
| `overview.test.ts` | The overview strip's mapping: time, track rows, the visible-area box, scroll and zoom drags. |
| `scrolling.test.ts` | Mouse Wheel Scrolls (`src/wheel.ts`) and bringing the playhead into view (`revealLeft` in `src/view.ts`). |
| `trackHeader.test.ts` | Track header layouts by height, the volume slider's meter scale, and region name bars (`src/regionBar.ts`). |
| `autoLanes.test.ts` | Automation curves, Show Used Automation, the extra lanes and their boxes. |
| `laneFollow.test.ts` | The shown lane following a plug-in moved to another track. |
| `alternatives.test.ts` | Backup labels, alternative names and track alternatives (`src/alternativesModel.ts`). |
| `bounceFormat.test.ts` | The Bounce dialog's formats, resolutions and file names. |
| `fxFormat.test.ts` | Built-in effect readouts and control mapping. |
| `fxShapes.test.ts` | The curves the modulation, distortion and Gain panels draw, against the engine's values. |
| `paneSizes.test.ts` | Pane sizes (`src/paneSizes.ts`): limits, malformed saved sizes, the Tracks area's minimum and the bottom section's room; guards that the window's grid column can't grow with its content and that the bottom panes shrink. |
| `presetBox.test.ts` | The preset pop-up's label (`src/presetBox.ts`): the loaded preset's name, "(edited)", "Default" and "Manual". |
| `fxDock.test.ts` | Plug-in panels docked or floating: the slot toggle, closing back, one Channel EQ at a time, Esc, pruning, resize maths and saved sizes. |
| `linearPhaseEq.test.ts` | The Linear Phase EQ's parameters as Channel EQ settings. |
| `cpuMeter.test.ts` | The CPU meter's tooltip, its figure and colour bands, and where its peak tick goes. |
| `sliderKeys.test.ts` | Keyboard control of knobs, faders and sliders (`src/sliderKeys.ts`): steps, ends, rounding and switch positions, the ARIA props. |
| `focusRing.test.ts` | When the keyboard focus ring shows (`src/focusRing.ts`): on after Tab, off after a mouse press. |
| `stripMeters.test.ts` | The clip indicator readout and the gain-reduction meter. |
| `vca.test.ts` | VCA offsets and loop checks in the Mixer. |
| `trackOff.test.ts` | Whether a track is off through its stacks (`src/trackOff.ts`), and the power button's tooltip. |
| `controllers.test.ts` | Controller assignment types and labels (MIDI Learn). |
| `keyFormat.test.ts` | Shortcuts written as each platform's menu bar writes them; on Linux against `gtkKeyLabels.json`, the pairs `menu.rs`'s tests check against GTK. |
| `menuLabels.test.ts` | Guard: the menu bar (`menu.rs`) uses the names in `src/menuLabels.ts`, both Edit menus have the same layout, right-click short names only drop "Regions", and no key is both an accelerator and a key command (or missing from `MENU_SHORTCUTS`). |
| `menuBar.test.ts` | The menu bar's item states from the in-app menus and key commands, and sending only what changed (`src/menuBarModel.ts`). |
| `pluginKeys.test.ts` | Keys forwarded from a plug-in's editor window become the KeyboardEvent the main window would see. |
| `detach.test.ts` | Detached windows (`src/detachModel.ts`): panel keys and labels, the shared UI state, fitting windows onto the monitors. |
| `sheetKeys.test.ts` | Which keys reach the app while sheets are open (`src/sheetKeysModel.ts`), and that every sheet registers with `sheetKeys.ts`. |
| `storeSelectors.test.ts` | Guard: no `useStore`, `useEd` or `useSettings` selector builds a new array or object on each call (which re-renders forever and blanks the window). |
| `commands.test.ts` | Guard: every key command and native menu item is handled, and no two key commands share a default key. |
| `tauriCommands.test.ts` | Guard: every Tauri command the frontend calls is registered, and each call passes its required arguments in camelCase and nothing else. |
| `pianoRollModel.test.ts` | The Piano Roll's lanes, what a point hits, its grid sums in musical time, where a region opens and the lane's share. |
| `regionGrab.test.ts` | What a press on a region in the Tracks area grabs: the loop tool, trims and fades. |
| `audioEditorModel.test.ts` | The Audio Track Editor's layout, zoom range and what a press grabs. |
| `automationParams.test.ts` | Automation parameters: labels, each strip's list, Stereo Out as a track, the lane's scale. |
| `settingsText.test.ts` | The MIDI monitor's wording, plug-in types and the Recovery sheet's texts. |
| `updateCheck.test.ts` | The update check (`src/updateCheck.ts`): reading tags as versions, semver precedence with prereleases, each channel's newest release (drafts and non-version tags skipped), when an update is offered, and what the check says. |
| `compressorModel.test.ts` | The transfer curve's copy of `static_reduction` for each circuit. |
| `dynScale.test.ts` | The dynamics panels' value formatting and knob maths. |
| `styles.test.ts` | Guard: `src/styles/index.css` imports every stylesheet exactly once with the tokens first, no other file imports CSS, and each file's braces balance; no raw hex colour outside `tokens.css`; every font size from the type scale; `tokens.css` matches Studio in `themes.ts`; the focus ring. |
| `browserDialogs.test.ts` | Guard: no `window.prompt`, `confirm` or `alert` in `src/` (macOS's web view doesn't implement them; use `askName` or the dialog plug-in). |
| `contrast.test.ts` | Guard: in every theme of `themes.ts`, `--text`, `--dim` and `--faint` are at least 4.5:1 (WCAG) on each surface text sits on, `--lcd-label` on the LCD, and the three stay in that order. |

### What to run before sending a change

- `cargo test -p daw-engine --lib` and `cargo test -p daw`
- every headless example that touches your area (for engine changes,
  `scripts/headless-checks.sh` and the plug-in checks `pdc_check` and `freeze_check`); for a
  built-in effect, `fx_check` and `fx_bench`
- `npm test`, `npx tsc --noEmit -p .` and `npm run build` (the Vite bundle and CSS)
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`
- a visual check in the running app for UI changes

## Debugging

- **Logs**: the app uses `env_logger` with the default filter `info,jack=off`, printed to the
  terminal that started it. Set `RUST_LOG=debug` (or `RUST_LOG=daw_engine=debug`) for more.
  Plug-in load failures and missing audio files are logged as errors; autosaves at info level;
  plug-in restart flags at debug level.
- **Frontend errors**: a failed command shows its error as a toast (`report` in `api.ts`). The
  app replaces the webview's right-click menu with its own.
- **Blank window on Linux**: some GPU drivers need `WEBKIT_DISABLE_DMABUF_RENDERER=1`. Leave it
  unset otherwise, because WebKit then paints on the CPU at much higher cost.
- **Wayland**: `main.rs` sets `GDK_BACKEND=x11` unless you set it yourself, so the interface
  runs through XWayland like the plug-in editors. `MARBBLE_WAYLAND=1` (or `GDK_BACKEND=wayland`)
  runs it natively; the log then says "running natively on Wayland", and GL contexts are kept
  apart around plug-in calls (see [Editor windows](ARCHITECTURE.md#editor-windows)). An OpenGL
  editor that stays black there, with "X11 error from a plug-in GUI ignored (error 10, request
  N)" in the log where N is the GLX opcode, is a call into the plug-in that isn't wrapped in
  `gl_isolated`.
- **Plug-in editor scale**: the log says "plug-in editor scale" with where it came from when an
  editor opens at a scale other than 1. `GDK_SCALE=2` forces 2x for X11 programs.
- **Audio problems**: the status line shows the device, rate and buffer; Settings › Audio shows
  the measured latencies. `cargo run -p daw-engine --example hosts` lists what cpal sees.
  The tick carries an `overruns` counter that grows when a recording ring buffer overflowed.
- **Engine errors**: a panic inside `Engine::process` is caught in the audio callback, the
  block is silenced, `EngineStatus::engine_errors` counts it, and a toast asks for a report;
  the panic message and backtrace are in the terminal (`RUST_BACKTRACE=1`). A plug-in putting
  out NaNs or runaway levels is contained and named in a toast (`bad_audio_warning`).
- **"System overload"**: the callbacks ran late for more than 60 ms in a row, so the engine shed
  load and the session stopped the transport (`EngineStatus::overloads`). The CPU meter in the control
  bar shows how close you are: `Tick.cpu` is the average, `cpu_peak` the worst recent block and
  `late_blocks` the blocks over 100%, a single one of which is a dropout but no overload
  (`BlockLoad` in `dsp/status.rs`). All of them are one thread's load, so they won't match a system
  monitor. `fx_bench --release` times the built-in effects; the audio
  thread runs with flush-to-zero, so denormals are not the cause.
- **Plug-ins**: `cargo run -p vst3-host --example probe -- <name>` loads and runs plug-ins
  outside the app. Settings › Plug-ins shows the last scan's report, including bundles that
  crashed or timed out. The cache is `plugins.json` in the config folder; delete it (or use
  Rescan) to force a new scan.
- **Settings and projects**: `settings.json` and `recent.json` are in `~/.config/marbble`
  (`$XDG_CONFIG_HOME/marbble`, `%APPDATA%\marbble` on Windows). A project is plain JSON in
  `project.json`; it is safe to read, and editing it by hand works if the app is closed.
- **Engine state**: everything the UI knows is in the `Snapshot` (`getState().snap` in
  `src/store.ts`) and the last `Tick` (`currentTick()`); a headless example is usually the
  quickest way to check what the engine does.
- **Never test with your real projects folder**: headless checks write into the scratch
  directory you pass; keep it away from `~/Music/Marbble`.

## Where things live

[ARCHITECTURE.md › Module map](ARCHITECTURE.md#15-module-map) lists every file. In short:

| You want to change | Look in |
|---|---|
| What is saved | `crates/engine/src/model/` (`Project`, `Track`, `Region`...), `src/api.ts` mirrors it |
| What an edit does | `crates/engine/src/session/`: one module per feature, all `impl Session` |
| What the UI can call | `src-tauri/src/commands/`, one module per area; shared wrappers in `commands.rs`; registered in `main.rs` |
| What plays, on the audio thread | `crates/engine/src/dsp/`: `process.rs` (the block), `strip.rs` (a strip), `apply.rs` (commands) |
| How state reaches the audio thread | `session/engine_sync.rs` and `session/undo.rs` send, `dsp/cmd.rs` defines, `dsp/apply.rs` receives |
| A built-in effect | `crates/engine/src/fx/`; contract in `fx/mod.rs` |
| A built-in instrument | `crates/engine/src/instrument/`; contract in `instrument/mod.rs`; parts in `synth_dsp/` |
| Hosting VST3s | `crates/vst3-host/src/`; `plugin/processor.rs` and `plugin/controller.rs` are the two halves |
| Pure logic with its own tests | Single files at the root of `crates/engine/src` (`takes.rs`, `tempo.rs`, `fades.rs`...), listed by theme in `lib.rs` |

A file that grows past about 1000 lines is a hint to split it by responsibility: types stay in the
parent module (`dsp.rs`, `session.rs`) where children can reach their private fields, and the code
moves into children that start with `use super::*`. Tests move to a `tests.rs` of their own.

## Conventions

These keep the code maintainable and the merges clean.

- **Real-time safety.** Nothing on the audio thread allocates, locks, blocks or does I/O, and
  nothing waits on another thread: use `rtrb` rings and atomics, not channels or mutexes. New
  engine state arrives as a `Cmd` and replaced objects leave as `Garbage`. See
  [ARCHITECTURE.md › Real-time rules](ARCHITECTURE.md#3-real-time-rules).
- **Model compatibility.** Every new field in `model/` or `prefs.rs` gets `#[serde(default)]`
  (or a struct-level `#[serde(default)]`), and optional fields usually also
  `skip_serializing_if = "Option::is_none"`, so older projects and settings files still load.
- **Struct literals.** Every `Region { .. }` literal needs `loop_end`, `takes`, `params` and
  `pattern`, every `RegionContent::Audio { .. }` needs `fade_shape`, every `RegionDsp::Audio`
  needs `laws`, and every `Note` needs `muted`. Tests and examples build these by hand, so
  `Default::default()` (or `None`) is usually what they want.
- **Clippy.** `cargo clippy --workspace --all-targets -- -D warnings` is clean on Linux, and CI
  keeps it so. It isn't run on Windows or macOS, where CI only compiles, so `cfg` code may have
  warnings of its own. A narrow `#[allow]` with a one-line reason is right where a lint
  would make DSP or tables worse. CI lints on a pinned toolchain (`LINT_TOOLCHAIN` in `ci.yml`) so a
  new Rust release can't turn it red with no code change; to move to a newer one, bump it there and
  fix what it reports in the same commit.
- **Every platform.** Keep platform-specific code behind `cfg`. VST3 enum casts stay explicit: the
  C enums are `i32` in the MSVC bindings, so a cast that clippy calls a no-op on Linux is needed on
  Windows.
- **Undo.** Every user edit goes through a checkpointed command (`edit`, `try_edit`,
  `edit_made`). Performance switches (mute, solo, on/off, arm, monitor) and transport are not undoable.
  Anything you send to the engine must also be handled in `restore()`, `build_track()` and
  `rebuild_graph()`.
- **Main thread for plug-ins.** VST3 controllers and editors are touched only on the main
  thread (synchronous commands, `idle`). Long work goes in an async command; hand edits back
  to the main thread with `run_on_main_thread` (see `bounce_in_place`, `freeze_tracks`).
- **Argument names.** Tauri maps camelCase JS argument names to snake_case Rust parameters.
  Struct fields keep their serde (snake_case) names.
- **Where things go.** Substantial new code goes into a new module (`session/<feature>.rs`,
  `src/<feature>.ts`, `src/components/<Feature>.tsx`) and is wired in with small edits. New
  struct fields go at the end of the struct, new commands at the end of the fitting `commands/<area>.rs` and of the
  `invoke_handler` list in `main.rs`, new CSS at the end of its area's file in
  `src/styles/` (a new area: a new file imported at the end of `src/styles/index.css`), new menu
  items next to related ones.
- **Comments.** Public items get doc comments. Comments explain *why*, not what. Keep the
  Logic Pro reference in mind: behaviour and labels follow Logic unless there is a reason not
  to, and the reason goes in a comment.
- **UI.** Use the existing CSS variables and components (`Form.tsx`, `Controls.tsx`,
  `Menu.tsx`). Follow the Logic-like visual language; don't introduce a new look.
  - **Colours** come from tokens: no hex colour in an area file (`styles.test.ts` fails on one).
    A colour that's the same in every theme (a piano key, the VU meter's face) still gets a
    token; see [Add a theme colour](#add-a-theme-colour).
  - **Type scale.** Six sizes in `tokens.css`: `--fs-xs` 10 px, `--fs-sm` 11, `--fs-md` 12 (the
    body), `--fs-lg` 13, `--fs-xl` 15, `--fs-xxl` 17. Text uses `--fs-sm` and up; `--fs-xs` is
    only for tiny numbers and captions inside fixed-size readouts (meter scales, the CPU figure,
    the LCD's captions). Display numerals sized to their box (the LCD's digits, the Tuner's note)
    set a px size of 18 or more. Canvas text (rulers, regions, notes) is sized in its draw code.
  - **Default states stay quiet.** Colour marks what differs from the default: Read is grey and
    Touch, Latch and Write are coloured; the power button is grey while the track plays and
    stands out when it's off.
  - **Keyboard.** `:focus-visible` draws `--focus-ring` (base.css); a click never shows one, and
    outside text fields the ring waits for Tab (`src/focusRing.ts`). Don't add a bare
    `outline: none`: a field with its own focus look (accent border, glow) may turn the outline
    off, and an editor focused only by a click (tabIndex -1) may too. Custom controls get a role
    and a name: knobs and faders use `sliderAria` and `sliderKeyDown` (`Controls.tsx`), which go
    through the same `onChange` and touch path as a drag; an icon-only button gets an
    `aria-label`, usually its tooltip.
- **Formatting.** `cargo fmt --all` for Rust, which CI checks. `rustfmt.toml` allows lines up to 160
  columns; `#[rustfmt::skip]` stays on tables that would otherwise fall apart. The one project-wide
  pass is listed in `.git-blame-ignore-revs`, so `git config blame.ignoreRevsFile .git-blame-ignore-revs`
  lets blame look through it. TypeScript is strict with no unused locals or parameters.

## The frontend: where things live

[ARCHITECTURE.md § The frontend](ARCHITECTURE.md#11-the-frontend) has how it works, and its
[source map](ARCHITECTURE.md#src-frontend) lists every file. In short:

| Where | What |
|---|---|
| `src/*.ts` | Feature modules, flat. A feature usually has a pure model (`samplerMap.ts`, `stepSeq.ts`, `regionGrab.ts`) and a module doing the store and Tauri work beside it. |
| `src/automation.ts`, `src/automation/` | Automation, in several files behind one set of imports. |
| `src/components/*.tsx` | One file for each pane, window or sheet. |
| `src/components/fx/` | Effect and instrument panels and their shared controls (`common.tsx`, `dynControls.tsx`). |
| `src/components/pianoRollEditor/`, `tracksCanvas/`, `audioTrackEditor/` | The parts of `PianoRoll.tsx`, `Arrange.tsx` and `AudioEditor.tsx`: drawing, pointer, keys and menus. |
| `src/components/settingsPanes/` | One file per Settings pane, and what they share in `parts.tsx`. |
| `src/styles/` | The stylesheet, one file per area. `index.css` imports them, and its order is the cascade. |
| `tests/*.test.ts` | One test per pure module, and the guard tests that read the sources. |

**A module with no screen in it stays testable.** Node loads it directly, so it can't import
Tauri or React at run time. Its own relative imports carry the extension (`from "./midiEdit.ts"`,
which Vite and TypeScript accept too), `import type` can stay as it is, and it can't use syntax
Node has to transform, such as a TypeScript `enum` (use a `const` object). The test imports it
from `../src/<module>.ts`. `tests/themes.test.ts` shows the way round for a module that imports
the store: it registers a resolve hook that adds the extensions, stubs `document`, and imports
`themes.ts` after.

**A canvas editor is a component and its parts.** `PianoRoll.tsx`, `Arrange.tsx` and
`AudioEditor.tsx` own the refs and the effects and lay out the panes; drawing and event handling
are functions in their folder that take those refs (`tracksPointer`, `noteGestures`, `editorDrawer`).
The rules that keep this safe:

- Hooks stay in the component, so hook order and dependency arrays can be read in one place. The
  parts hold no state and call no hooks, and being rebuilt on each render costs what the closures
  always did.
- Imperative code reads current values through refs (`live.current`), never from a closure over
  a render's values. A value it does take from the render, such as `showAutomation`, is passed
  in on each render, and an effect with `[]` must not capture one.
- A drag adds `mousemove` and `mouseup` listeners on `window` when it starts and removes them on
  release (`dragWith` in the Piano Roll), so it keeps working when the pointer leaves the canvas.

**Keep a folder's name different from its component's by more than case** (`settingsPanes/`
beside `Settings.tsx`, not `settings/`): Windows and macOS file systems ignore case.

**Flat modules stay where they are unless there's a reason.** `App.tsx` imports most of them,
and a move touches it, the tests, these docs and any branch in flight. A file that outgrows
itself can become a folder of the same name with the old path re-exporting it, as
`automation.ts` did, and importers don't change.

## Recipes

### Add a project field

Example: a per-project "Notes" field (it exists as `Project::notes`).

1. `crates/engine/src/model/project.rs`: add the field at the end of `Project` with
   `#[serde(default)]`, and initialise it in `Project::new`.
2. `src/api.ts`: add it to the `Project` interface.
3. Add a `Session` method that changes it and sets `self.dirty = true` (in the most fitting
   `session/` module, `options.rs` for project options).
4. Add an undoable command for it (next recipe).
5. If the engine needs the value, add a `Cmd` (recipe after next), send it from
   `sync_globals()` (for project-wide values) or `build_track()` (per track), and make sure
   `restore()` resends it when it differs.
6. Build anything that constructs a `Project` or the struct you changed by hand (tests,
   examples) and fix the literals.

### Add an undoable command end to end

1. **Session.** Write the edit as a method on `Session`. It changes `self.project`, sends what
   the engine needs (`push_regions`, `Cmd::Params` via `push_all_params`, `sync_tracks` for
   structural changes) and sets `self.dirty = true`. Return `Result` if it can fail.
2. **Command.** At the end of the fitting `src-tauri/src/commands/<area>.rs` (add a module in
   `commands.rs` if none fits, with `mod <area>; pub use <area>::*;`):

   ```rust
   /// Sets the project notes (Project Settings).
   #[tauri::command]
   pub fn set_project_notes(state: State<AppState>, notes: String) -> Snapshot {
       edit(&state, "Project Notes", Some("notes".into()), false, |s| s.set_notes(notes))
   }
   ```

   Use `try_edit` when the method returns `Result`, and `edit_made` when the UI needs the ids
   of what was created. The label is what Edit › Undo shows. Give a `key` when a drag or a
   typing burst should merge into one undo step. Pass `plugins = true` when the edit can remove
   plug-ins, so undo can bring their state back.
3. **Register** it at the end of the `invoke_handler` list in `src-tauri/src/main.rs`.
4. **Call** it from the UI: `act("set_project_notes", { notes })`, or
   `actMade("cmd", { ... })` to get the new ids. Argument names are camelCase in JS.
5. **Check undo**: make the edit, undo, redo, and check that the engine plays the right thing
   (a headless example is the best way). If the engine holds state for it, `restore()` must
   resend it.

### Add an engine `Cmd`

1. Add the variant at the end of `enum Cmd` in `crates/engine/src/dsp/cmd.rs`, with a doc comment.
   Prefer small `Copy` payloads; anything allocated is built by the session and moved in
   (`Box`, `Arc`).
2. Handle it in `Engine::apply` (`dsp/apply.rs`). If it replaces an allocated object, send the old one back
   with `self.discard(Garbage::...)` (add a `Garbage` variant if needed). Never drop
   allocations on the audio thread.
3. Use the new state in `process_block` (`dsp/process.rs`) or `run_strip` (`dsp/strip.rs`) without
   allocating. Preallocate buffers in `TrackDsp::new` (`dsp/track.rs`) or `Engine::new` (`dsp.rs`).
4. Send it from the session with `self.send(Cmd::...)`.
5. Make it survive everything that rebuilds or diffs the graph: `build_track()` (new tracks
   and project load) and `sync_globals()` (global values, called after load and undo) in
   `session/engine_sync.rs`, `restore()` (undo and redo) in `session/undo.rs`, and, if offline renders must save and restore it,
   `Engine::render` in `dsp/offline.rs`.
6. Add a unit test in `dsp/tests.rs` (see the `rig()` helper) or a headless example.

### Add a key command

1. `src/keymap.ts`: add an entry to `KEY_COMMANDS` with a unique `id`, the label shown in
   Settings › Key Commands, a group from `KEY_GROUPS` and the default keys (`"Mod+..."` for
   ⌘/Ctrl, `"Ctrl+..."` for the macOS Control key, then `Alt`, `Shift`). Check that the keys
   don't clash with `MENU_SHORTCUTS` or another command.
2. `src/App.tsx`: add the action to `KEY_ACTIONS` under the same id. If a menu item does the
   same thing, call `handleMenu("<menu id>")` so both stay in step.
3. Settings › Key Commands lists the new command automatically, and user overrides work
   through `settings.json` › `ui.keymap`. `npm test` (`tests/commands.test.ts`) fails if the id
   has no action or its default key is already taken.
4. If an editor (Piano Roll, Audio Track Editor) should handle the key differently while it
   has focus, handle it in that editor's own key listener and stop propagation there.

### Add a native menu item

1. `src-tauri/src/menu.rs` `spec()`: add `item("my-id", "My Command…")` next to related items
   (`check(…)` for one with a tick). Use an accelerator (`keyed("my-id", "My Command…",
   "Shift+CmdOrCtrl+X")`) only for ⌘ shortcuts that should work even in text fields; plain keys
   belong in the keymap so they can be changed, and the menu bar shows them by itself when the
   key command has the item's id.
2. `src/App.tsx`: add `case "my-id":` to `handleMenu`.
3. If you gave it an accelerator, add it to `MENU_SHORTCUTS` in `src/keymap.ts` so Settings
   › Key Commands shows it and refuses conflicting keys.
4. If the command should also run from a key that the user can change, add a key command
   (above) with the same id whose action calls `handleMenu("my-id")`.
5. Ticks and greyed-out states: if the Tracks area's menus have the command, give their item
   `id: "my-id"` and the name from `src/menuLabels.ts` (add it there); the menu bar follows that
   item. Otherwise add its condition to `ownStates` in `src/menuBar.ts`.
6. List it in the menu bar table in `docs/USER_GUIDE.md`. `tests/commands.test.ts` checks that
   `handleMenu` handles the id; `tests/menuLabels.test.ts` checks the names, the Edit menu's
   layout against the Tracks area's, and that no key is both an accelerator and a key command.

### Add a setting

Decide where it belongs:

- **Engine behaviour** (General, Recording, MIDI, Editing, latency): a field in the matching
  struct in `crates/engine/src/prefs.rs` (`GeneralPrefs`, `RecordingPrefs`, ...). The structs
  use `#[serde(default)]`; add the default in their `Default` impl.
- **Interface only**: `UiPrefs` in `src-tauri/src/settings.rs` (add the default to its
  `Default` impl).
- **Saved with the project**: a `Project` field instead (see above).

Then:

1. Mirror the field in `src/prefs.ts` (`Prefs` or `UiPrefs`).
2. Add the control to the right pane in `src/components/settingsPanes/`, using the `Form.tsx`
   controls, and change it with `setPrefs` or `setUi` from `settingsPanes/parts.tsx`, which go
   through `updateSettings`. That calls `set_settings`, which writes `settings.json` and calls
   `Session::set_prefs`.
3. Apply it in `Session::set_prefs` (`session/options.rs`) if the engine must react (send a
   `Cmd`, call `update_pdc`, ...), or read `self.prefs` where it is used.
4. Add a case to the tests in `prefs.rs` or `settings.rs` if parsing could be affected.

### Add a built-in effect

See [ARCHITECTURE.md › Built-in effects and presets](ARCHITECTURE.md#13-built-in-effects-and-presets).

1. `crates/engine/src/fx/<name>.rs` (or a folder, as `fx/reverb/` is, when it's big): a struct
   implementing `Effect`, allocating everything in its constructor; `PARAMS` (new ids only, never renumbered), meter names, `PRESETS`, and
   `pub fn info() -> EffectInfo` with the id `builtin:<name>` and a category.
2. List it in `fx::registry()` and `fx::factory_presets()` in `fx/mod.rs`.
3. Unit tests in the module: silence stays clean, the reported latency is the real one, the
   presets set real parameters within range, and it stays stable at extreme settings (see
   `limiter.rs` and `reverb/tests.rs`).
4. A panel, if the generic one isn't enough: `src/components/fx/<Name>.tsx`, built from
   `common.tsx`, registered in `FX_PANELS` in `src/fx.ts`. Call the panel props' `touch(id, on)`
   around every drag so Touch and Latch automation record it.
5. Run `fx_check` (it covers every registry entry) and
   `cargo run --release -q -p daw-engine --example fx_bench` to check the cost per block.

### Add a built-in instrument

The contract is at the top of `crates/engine/src/instrument/mod.rs`: the same real-time rules as an
effect, the same bits after a `reset`, parameter ids stable forever. `instrument/uno/` is a small
complete model and `tre/` a larger one.

1. `crates/engine/src/instrument/<name>/`: `mod.rs` with a struct implementing `Instrument` and
   `pub fn info() -> InstrumentInfo` with the id `builtin:<name>`; `params.rs` and `presets.rs`.
2. List it in `instrument::registry()` in `instrument/mod.rs`, with a `pub mod`. Nothing else is
   needed: menus, the panel (from `sections`), presets, automation, MIDI learn, freezing and saving
   all work from the registry entry.
3. `tests.rs`, built on `instrument/test_kit.rs`, for what `tre/tests.rs` measures and what's
   specific to the model. `tests/builtinInstruments.test.ts` keeps trademarks out of each model's
   panel and presets; add the model there, with the names to keep out.
4. To have the real-time checks cover it, add it to `MODELS` in
   `crates/engine/examples/common/all_features.rs`. Run `instrument_check`, `rt_alloc_check` and
   `parallel_check`, and `cargo run --release -q -p daw-engine --example instrument_bench` for the
   cost per block.

### Add a panel

There are four kinds, from least to most wiring. New CSS goes in `src/styles/` (see
[Conventions](#conventions)); use the theme variables.

**A sheet.** Build it on `Sheet` from `components/Dialogs.tsx`, which registers its keys with
`useSheetKeys` (`tests/sheetKeys.test.ts` fails on a sheet that doesn't). A flag or a `state.dialog`
value in `store.ts` shows it, and `App.tsx` renders it next to the others.

**A Settings pane.** A file in `components/settingsPanes/` exporting a component that takes
`{ close }`, ending in `Foot`, and an entry in `tabs` in `SettingsWindow` (`Settings.tsx`) with an
id added to `SettingsTab` in `store.ts`.

**A built-in effect's or instrument's panel.** `components/fx/<Name>.tsx` exporting a component
that takes `FxPanelProps`: `info` and `values` to read, `set` and `touch` to change a parameter,
`meters` to draw (start from `fx/Gain.tsx`; `fx/common.tsx` has the controls). Register it by plug-in
id in `FX_PANELS` (`src/fx.ts`). That is all: `fx/FxPanel.tsx` shows it docked, floating or in its
own window, with the header controls. Without an entry an effect gets the generic panel, and an
instrument the sectioned one from its spec.

**A pane in the bottom section.**

1. Add it to `BottomPane` in `store.ts`, and show it with `setState({ bottom: "<pane>" })`
   from a key command, a menu item or `EditorTabs`.
2. Render it in the layout in `App.tsx`, next to `s.bottom === "steps"`.
3. To let it detach, register it in `panelKinds.tsx` with `bottomPanel("<pane>", { title, render,
   size, own: ownBottom("<pane>") })`, and put `<DetachButton panel="<pane>" />` in its bar.

### Add a theme colour

For a CSS colour:

1. `src/themes.ts`: add `"--my-colour": "..."` to `studioCss`. Flat, Midnight, Light and High
   Contrast inherit it by spreading, so override it only where it must differ (at least check
   `lightCss` and `contrastCss`).
2. `src/styles/tokens.css`: the same value in the first `:root` block, for the first paint
   (`styles.test.ts` checks the two match).
3. Use `var(--my-colour)` in the area's file in `src/styles/` (new rules go at the end of it).
   A hex colour there fails `styles.test.ts`.

For a canvas colour:

1. `src/theme.ts`: add the field to `CanvasTheme` and a value to the default `theme` object.
2. `src/themes.ts`: give it a value in `studioCanvas`, and override it in the other canvas
   themes where needed (they spread `studioCanvas` or `flatCanvas`).
3. Draw with `theme.myColour`. Views redraw on a theme switch when they subscribe to
   `themeId`.

## Packaging

```sh
nix build .               # result/bin/marbble, result/share/applications/Marbble.desktop, icons
pnpm tauri build          # native bundles for the current platform (deb, rpm, AppImage, NSIS, dmg, ...)
```

- The Nix package (`nix/package.nix`) builds the frontend with npm in the sandbox, vendors the
  Rust crates from `Cargo.lock`, runs `cargo tauri build`, runs the engine unit tests
  (`cargo test -p daw-engine --lib`), and installs the result with a `marbble` symlink to the
  `daw` binary and a desktop entry in the audio categories.
- npm dependencies come straight from `package-lock.json` through nixpkgs' `importNpmLock`,
  so there's no npm hash to refresh. Keep `package-lock.json` in step with `package.json`
  (`npm install --package-lock-only` after changing dependencies with pnpm).

- The package source is a file set (Cargo files, `crates`, `src-tauri`, `src`, `index.html`,
  `LICENSE`, the npm and TypeScript configs), so editing docs doesn't trigger a rebuild.
- The bundles (and so the Nix package, in `lib/Marbble`) carry a copy of `LICENSE`, through
  `bundle.resources` in `src-tauri/tauri.conf.json`.
- GTK's file chooser needs the `org.gtk.Settings.FileChooser` GSettings schema at run time:
  `wrapGAppsHook3` puts GTK's schemas on `XDG_DATA_DIRS` in the Nix package's wrapper, the
  AppImage gets them from linuxdeploy's GTK plug-in, and the .deb and .rpm from their GTK
  dependency. A new packaging route must carry them too (ARCHITECTURE.md › File dialogs).
- `nix/package.nix` has its own `version` string; `scripts/set-version.sh` keeps it in step
  (see below).

## Versioning and releases

Marbble follows [semantic versioning](https://semver.org). Prereleases carry a suffix
(`0.1.0-alpha.1`, `0.1.0-alpha.2`, ...), and every release is tagged `v<version>`.
[CHANGELOG.md](../CHANGELOG.md) records each release in the Keep a Changelog format.

The version is recorded in the root `Cargo.toml` (`[workspace.package] version`, inherited by
every crate) and `Cargo.lock`, `package.json` and `package-lock.json`,
`src-tauri/tauri.conf.json`, and `nix/package.nix`. `scripts/set-version.sh <version>` rewrites
all of them (the three workspace crates in `Cargo.lock`), refuses anything that isn't a
semver version, and prints the result. It needs GNU sed: on macOS install it (`brew install
gnu-sed`) and put `gsed` first in `PATH` as `sed`.

To make a release:

1. Move the `[Unreleased]` entries in `CHANGELOG.md` under a new `## [<version>] - <date>`
   heading. The release notes are taken from that section.
2. `scripts/set-version.sh <version>`.
3. Commit, for example `git commit -am "Release <version>"`.
4. Tag and push the tag: `git tag v<version> && git push origin main v<version>`.

Pushing a `v*` tag runs `.github/workflows/release.yml`:

1. `create-release` makes a draft GitHub release named "Marbble v<version>", with the version's
   CHANGELOG section plus download and unsigned-build notes. A version with a `-` suffix is
   marked as a prerelease.
2. `build` runs `tauri-action` on five runners and uploads the bundles to the draft:
   Linux x86_64 (`ubuntu-24.04`) and aarch64 (`ubuntu-24.04-arm`): `.deb` and `.AppImage`,
   plus `.rpm` for versions without a prerelease suffix (RPM versions can't contain `-`); Windows (`windows-latest`): the NSIS installer (`_x64-setup.exe`); macOS
   (`macos-latest`): `.app` and `.dmg` for `aarch64-apple-darwin` and `x86_64-apple-darwin`.
3. `publish` takes the release out of draft once every build has finished, then moves the
   release channels (below).

### Release channels

There are two, and the version decides which a release is on:

- **Stable:** versions without a suffix (`1.2.3`). GitHub marks the newest as the Latest release.
- **Unstable:** every release, prereleases (`1.2.3-alpha.1`, `-beta.1`, `-rc.1`) included, so
  Unstable is never behind Stable.

Each channel is also a branch, `stable` and `unstable`, that `publish` moves (force-pushes) to the
release's commit when the release is the newest on that channel by version; a patch to an older
line leaves them alone. Nix users pin a flake input to `github:JackMechem/marbble/stable` or
`/unstable`. Nothing is committed to those branches by hand; `stable` doesn't exist until the
first stable release. The release notes start with the channel, and the website's Download
section has a Stable/Unstable switch.

To try a change to the workflow without releasing anything, push it to the `release-check`
branch: the same builds run, and nothing is created or published. The macOS bundles are
ad-hoc signed (`"signingIdentity": "-"` in `tauri.conf.json`), with the hardened runtime and
`Entitlements.plist`, and a step checks the signature: Apple Silicon calls an arm64 app whose
bundle isn't sealed "damaged" and won't open it, though the linker signs the binary itself. No
build is signed with a developer certificate or notarized yet.

`.github/workflows/ci.yml` runs on pushes to `main`, on pull requests and on demand, on
`ubuntu-24.04`, `windows-latest` and `macos-latest`: `npm ci`, `npm run build` (type check and
frontend build), `npm test`, `cargo test -p daw-engine --lib`, `cargo test -p daw` and
`cargo check --workspace --all-targets` (so the examples must compile). On Linux it then checks
`cargo fmt` and runs clippy with `-D warnings`, both on the pinned `LINT_TOOLCHAIN`, and runs
`scripts/headless-checks.sh`: the headless checks that need no third-party plug-ins, on ALSA's
`null` device (the runner has no sound card or audio server) with a generated test tone. A
separate `nix` job runs `nix flake check`, which builds the Nix package. `pdc_check`,
`freeze_check` and the plug-in parts of the other checks need real plug-ins, so they don't run
in CI; run them locally.

The main branch is `main` on <https://github.com/JackMechem/marbble>.
