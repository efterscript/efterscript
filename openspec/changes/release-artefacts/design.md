# Design: Prebuilt session-library archives on every release

See proposal.md and the spec deltas.

## Context

- `release.yml` has `verify` and `publish` jobs; `publish` runs in the
  `release` environment after approval and uploads the crates.
- The session library builds as `libplaten.a` (`crate-type =
  ["staticlib", "rlib"]`, `lib.name = "platen"`) with `platen.h` in
  `crates/efterscript-platen/include/`. `cargo xtask check-wasm` only
  type-checks the Emscripten target; a real archive needs `emcc`.
- Rust's Emscripten target ships a prebuilt standard library built
  against one Emscripten SDK version; the Rust target documentation
  says a local Emscripten should match it and that Emscripten's ABI
  is not semantically versioned. The emulator pins its own SDK
  (4.0.10 in its devcontainer image).

## Goals / Non-Goals

**Goals:** a host links a released version without Rust; the
Emscripten archive is usable by the emulator's pinned toolchain; the
pin is explicit and tested at release time.

**Non-Goals:** more host targets now; a package registry for the
archives; building the standard library from source.

## Decisions

**D1. A third job, `artefacts`, after `verify`, independent of
`publish`.** It needs no registry token and no environment approval
(the archives are built from the same tagged commit `verify` passed),
so it runs in parallel with the approval wait. It uploads to the
GitHub release for the tag (created if absent) with `contents: write`;
the release notes are the tag message. *Alternative:* inside
`publish` — would make the crates wait on an Emscripten build and mix
two credentials in one job.

**D2. Two targets, three files each plus the header and checksums.**
Host: `cargo build --release -p efterscript-platen` on
`ubuntu-latest` → `libplaten-<version>-x86_64-unknown-linux-gnu.a`.
Emscripten: `emsdk` installed at the pinned version, `cargo build
--release -p efterscript-platen --target wasm32-unknown-emscripten` →
`libplaten-<version>-wasm32-unknown-emscripten-<emsdk>.a`. Plus
`platen-<version>.h` and `SHA256SUMS`. Version = the tag's.

**D3. The pin is one workflow variable, `EMSDK_VERSION`.** Its value is
the Emscripten version Rust's prebuilt Emscripten standard library was
built with for the pinned Rust (recorded in the notes with its
source); the emulator matches it. Changing either the Rust pin or the
SDK pin is a commit that states the pairing.

**D4. The link check is the release gate for the Emscripten
archive.** The job compiles a ten-line C program that includes
`platen.h`, creates a job, feeds a one-line program, finishes, reads
the page count, and frees; links it with the same `emcc` against the
archive with `-s ALLOW_MEMORY_GROWTH=1` (the emulator's setting);
runs it under `node` (Emscripten's default output); expects `pages=1`.
A failure fails the job before upload.

**D5. Documentation.** The embedding guide gains a "Prebuilt archives"
section: the file names, the checksum, the Emscripten rule, and the
fetch example.

## Risks / Trade-offs

- [The SDK pin drifts from Rust's] → the link check catches ABI
  breakage at release time; the notes record the pairing.
- [`emsdk` install time] → cached by the SDK's own cache action or a
  cache key on the version; a few minutes at worst.
- [The archive is large] → the host archive is about 40 MB unstripped;
  build with `strip = "debuginfo"` for the release profile of this
  job (release profile setting in the workflow, not the workspace),
  recorded.

## Implementation notes

- **As built.** A third job `artefacts` in `.github/workflows/release.yml`,
  after `verify`, parallel to `publish`, with `contents: write` only.
  It re-checks the tag against the workspace version, installs the
  pinned Rust with the Emscripten target and the pinned SDK
  (`mymindstorm/setup-emsdk@v14`, cached), builds the host and
  Emscripten archives, links a ten-line C host against the Emscripten
  archive with the same `emcc` and `-s ALLOW_MEMORY_GROWTH=1`, runs it
  under `node`, and requires `pages=1`; then assembles `dist/` (two
  archives, the header, `SHA256SUMS`) and attaches it to the tag's
  release with `softprops/action-gh-release@v2`.
- **The pin pair and its source.** Rust's CI does not pin an Emscripten
  version: `src/ci/docker/host-x86_64/dist-various-1/install-emscripten.sh`
  at tag 1.98.0 runs `emsdk install latest`, so the prebuilt standard
  library carries whatever was latest on the toolchain's build date.
  Rust 1.98.0 was built 2026-08-18; the emsdk release tags as of that
  date named 6.0.7 as latest (the 1.98.1 point release, built
  2026-09-01, would pair with 6.0.9). The job therefore pins
  `RUST_VERSION: 1.98.0` and `EMSDK_VERSION: 6.0.7`, an exact pair; the
  workspace `rust-version = "1.98"` is a minimum and stays. The Rust
  target documentation's advice to match versions is the basis (the
  alternative, rebuilding the standard library with `-Zbuild-std`, needs
  nightly and was not taken).
- **Consequence for the emulator.** Its devcontainer pins Emscripten
  4.0.10; a 6.0.7 archive is not expected to link with it. Adopting the
  artefact means the emulator bumps its SDK to 6.0.7 in step with this
  pair, which the artefact's file name makes explicit. Recorded here
  and relayed to the emulator branch.
- **Checked here.** The host archive builds in release profile (40.6 MB
  unstripped) and the check program links natively against it and
  prints `pages=1`; the workflow parses with three jobs; no denylisted
  string. Not verifiable here: the Emscripten build and link (no `emcc`)
  and the upload; the first tag after this change is the proof.
- **Deviation from D2.** The archive is not stripped in this version:
  `strip` for a static archive would need a profile override; deferred
  with the size recorded.
