# publication Specification

## Purpose
What the project publishes: the crate family and its naming, the
facade crate a user depends on, the enforced memory-safety boundary,
and the fuzzing that guards the parsers.

## Requirements

### Requirement: The crate family

Every library crate SHALL be named with the `efterscript-` prefix
(`efterscript-vm`, `efterscript-graphics`, `efterscript-fonts`,
`efterscript-pdf`, `efterscript-codec`, `efterscript-remelt`,
`efterscript-platen`), the command-line crate SHALL be
`efterscript-cli` with the binary `efterscript`, and a facade crate
`efterscript` SHALL re-export the distillation entry points and the
session job API so that depending on it alone suffices to distil a
program or run a job. Renaming SHALL change no behaviour: every golden
SHALL stay byte-identical.

#### Scenario: Distilling through the facade

- **WHEN** a program depends only on the `efterscript` crate and calls its distillation entry point on a corpus file
- **THEN** the PDF equals the corpus golden byte for byte

#### Scenario: The workspace builds under the new names

- **WHEN** the workspace is built and tested after the rename
- **THEN** every crate, test, tool, and the corpus run pass unchanged and no crate outside the family remains

### Requirement: The safety boundary is enforced

Every library crate except the session library SHALL forbid unsafe
code at the crate level; the session library SHALL confine unsafe
code to its C interface module and document why.

#### Scenario: Unsafe code is rejected outside the boundary

- **WHEN** an `unsafe` block is added to any library crate other than the session library
- **THEN** the crate fails to compile

### Requirement: Fuzz targets guard the parsers

Fuzz targets SHALL exist for the scanner, the codec crate's decoders,
the filter chain, the JPEG marker walker, each font parser, and a
distillation round trip, each seeded from the corpus, and a workspace
task SHALL run them for a bounded time when the fuzzing toolchain is
available and skip with a message otherwise; a target that panics on
any input SHALL be a defect.

#### Scenario: A bounded fuzz round

- **WHEN** `cargo xtask fuzz-smoke` runs with the fuzzing toolchain installed
- **THEN** every target runs for its bounded time without a crash, and without the toolchain the task reports a skip and exits successfully
