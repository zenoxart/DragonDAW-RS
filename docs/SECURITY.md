# Security model

Dragon DAW opens files from other people (projects, presets, samples, MIDI, video) and loads plugins,
which are native code. This is what protects you.

## Projects can't load code or reach the network on their own

A `.dragon` project names files and plugins by path. On open:

- **Plugins** load only from your scanned plugins, the platform's standard plugin folders, or paths you
  allowed. A project made on another computer loads the same plugin (by its id) from where it's installed
  here. Anything else stays off, and Dragon DAW asks: *Keep them off* / *Allow and load them*.
  Network paths and paths that climb out of a plugin folder (`..`) are never loaded as is.
- **Network locations** (`\\server\share\…`) named by the project are blocked before anything touches
  them: on Windows, opening such a path signs you in to that server and hands it your hashed Windows
  credentials. The share the project itself is on stays allowed; others need your OK (remembered).
- **Saving** collects only media files (audio, video, MIDI) into the project's `Samples` folder: a project
  that names another file (a key, a document) can't get it copied into a folder you might share.

## Downloads are checked before they run

- **Updates** come only from this repository's releases, with a plain file name, and must match the
  SHA-256 that GitHub publishes for the file; otherwise they are deleted and not installed. The installer
  script gets a unique name each time. Release builds also carry signed build-provenance attestations
  (`gh attestation verify <file> --repo zenoxart/DragonDAW-RS`).
- **ffmpeg** (Video › Get ffmpeg) must match the checksum its publisher lists, or it is not installed.
- On Windows, `curl`, `tar` and `powershell` are run from System32, never a same-named program found first.

## Build and release

CI and release workflows run with read-only permissions by default, pin every action to a commit SHA,
publish only builds of pushes to `main` in this repository, and verify the bundled ffmpeg.
`cargo audit` is clean of known vulnerabilities in code reachable from files you open; remaining
advisories are "unmaintained" notices and a Linux-only accessibility dependency.

## Reporting

Please report security problems privately via GitHub's *Report a vulnerability* on the repository.
