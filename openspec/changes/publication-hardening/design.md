# Design: Publication hardening

See proposal.md and the spec deltas. This document fixes the rename
mechanics, the facade's surface, the safety attribute placement, the
workflow's shape, and the fuzz targets.

## Context

- Eight crates under private-phase names; every manifest says the
  published names carry the `efterscript-` prefix; Rust paths import
  them in about 150 source files. The session library's C interface
  is consumed by the emulator bridge as `libplaten.a` + `platen.h`.
- `codec` alone forbids unsafe code; the other library crates contain
  none but do not say so; the session library's `ffi.rs` holds the 38
  uses the C boundary needs.
- One libFuzzer target exists (`ps-vm/fuzz`, the scanner), outside the
  workspace so stable builds stay clean; `cargo xtask fuzz-round` is
  the generator-based round, not libFuzzer.
- No CI exists; the gates are the `xtask` commands and cargo. The
  strings lint needs the vault; the oracle needs the converter.
- The charter reserves the `efterscript` GitHub organisation and wants
  a genuine `efterscript` seed crate.

## Goals / Non-Goals

**Goals:** a publishable crate family with one facade; behaviour and
goldens untouched; safety enforced by the compiler; the public tier
green on every push; parsers fuzzed.

**Non-Goals:** publishing (a later deliberate step); the README and
other documents; the emulator bridge (its link line already uses the
unchanged library name); nightly-only CI steps.

## Decisions

**D1. Rename in one commit, directories included.** Package names,
directory names, the workspace `members` and `[workspace.dependencies]`
table, and every `use`/`extern` path change together, mechanically
(`ps_vm::` → `efterscript_vm::` etc.), with `lib.name` set only where
a file name must survive: `efterscript-platen` keeps `name = "platen"`
for its library so the static library and its symbols are unchanged.
Verified by the full gate chain and byte-identical goldens.
*Alternative:* keep directories under the old names — a permanent
mismatch between path and package name for no benefit.

**D2. The facade is thin.** `crates/efterscript` depends on
`efterscript-remelt` and `efterscript-platen` and re-exports their
public surfaces under two modules (`distill` and `session`) plus the
top-level entry points (`distill`, `distill_into`, `Options`, `Params`,
`Report`, `Job`, `JobConfig`); it defines nothing of its own beyond
docs. The CLI depends on the facade. *Alternative:* a facade with its
own API — premature; the entry points are already the API.

**D3. Safety attribute placement.** `#![forbid(unsafe_code)]` in
`efterscript-vm`, `-graphics`, `-fonts`, `-pdf`, `-remelt`, `-codec`
(already), and the CLI; `efterscript-platen` gets `#![deny(unsafe_code)]`
at the crate level with `#![allow(unsafe_code)]` on the `ffi` module
only, so unsafe code cannot appear elsewhere in it.

**D4. The workflow.** One GitHub Actions workflow with a matrix of one
Linux job on the pinned stable toolchain (`rust-version` from the
workspace), caching cargo, running the exact gate commands the
project already uses; the WebAssembly check adds the
`wasm32-unknown-emscripten` target through `rustup` (no Emscripten
needed for a check). The lint and oracle steps are listed in a comment
as private-only. *Alternative:* a nightly fuzz job — added later when
the fuzz targets prove stable.

**D5. Fuzz targets.** A `fuzz/` crate per library that has a parser
(`codec`: inflate, lzw, predictor; `vm`: scanner (moved), filter chain
over bytes, JPEG marker walker, and a whole-program distillation
target that runs the VM with a mock backend; `fonts`: Type 1, CFF,
TrueType), each outside the workspace like today, each seeded by a
tiny corpus copied from `corpus/unit` at first run; `cargo xtask
fuzz-smoke` builds and runs each target for a bounded time (`-max_total_
time=30`) when `cargo fuzz` and a nightly toolchain are found, else
skips with a message. Panics found become corpus files and fixes.

**D6. Metadata.** Each manifest gains `description`, `keywords`,
`categories`, `readme`, and the family `repository` URL; `publish =
false` stays until the registry checks are done, with the flip
recorded as its own commit.

## Risks / Trade-offs

- [A missed path in the rename] → the workspace does not compile;
  the gates catch it.
- [The rename churns every open branch] → only the emulator branch is
  open and it does not touch repo A.
- [Fuzz targets need nightly] → gated behind availability; CI does
  not run them yet.
- [A first fuzz round finds real crashes] → that is the point; each
  becomes a corpus file with a fix before the change archives, or a
  recorded limit with its trigger.

## Open Questions

- None that change the specs or tasks.
