# Tasks: publication-hardening

## 1. Rename and facade

- [x] 1.1 Rename the seven library crates and their directories, the workspace tables, and every Rust path; `efterscript-platen` keeps `lib.name = "platen"`; `xtask`/`difftest` references updated; verified by the full gate chain and every golden byte-identical, and by rebuilding the emulator bridge's link line unchanged (`libplaten.a`, `platen.h`)
- [x] 1.2 The `efterscript` facade crate re-exporting the distillation and session surfaces; the CLI depends on it; verified by a facade test distilling a corpus file to its golden and the two workspace scenarios

## 2. Safety and metadata

- [x] 2.1 `forbid(unsafe_code)` on every library crate and the CLI, `deny` + module-level `allow` on the session library's `ffi`; verified by a compile-fail test (`trybuild` is not allowed — use a `#[cfg(test)]` doc test or a build script check; decide and record) or by the documented attribute plus clippy
- [x] 2.2 Publish metadata in every manifest with `publish = false` retained; verified by `cargo package --list --allow-dirty` succeeding for each crate

## 3. Continuous integration

- [x] 3.1 `.github/workflows/ci.yml` running the public tier on the pinned toolchain with the private tiers excluded and explained; verified by running the same steps locally with `act` if available, otherwise by a dry review and the first push

## 4. Fuzzing

- [x] 4.1 Fuzz crates and targets per D5 with corpus seeds; `cargo xtask fuzz-smoke`; verified by each target building under `cargo fuzz build` when available, a bounded run with no crash, and the skip path without the toolchain

## 5. Verification

- [x] 5.1 `cargo test --workspace`, clippy, fmt, `difftest run` (goldens byte-identical), `parse-survival`, `fuzz-round`, `lint-strings`, `check-wasm`, `openspec validate publication-hardening`; the captured driver jobs still pass; the emulator bridge builds against the renamed library; design.md gains "## Implementation notes"
