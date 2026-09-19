# Design: emscripten-link-flags

## D1: Enable at the link, not in the archive

The Rust target documentation (1.98.0, "wasm32-unknown-emscripten")
states that code for the target enables `-fwasm-exceptions` (legacy
WebAssembly exceptions) unless compiled with `panic=abort`, and that
the standard library is prebuilt that way. Compiling the crate with
`panic=abort` would not change the prebuilt standard library's objects,
so the flag belongs on the consumer's link line, where Emscripten's
runtime support is selected. C objects need no change.

## D2: Say the form

`-sWASM_LEGACY_EXCEPTIONS=1` is passed beside the flag so the link
names the legacy form the Rust objects use, whatever the SDK's default
in a later version.

## D3: Where it is written

The workflow's link check, the embedding guide's example and archive
rule, and the platen requirement.

## Implementation notes

- **Evidence.** The failed link on the first 0.0.2 run: five
  `undefined symbol: __cpp_exception` reports from one object of the
  archive, in a link whose flags show no exception support. Both
  archives had built, so the Rust 1.98.0 and Emscripten 6.0.7 pairing
  holds at least through compilation.
- **Checked here.** The workflow parses with three jobs and nine
  artefact steps; the string lint passes. The link itself cannot be run
  in this container (no `emcc`); the next tagged run is the proof.
