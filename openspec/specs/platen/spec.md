# platen Specification

## Purpose
The job session API a host uses to act as a printer: how bytes are fed
and replies returned while the program arrives, how a job ends, what
the outcome carries, the C ABI, and the build targets.

## Requirements

### Requirement: Incremental job execution

A job SHALL be created from a configuration (identity entries,
prelude, server password, writer options, execution budget) and SHALL
accept its program in pieces of any size; each `feed` SHALL execute as
far as the bytes allow, suspending mid-token if needed, and SHALL
return the standard-output bytes and the error-report bytes produced
since the previous call. `finish` SHALL signal end of data, run to
completion, and return the outcome, the PDF, and the writer's report.
After `finish` the job SHALL accept nothing.

#### Scenario: A query answered before end of data

- **GIVEN** a job fed `statusdict /product get = flush` in three pieces
  split inside the word `product`
- **THEN** the third `feed` returns the product string followed by a
  newline as reply bytes, before `finish` is called

#### Scenario: A page job

- **GIVEN** a job fed a program that fills a rectangle and calls
  `showpage`, then `finish`
- **THEN** the outcome is ok with one page, and the PDF bytes are a
  document whose page holds the fill

#### Scenario: An error reported on the error channel

- **GIVEN** a job fed `1 0 div` then `finish`
- **THEN** the outcome names `undefinedresult` and `div`, and the error
  bytes hold the conventional error report line

### Requirement: C ABI

A C header SHALL declare a versioned interface: create a job from a
configuration struct, feed bytes, read reply bytes, read error bytes,
finish, read the PDF bytes, read the outcome (a code, an error name, an
offending command), and free. Strings and buffers SHALL be owned by the
job until it is freed; no function SHALL call back into the host or
create threads; every function SHALL be safe to call after a failure
except on a freed job.

#### Scenario: The same query through the ABI

- **GIVEN** the query scenario driven through the C functions from a
  test
- **THEN** the reply bytes read after the third feed are the product
  string and newline, and `finish` returns the ok code

### Requirement: Build targets

The crate SHALL build as a static library and an rlib; `cargo check`
for the `wasm32-unknown-emscripten` target SHALL pass in CI; the
library SHALL use no threads, no file system, and no time source, so
it links into an Emscripten program without host shims.

#### Scenario: WebAssembly check

- **WHEN** the workspace is checked for the Emscripten target
- **THEN** `platen` and its dependencies compile

### Requirement: The C interface is stable across the crate rename

Renaming the session crate SHALL leave the static library name
`libplaten.a`, the header `platen.h`, and every `platen_*` symbol
unchanged, so an embedder linking the previous build links the new
one without modification.

#### Scenario: The emulator bridge links unchanged

- **WHEN** the session library is rebuilt after the rename and linked by a host that includes `platen.h` and calls `platen_job_new`
- **THEN** the host builds and runs without any source change

### Requirement: The Emscripten archive is bound to an Emscripten version

A prebuilt session-library archive for the Emscripten target SHALL be
documented as usable only by a program linked with the Emscripten
version the archive names and with WebAssembly exception handling
enabled at the link (`-fwasm-exceptions`, the form the Rust target
uses), and the embedding guide SHALL state both rules and the current
pin.

#### Scenario: A mismatched host

- **WHEN** a host pinned to a different Emscripten version fetches the archive
- **THEN** its fetch step can detect the mismatch from the file name before linking

#### Scenario: A link without exception handling

- **WHEN** a host links the archive without `-fwasm-exceptions`
- **THEN** the link fails on the undefined exception tag, and the
  embedding guide names the flag as the remedy

### Requirement: The C interface as a WebAssembly module

The session library SHALL build for `wasm32-unknown-unknown` as a
`cdylib` whose exports are the C interface's `platen_*` functions and
its memory, and which imports nothing, so a JavaScript host can drive
it without Emscripten; the build SHALL need no change to the crate's
declared crate types and no code beyond the existing C interface.

#### Scenario: A self-contained module

- **WHEN** `cargo xtask npm-package` builds the module
- **THEN** the module's import list is empty and its exports include `platen_job_new`, `platen_job_feed`, `platen_job_finish`, `platen_job_pdf`, and `memory`

### Requirement: Printer sessions

A printer SHALL be created from the same configuration a job takes
(identity entries, prelude, server password, writer options, execution
budget); creating it SHALL build one interpreter, seed the identity,
and run the prelude as the first unencapsulated job. The printer SHALL
open one job at a time over that interpreter; opening a second while
one is open SHALL fail with a busy error. Each job SHALL be fed and
finished as a job created alone is, SHALL produce its own document,
and SHALL begin in the job server's initial state (see `job-server`),
so jobs are isolated unless one uses `startjob` or `exitserver`. The
budget SHALL apply to each job. `Finished` SHALL report whether the job
changed the printer's initial state. A job created alone SHALL be a
printer serving that one job.

#### Scenario: A download persists across jobs

- **GIVEN** a printer, a first job fed `serverdict begin 0 exitserver /x 1 def` and finished, and a second job fed `x =` and finished
- **THEN** the second job's replies are `1` and a newline, and the first job's `Finished` reports a changed initial state

#### Scenario: Encapsulated jobs are isolated

- **GIVEN** a printer, a first job fed `/x 1 def` and finished, and a second job fed `x`
- **THEN** the second job's outcome names `undefined`

#### Scenario: A persisted font is embedded in a later document

- **GIVEN** a printer, a first job that defines a Type 1 font after `exitserver`, and a second job that shows text in it
- **THEN** the second job's PDF embeds that font's program, subset to the glyphs shown

#### Scenario: One job at a time

- **GIVEN** a printer with a job open
- **WHEN** a second job is requested
- **THEN** the request fails with the busy error and the open job is unaffected

#### Scenario: The prelude runs once

- **GIVEN** a printer whose prelude runs `userdict /runs known { /runs runs 1 add def } { /runs 1 def } ifelse`
- **WHEN** three jobs each run `runs =`
- **THEN** each prints `1`

### Requirement: Abandoned jobs

A job dropped or freed before `finish` SHALL be abandoned: the job
server SHALL end it as at an end of data after an error — reverting it
if it was encapsulated, keeping what it had already made permanent if
not — and no document SHALL be produced. The printer SHALL then accept
a new job.

#### Scenario: A connection lost mid-job

- **GIVEN** a printer, a first job fed `/x 1 def` and dropped, and a second job fed `/x where =`
- **THEN** the second job prints `false`

### Requirement: Printer sessions through the C ABI

The C header SHALL declare `platen_printer_new` (taking the job
configuration struct), `platen_printer_job` (returning a job driven by
the existing `platen_job_*` functions, or NULL with `platen_last_error`
explaining when a job is open), and `platen_printer_free`.
`platen_job_free` on a printer's unfinished job SHALL abandon it. The
printer and its jobs SHALL be freeable in either order. The existing
functions and `PLATEN_ABI_VERSION` 1 SHALL be unchanged, so a host built
against the earlier header links and behaves as before.

#### Scenario: Two jobs through the ABI

- **GIVEN** the download scenario driven through the C functions from a test, freeing the printer before the second job
- **THEN** the second job's reply bytes are `1` and a newline, and freeing the job afterwards is valid
