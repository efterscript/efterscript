<!-- SPDX-FileCopyrightText: 2026 EfterScript contributors -->
<!-- SPDX-License-Identifier: MIT -->

# Contributing

EfterScript is spec-driven: behaviour is decided before it is written, and
the decision is kept. This page says how to take part.

## Before you write code

- **Behaviour changes start as an OpenSpec proposal.** Run
  `openspec new change <name>` and fill in the proposal, the spec deltas,
  the design, and the tasks under `openspec/changes/<name>/`; `openspec
  validate <name>` must pass. The change's proposal is the discussion,
  open as a pull request before any implementation. Name a change for the
  behaviour it adds, never for a product.
- **Documentation and trivial fixes** (typos, a comment, a doc page) do
  not need a proposal; open a pull request directly.
- **Read the living specs** under `openspec/specs/` for the area you
  touch, and the archived change's design notes under
  `openspec/changes/archive/` for why the code is the way it is.

## The rules that will get a pull request declined

- **Implement from the published specifications only.** The language
  reference and the PDF standard own semantics; cite them by section
  number and never quote or closely paraphrase them, and never copy their
  examples into the corpus. Do not read, borrow from, or restate another
  interpreter's or PDF tool's source code, tests, or documentation, and
  do not reproduce a remembered implementation. The differential harness
  may observe a reference converter's *behaviour*; that is the only way
  another implementation informs this one.
- **No product or vendor names** in code, comments, tests, corpus files,
  specs, or commit messages. "PostScript" is used only as the name of the
  language. Describe formats and tools generically.
- **Library crates stay pure.** No new dependencies, no `std::fs`, no
  printing to the console, no threads, no time source; every library
  crate forbids unsafe code, and the session library's C interface is the
  one permitted site. The WebAssembly check must keep passing.
- **Every source file carries its SPDX header** (`SPDX-FileCopyrightText:
  2026 EfterScript contributors` and `SPDX-License-Identifier: MIT`); a
  file whose format has no comment syntax is covered by an annotation in
  `REUSE.toml` instead. Third-party assets need REUSE annotations and a
  provenance entry.
- **Goldens are byte-identical unless the change intends otherwise**, and
  then the intended goldens are updated and the reason recorded in the
  design's implementation notes. Where a behaviour deliberately differs
  from the reference converter, register it in the expected-divergences
  spec through the change, and declare it on the corpus file.

## Before you push

Run the public gate chain; the CI workflow runs the same steps:

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p difftest -- run
cargo run -q -p xtask -- parse-survival
cargo run -q -p xtask -- fuzz-round
cargo build --workspace --no-default-features
cargo run -q -p xtask -- check-wasm
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

`cargo xtask fuzz-smoke` runs the fuzz targets for a bounded time when a
nightly toolchain and `cargo fuzz` are installed, and skips otherwise. Two
tiers are private and run only by maintainers: the strings lint and the
oracle comparison, which depend on the reference vault.

## After implementation

Append `## Implementation notes` to the change's `design.md`: what the
code does where it departs from or pins down the design, with the
specification sections it was built from. Tick the tasks. A maintainer
archives the change after merge, which turns its deltas into the living
specs.

## Working with AI assistants

Assistants are welcome and widely used here; the same rules apply to
what they produce. In particular, an assistant may remember other
implementations from its training; instruct it to work from the cited
specification pages and to write the code its own way, and review its
output as you would a stranger's. Every commit is made by a person.

## Licence

By contributing you agree that your contribution is licensed under the
MIT licence in [LICENSE](LICENSE).
