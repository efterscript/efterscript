# Tasks: serial-ffi-tests

## 1. The integration tests (efterscript-platen)

- [x] 1.1 A file-wide `SERIAL` mutex in `crates/efterscript-platen/tests/ffi.rs`, with a `serial()` guard that recovers a poisoned lock, held for the whole body of every test in the file; the comment states why (the one unguarded last-error buffer); no test body or assertion changes; verified by `cargo test -p efterscript-platen --test ffi` passing, and by a test that fails without the guard: a stress run of the two racing tests in a loop under parallel threads (run locally, not committed) reproduces the wrong message before the change and never after

## 2. Gates

- [x] 2.1 `cargo test --workspace`, clippy, fmt, `openspec validate serial-ffi-tests`; the reproduction and its result recorded in tasks.md below this task

Results:

- Before the change, a stress run (a temporary test, not committed) ran the two racing operations on two threads: a job whose prelude fails, and a printer refusing a second job. The prelude failure read the wrong message in 78 of 3000 iterations.
- After the change, every test in `tests/ffi.rs` holds the file's lock for its whole body, so the two tests can no longer overlap. The file's test binary, run 300 times in a row with the harness's parallel threads, failed 0 times. The stress run races inside one test, so it cannot exercise the guard and was not repeated.
- Gates: `cargo test --workspace` 1299 passed, 0 failed; clippy and `cargo fmt --check` clean; `openspec validate serial-ffi-tests` valid.
