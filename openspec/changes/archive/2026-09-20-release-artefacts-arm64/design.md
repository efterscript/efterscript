# Design: release-artefacts-arm64

## D1: Per-platform build jobs, one attach job

The single `artefacts` job becomes three. `host-x86_64` (on
`ubuntu-latest`) and `host-aarch64` (on `ubuntu-24.04-arm`) each build
the host archive with the pinned Rust, link the check program natively
against it, run it, and upload `libplaten-<v>-<triple>.a` as a workflow
artifact. `wasm` (on `ubuntu-latest`) builds the Emscripten archive with
the pinned SDK, runs the existing Node link-and-run check, and uploads
the archive and the header. All three re-check the tag against the
workspace version. An `attach` job, needing the three, downloads every
artifact into `dist/`, writes one `SHA256SUMS` over the lot, and attaches
`dist/*` to the tag's release with `fail_on_unmatched_files`, as the
single job does today. Only `attach` holds `contents: write`.

## D2: Why not a matrix on one job

A matrix cannot share one checksum file: each leg would attach its own,
and the consumer's fetch script verifies against a single `SHA256SUMS`.
Workflow artifacts between jobs are the supported way to hand files to
one finishing job; they also let the wasm and host builds run in
parallel, which shortens the release.

## D3: The native check on each host

The check program that today runs under Node for the wasm archive is the
same C file; each host job compiles it with the system `cc` against its
own archive plus the system libraries a Rust staticlib needs on Linux
(`-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc`, from
`cargo rustc -- --print native-static-libs`) and requires `pages=1`. A
cross-built archive is never uploaded: each is built and run where it
will be used.

## D4: Names

The triple in the file name comes from the job's target, not from
`uname`, so the names stay exact: `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu`. The consumer maps `uname -m` to the triple
(the emulator's makefile already does).

## D5: Docs

The embedding guide's "Prebuilt archives" lists both host archives and
states that a host without one falls back to a checkout build.

## Implementation notes

- **As built.** `artefacts` is replaced by `host-archive` (a two-leg
  matrix: `x86_64-unknown-linux-gnu` on `ubuntu-latest`,
  `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm`), `wasm-archive`, and
  `attach`. Each build job re-checks the tag, builds with the pinned
  toolchain, and runs the check program on its own platform (the host
  legs compile it with the system `cc` and the staticlib's system
  libraries and assert `pages=1`; a leg also asserts `rustc`'s host equals
  its triple, so a mislabeled runner cannot upload a misnamed archive).
  Each uploads its named files as a workflow artifact; `attach` downloads
  them merged into `dist/`, writes one `SHA256SUMS`, and attaches.
  `contents: write` moved to `attach` alone.
- **Checked here.** The workflow parses with five jobs and the stated
  dependency edges; no denylisted string. Not verifiable here: the arm64
  runner and the artifact hand-off; the first tag after this change
  (0.0.3) is the proof.
