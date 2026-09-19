# emscripten-link-flags

## Why

The first release with the artefact job built both archives but failed
the link check: the linker reported `__cpp_exception` undefined in the
Emscripten archive. Rust compiles the Emscripten target with
WebAssembly exception handling on, so any final link that includes the
archive must enable it; the job's link line and the embedding guide's
example did not.

## What changes

- The link check passes `-fwasm-exceptions` (legacy form, stated
  explicitly).
- The embedding guide's link example and the archive rule say the same,
  since every consumer's link line needs it.
- The platen spec's Emscripten requirement names the flag beside the
  version.

## Non-goals

Rebuilding the standard library, or changing how the archive is built.
