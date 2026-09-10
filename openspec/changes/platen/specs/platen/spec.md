# platen

## Purpose

The job session API a host uses to act as a printer: how bytes are fed
and replies returned while the program arrives, how a job ends, what
the outcome carries, the C ABI, and the build targets.

## ADDED Requirements

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

### Requirement: Per-job instances

Each job SHALL run in its own interpreter with its own seeded identity
and prelude; nothing SHALL persist between jobs. Definitions made after
`exitserver` within a job SHALL persist for that job.

#### Scenario: No state between jobs

- **GIVEN** a job defining `/x 1 def` at the server level after
  `exitserver`, finished, then a second job from the same configuration
  running `x`
- **THEN** the second job's outcome names `undefined`

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
