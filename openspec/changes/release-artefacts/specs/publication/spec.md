## ADDED Requirements

### Requirement: Releases carry the session library

Every release SHALL attach to its GitHub release the session library's
static archive for the Linux host and for the Emscripten target, the C
header, and a checksum file, built on the tagged commit with the
pinned toolchains; the Emscripten archive's file name SHALL carry the
Emscripten version it was built with, and the release SHALL be refused
if a minimal C program fails to link against that archive with the
same Emscripten.

#### Scenario: A host fetches a release

- **WHEN** a host downloads the archive and header for a tagged version and checks the checksum
- **THEN** the files match the checksum and the host links `platen_job_new` without a Rust toolchain

#### Scenario: The Emscripten archive names its toolchain

- **WHEN** the release job runs with the pinned Emscripten SDK
- **THEN** the uploaded archive is named with that version and the link check passed before upload
