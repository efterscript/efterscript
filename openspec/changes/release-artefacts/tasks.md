# Tasks: release-artefacts

## 1. Workflow

- [x] 1.1 The `artefacts` job in `.github/workflows/release.yml` per D1–D4 (host and Emscripten builds, the pinned `EMSDK_VERSION`, the link-and-run check, checksums, upload to the tag's GitHub release); verified by YAML parsing, a local host build of the archive with the same commands, and a review of every step

## 2. Documentation and notes

- [x] 2.1 Embedding guide section per D5; design.md gains "## Implementation notes" with the Rust/Emscripten pairing and its source; verified by the strings lint and a read-through
