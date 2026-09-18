# Change: platen — the job session API and its C ABI

## Why

A printer is a session: bytes arrive from a driver, replies go back on
a channel while the program is still arriving, and a document comes out
at the end. EfterScript can distil a whole file but has no surface for
that exchange, and a host that emulates a printer, natively or in a
browser, needs one it can call from C. `platen` is that surface. It is
deliberately thin: the interpreter, the identity mechanism, and the
writer already exist; `platen` packages them as a job that is fed
incrementally, drained for its output, and finished for its PDF, with a
C ABI and a WebAssembly build that links into an Emscripten program.
Two decisions taken with the emulator side shape it: one interpreter
instance per job, so nothing persists between jobs and the host
re-sends what a printer would have kept; and status text is the host's
business, so `platen` reports facts and never phrases them.

## What Changes

- **A job API** (`platen`, Rust): `Job::new(JobConfig)` builds an
  interpreter seeded with the host's identity entries and prelude and a
  PDF sink; `feed(bytes)` executes as far as the bytes allow and
  returns the new reply bytes and error-report bytes the program
  produced; `finish()` signals end of data, runs to completion, and
  returns the outcome (ok, error name and offending command, or budget
  exhausted), the PDF bytes, and the writer's report. Replies are the
  program's standard-output bytes; error reports are the conventional
  `%%[ Error: …; OffendingCommand: … ]%%` lines; the host routes both.
- **Per-job instances**: no state survives `finish`; downloads through
  `exitserver` persist only within the job; the host maps one driver
  connection to one job. Recorded as the model, with the founding plan's
  persistent-parent design deferred to a later profile if a host needs
  it.
- **A C ABI** in the same crate (`platen.h`, hand-written, versioned):
  create a job from a configuration struct (identity as PostScript
  literal text per entry, prelude bytes, server password, compression
  and embed-all flags, execution budget), feed, read replies, read
  errors, finish, get the PDF, get the outcome, free; all memory owned
  by the job until freed; no callbacks, no threads.
- **Builds**: `crate-type` static library plus rlib; the
  `wasm32-unknown-emscripten` target checked in CI so the archive links
  into an Emscripten program; native static library for headless hosts;
  a `languagelevel` operator answering 2, since drivers branch on it.
- **A reference host** in repo A for tests only: a small Rust test
  harness that feeds a query program in pieces and asserts the reply
  arrives after the `flush` and before end of data, then feeds a job
  and reads the PDF; the same through the C ABI from Rust.
- Out of scope, with triggers: transports (LPD, raw socket, pty, PAP —
  the host's); status phrasing; cross-job persistence (a host that
  needs it); `startjob`; asynchronous status while a long job runs
  (the host polls `feed` results; a cooperative yield if a job proves
  too long for a browser frame).

## Capabilities

### New Capabilities
- `platen`: the job API, the incremental feed and reply contract, the
  outcome, the C ABI, and the build targets.

### Modified Capabilities
- `interpreter-core`: ADDED requirement for `languagelevel`.

## Impact

- Code: `crates/platen` (Rust API, C ABI, header, tests), `crates/ps-vm`
  (`languagelevel`; the suspend-and-resume path exercised by `feed`),
  `crates/remelt` (a sink over an in-memory buffer already exists), CI
  configuration for the WebAssembly check, `README.md` embedding notes.
- No new dependencies. The Emscripten toolchain is not required to
  build or test repo A; only the Rust target is.
- Depends on `printer-identity-mechanism` (archived).
