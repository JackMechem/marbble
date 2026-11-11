# Security policy

## Supported versions

Marbble is in alpha, and only the latest prerelease on the
[Releases page](https://github.com/JackMechem/marbble/releases) gets fixes. A fix lands on `main`
and goes out in the next prerelease; older releases aren't patched.

## Reporting a vulnerability

Report it privately through GitHub:
[open a security advisory](https://github.com/JackMechem/marbble/security/advisories/new). Only
the maintainer sees it, and you can discuss it there until a fix is out. Please don't open a public
issue or discussion for it.

Say which version and platform you found it on, what an attacker could do, and the steps or file
that show it. You should hear back within a few days.

## What counts

- **Malicious project files.** Opening a `.marbble` project, or importing an audio, MIDI or SFZ
  file, that runs code, writes or deletes files outside the folders it should, or crashes Marbble
  in a way that looks exploitable.
- **Plug-in loading.** Marbble loading a plug-in from somewhere the user didn't choose or expect,
  for example because a project names a path. A VST3 plug-in runs with your rights by design, so
  a plug-in you installed yourself behaving badly isn't a Marbble vulnerability.
- **The update check.** Anything that could make it point you to a release that isn't from this
  repository. It only asks GitHub for the list of releases and opens a release page; it never
  downloads or installs anything.
- **The release builds** themselves, if what's published differs from what the release workflow
  built.

A bug that loses work or crashes on an ordinary project is serious, but it isn't a security issue:
report it as a [bug](https://github.com/JackMechem/marbble/issues/new/choose).
