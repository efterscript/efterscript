# Change: Publication hardening — crate family, seed crate, enforced safety, CI, fuzzing

## Why

The interpreter has reached the planned v1 language and colour
surface, and the joint milestone with the emulator is one integration
step away, but the repository is still shaped for its private phase:
every crate carries a placeholder name and `publish = false`, no
continuous integration exists, the safety property that only the C
boundary may contain unsafe code is a fact rather than an enforced
one, and fuzzing covers the scanner alone. Each of these is cheap to
fix now and expensive to fix after publication: crate names are
permanent on the registry, a first release without CI has no trust
signal, and a safety claim without an enforcing attribute is a claim
a reviewer must re-verify. This change makes the repository
publishable as a family of crates under one prefix with a facade
users depend on, with the verification pipeline running on every
push, and with the memory-safety boundary enforced by the compiler.
The README, contributing guide, and other project documents are
maintained outside this process and are not part of the change.

## What Changes

- **Crate family under the `efterscript-` prefix**: `ps-vm` →
  `efterscript-vm`, `ps-graphics` → `efterscript-graphics`, `ps-fonts`
  → `efterscript-fonts`, `pdf-out` → `efterscript-pdf`, `codec` →
  `efterscript-codec`, `remelt` → `efterscript-remelt`, `platen` →
  `efterscript-platen`; `efterscript-cli` unchanged with its
  `efterscript` binary. Directories follow the names. The session
  library's C interface is unchanged: the static library stays
  `libplaten.a`, the header `platen.h`, the symbols `platen_*`, so
  embedders and the emulator bridge link exactly as before.
- **The `efterscript` facade crate**: a new crate at `crates/
  efterscript` that re-exports the distillation entry points (options,
  parameters, the report, the distillation calls) and the session
  job API, so a user depends on one crate; the command-line tool
  depends on it. It is the genuine seed the project wants on the
  registry, carrying the whole engine.
- **Enforced safety boundary**: `#![forbid(unsafe_code)]` on every
  library crate except the session library, whose C boundary is the
  one permitted site, documented as such; the audit result becomes a
  compile-time property.
- **Continuous integration**: a workflow running the public tier on
  every push and pull request — tests, clippy, formatting, the corpus
  run with byte-identical goldens, parse survival, the generator round,
  the no-default-features build, and the WebAssembly check — on the
  pinned toolchain; the strings lint and the oracle tier stay private
  by design and the workflow says so.
- **Fuzz targets**: libFuzzer targets beside the scanner's for the
  codec crate (inflate, LZW, predictors), the filter chain, the JPEG
  marker walker, the three font parsers, and a distillation round
  trip from program text to PDF, each seeded from the corpus, run by
  `cargo xtask fuzz-smoke` for a bounded time on the pinned nightly
  when available and skipped with a message otherwise.
- **Publish metadata**: descriptions, keywords, categories, and the
  repository URL of the organisation the project reserves, with
  `publish = false` retained until the planned registry checks
  are done — flipping the flag is a separate, deliberate step.
- Out of scope: the README and other project documents (maintained
  directly); the history question (decided: unchanged); publishing
  itself; the clone-detection scan; quirks-mode policy.

## Capabilities

### New Capabilities
- `publication`: the crate family and its naming, the facade crate's
  surface, the enforced safety boundary, and the fuzz targets.

### Modified Capabilities
- `oracle-testing`: ADDED requirement for the public tier in
  continuous integration and the private tiers' exclusion.
- `platen`: ADDED requirement that the C interface's library name,
  header, and symbols are stable across the crate rename.

## Impact

- Code: every `Cargo.toml` and the workspace dependency table; every
  `use` path across the workspace (`ps_vm::` → `efterscript_vm::` and
  the rest); `xtask` and `difftest` references to crate names; the new
  `crates/efterscript` facade; `lib.rs` attributes; `crates/*/fuzz`
  targets; `.github/workflows/`; the platen embedding guide's crate
  references. Goldens must stay byte-identical: nothing about
  behaviour changes.
- No new dependencies in library crates; `libfuzzer-sys` only in the
  fuzz crates, outside the workspace as today.
- Depends on every archived change; blocks publishing.
