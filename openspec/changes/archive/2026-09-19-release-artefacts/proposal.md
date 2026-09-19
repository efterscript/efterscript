# Change: Prebuilt session-library archives on every release

## Why

The session library is the way other programs embed EfterScript as a
printer: a static library and a C header, linked by a host such as the
paired emulator. Today a host must build that library itself from a
sibling checkout with a Rust toolchain, and for the browser it must
also have the Emscripten toolchain that Rust's target compiles through.
Neither of the two development containers in the pairing has both, so
the browser build of the bridge has never been linked. Making the
library a release artefact removes the requirement: each tagged
release attaches the archive and header for the host and for the
Emscripten target, built on the pinned toolchains, and a host fetches
a version by URL and checksum. The Emscripten archive is only usable by
a program linked with a compatible Emscripten, so the artefact carries
that version in its name and the two projects bump it together. This
belongs in the release workflow now, before a second version exists,
so 0.0.2 is the first release a host can consume without Rust.

## What Changes

- **A release job builds the session library** for two targets on the
  tagged commit: the Linux host (`x86_64-unknown-linux-gnu`) and
  `wasm32-unknown-emscripten` with a pinned Emscripten SDK, and
  uploads to the GitHub release of the tag: `libplaten.a` for each
  target named by target (and Emscripten version), `platen.h`, and a
  checksum file; the Emscripten build is verified by linking a minimal
  C program against the archive with the same `emcc`.
- **The pin is one declared value** in the workflow, matching the
  Emscripten version Rust's prebuilt standard library for that target
  was built with; changing it is a deliberate commit.
- **The embedding guide** documents fetching a release's archive and
  the version-matching rule for Emscripten hosts.
- Out of scope: other host targets (added on request), publishing the
  archives anywhere but the GitHub release, and the emulator's fetch
  step (its own repository).

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `publication`: ADDED requirement for the release artefacts.
- `platen`: ADDED requirement for the Emscripten version constraint the
  artefact carries.

## Impact

- Code: `.github/workflows/release.yml` (a new job), `crates/
  efterscript-platen/docs/embedding.md`; no crate changes.
- No new dependencies.
- Depends on `release-workflow` (archived).
