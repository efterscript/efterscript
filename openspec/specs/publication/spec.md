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

### Requirement: Releases are tag-triggered and credential-free

A release SHALL be triggered only by pushing a tag of the form `v<X.Y.Z>`
whose version equals the workspace version, and the workflow SHALL
refuse any other tag. It SHALL run the public gate chain on the tagged
commit before publishing, SHALL publish only after a reviewer approves
the protected release environment, SHALL obtain its registry token
through trusted publishing for the duration of the job with no stored
registry credential, and SHALL publish the crates in dependency order,
skipping a crate whose version the registry already holds so a rerun
resumes.

#### Scenario: A tag that does not match the version

- **WHEN** a tag `v9.9.9` is pushed while the workspace version is `0.0.2`
- **THEN** the workflow fails at its version check and publishes nothing

#### Scenario: A rerun after a partial publish

- **WHEN** the publish job is rerun after three of nine crates reached the registry
- **THEN** those three are skipped as already published and the remaining six are published in order

### Requirement: Releases carry the session library

Every tagged release SHALL attach the session library as prebuilt
archives for the Linux hosts `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu` and for the Emscripten target, together with
the C header and one checksum file covering every attached file, each
archive built on the tagged commit and checked by linking and running a
small host program on its own platform.

#### Scenario: A host fetches a release

- **WHEN** a host downloads the archive and header for a tagged version and checks the checksum
- **THEN** the files match the checksum and the host links `platen_job_new` without a Rust toolchain

#### Scenario: The Emscripten archive names its toolchain

- **WHEN** the release job runs with the pinned Emscripten SDK
- **THEN** the uploaded archive is named with that version and the link check passed before upload

#### Scenario: An arm64 Linux consumer

- **WHEN** a build on an arm64 Linux host fetches the release by its
  host triple
- **THEN** the archive `libplaten-<version>-aarch64-unknown-linux-gnu.a`
  exists and its checksum is in `SHA256SUMS`

#### Scenario: One checksum file

- **WHEN** the release's files are produced by more than one job
- **THEN** `SHA256SUMS` still lists every attached archive and the header
