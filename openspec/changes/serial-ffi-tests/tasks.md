# Tasks: serial-ffi-tests

## 1. The integration tests (efterscript-platen)

- [ ] 1.1 A file-wide `SERIAL` mutex in `crates/efterscript-platen/tests/ffi.rs`, with a `serial()` guard that recovers a poisoned lock, held for the whole body of every test in the file; the comment states why (the one unguarded last-error buffer); no test body or assertion changes; verified by `cargo test -p efterscript-platen --test ffi` passing, and by a test that fails without the guard: a stress run of the two racing tests in a loop under parallel threads (run locally, not committed) reproduces the wrong message before the change and never after

## 2. Gates

- [ ] 2.1 `cargo test --workspace`, clippy, fmt, `openspec validate serial-ffi-tests`; the reproduction and its result recorded in tasks.md below this task
