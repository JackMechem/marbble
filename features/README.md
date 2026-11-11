# Features

Every feature of Logic Pro 12.3, one file per area, with how far Marbble has it. They're the
source of [docs/FEATURES.md](../docs/FEATURES.md) and of the beta progress in
[ROADMAP.md](../ROADMAP.md), which `python3 scripts/features.py` generates; CI fails when those are
out of date.

A pull request that builds or extends a feature changes its entry here, and runs the script.

## A file

```toml
title = "Mixing and automation"    # the area's name
about = "The Mixer, automation…"   # one sentence on what it covers
order = 9                          # its place on the page

[[feature]]
section = "Mixer"                  # the Logic Pro section it belongs to, within the area
name = "Reorder strips"
status = "partial"                 # done, partial or missing
tier = "core"                      # see below
beta = true                        # beta needs it; leave it out otherwise
logic = "559"                      # its page in the Logic Pro User Guide (12.3)
issue = 80                         # the issue that tracks it, if there is one
note = "Follows the track order."  # for partial: what Marbble has, and what it lacks
```

`done` means a user can do what Logic's feature does, in its essentials; `partial` means Marbble
has a simpler version, and the note says how. Only `section`, `name`, `status` and `tier` are
required.

## Tiers

The tier says how central a feature is, from the order the scope was planned in:

| Tier | What's in it |
|---|---|
| `core` | What everyday recording, editing and mixing needs. Most of it is `beta = true`. |
| `pro` | Professional editing and mixing: advanced editing, automation, comping, professional bounces. |
| `advanced` | The larger production features: Flex, Smart Tempo, Step Sequencer, Live Loops, chords. |
| `studio` | Studio integration: surround and Atmos, sync, video, external hardware, control surfaces. |
| `content` | Bundled instruments, effects, loops and machine learning. |
| `platform` | Plug-in formats and operating systems, accessibility, translations, help. |
