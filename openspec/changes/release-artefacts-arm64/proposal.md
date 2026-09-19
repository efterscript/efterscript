# release-artefacts-arm64

## Why

The release attaches one host archive, for `x86_64-unknown-linux-gnu`.
The first consumer, an emulator whose developers work in arm64 Linux
devcontainers, cannot build its headless binary against the release on
those machines: its fetch step finds no archive for the host and must
fall back to a Rust checkout. GitHub provides arm64 Linux runners, so
the release can carry the second host archive at the cost of one more
job.

## What changes

- The release attaches a host archive for `aarch64-unknown-linux-gnu`
  beside the x86_64 one, built and link-checked on an arm64 runner.
- The artefact work is restructured so several jobs can contribute
  files and one checksum file covers them all: per-host build jobs
  upload their files as workflow artifacts, and one attach job collects
  them, writes `SHA256SUMS`, and attaches everything to the release.
- The embedding guide lists both host triples.

## Non-goals

macOS or Windows host archives; changing what the archives contain.
