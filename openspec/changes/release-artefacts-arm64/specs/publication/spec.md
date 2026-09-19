## MODIFIED Requirements

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
