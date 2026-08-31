# Change: PDF writer (pdf-out)

## Why

The distillation backend needs a way to write PDF files. The build-vs-reuse
question left open at scaffolding time is now decided: **build our own**, so
the product's core path has zero external dependencies, compiles anywhere
Rust does (including WASM), and every byte of output is under this project's
control. The file syntax is fully specified by ISO 32000-1 §7, which the
project holds; the writer is a bounded, spec-driven component whose
correctness is testable byte-for-byte. Deciding this before the graphics
layer exists means the IR-to-PDF change can target a stable, owned API.

## What changes

- `pdf-out` becomes a real crate: object serialization (all nine object
  types), streams, classic cross-reference tables and trailer, and a small
  document-structure layer (catalog, page tree) built only on the
  primitives.
- Deterministic by construction: one canonical serialization for every
  value, fixed object-numbering discipline, byte-identical output across
  platforms and runs.
- Zero runtime dependencies. Flate output is a hand-written zlib container
  using stored (uncompressed) blocks — valid, deterministic, dependency-free;
  real DEFLATE compression is a recorded follow-up.
- Target: PDF 1.7 (ISO 32000-1), classic xref tables, no encryption, no
  incremental update (both recorded as out of scope with triggers).

## Impact

- New capability spec: `pdf-out`.
- Code: `crates/pdf-out` (currently an empty scaffold; its doc comment's
  "decision open" note is resolved by this change).
- Small, uncompressed golden PDFs land in `corpus/golden/pdf/` — they are
  ASCII-dominant and diffable, consistent with the text-based golden policy.
- No other crate changes; `remelt` and the graphics change consume this API
  later.
