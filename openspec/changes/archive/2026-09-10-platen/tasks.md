# Tasks: platen

## 1. Interpreter

- [x] 1.1 `languagelevel`; corpus scenario; verified by `difftest run`

## 2. Job API

- [x] 2.1 `JobConfig`, `Job::new`, `feed` with run/resume and drained captures, `finish` with the sink, `Outcome`, `Finished`; verified by the query, page, error, and no-persistence scenarios as Rust tests, plus a proptest splitting random corpus programs at random points and comparing with unsplit runs

## 3. C ABI and builds

- [x] 3.1 `src/ffi.rs` with the header's functions, `catch_unwind` at every entry, poisoning, `platen_last_error`; `platen.h`; verified by the ABI scenario from a Rust test calling the extern functions
- [x] 3.2 Crate types, the Emscripten target check in CI, `docs/embedding.md`, the no-threads/fs/time grep test; verified by `cargo check -p platen --target wasm32-unknown-emscripten` locally after `rustup target add`

## 4. Verification

- [x] 4.1 `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `fuzz-round`, `lint-strings`, `openspec validate platen`; design.md gains "## Implementation notes"; the captured driver job run through `Job` in pieces in the private tier, with the host prelude, reported
