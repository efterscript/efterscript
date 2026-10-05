# Change: Serial FFI integration tests

## Why

The C interface keeps the last failure's message in one static buffer,
unguarded because the interface is single-threaded by contract
(`platen_last_error`). The integration tests in
`crates/efterscript-platen/tests/ffi.rs` call that interface from the
test harness's parallel threads, so one test can overwrite the message
another is about to read. It has happened once: in
`a_rejected_identity_and_a_failing_prelude_return_null`, a failing
prelude's message (`prelude failed: undefinedresult in div`) was read as
`a job is open on this printer`. That message was written at the same
moment by `a_printer_keeps_a_download_across_jobs`, which checks that a
printer refuses a second job. The race is rare (it passed eight runs of
its own), but it can fail CI on any pull request, unrelated to the
change under review.

The unit tests in `src/ffi.rs` share the same buffer and already run one
at a time behind a mutex. The integration tests lack that guard; fixing
it now keeps an intermittent failure from being dismissed as a flake on
someone else's pull request.

## What Changes

- **The integration tests run one at a time**: every test in
  `crates/efterscript-platen/tests/ffi.rs` holds a file-wide mutex for
  its whole body, recovering the lock if an earlier test panicked while
  holding it, as `src/ffi.rs`'s tests already do. Tests keep their
  bodies and assertions unchanged; none is skipped or removed.

No library code, interface, or behaviour changes. The buffer stays
unguarded: single-threaded use is the interface's contract, and a C host
that calls it from one thread is unaffected.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. The change is test-only, so it declares `skip_specs`.

## Impact

- `efterscript-platen`: `tests/ffi.rs` only.
- No other crate, no corpus file, no golden, no spec.
