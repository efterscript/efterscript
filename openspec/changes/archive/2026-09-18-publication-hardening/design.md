# Design: Publication hardening

See proposal.md and the spec deltas. This document fixes the rename
mechanics, the facade's surface, the safety attribute placement, the
workflow's shape, and the fuzz targets.

## Context

- Eight crates under private-phase names; every manifest says the
  published names carry the `efterscript-` prefix; Rust paths import
  them in about 150 source files. The session library's C interface
  is consumed by the emulator bridge as `libplaten.a` + `platen.h`.
- `codec` alone forbids unsafe code; the other library crates contain
  none but do not say so; the session library's `ffi.rs` holds the 38
  uses the C boundary needs.
- One libFuzzer target exists (`ps-vm/fuzz`, the scanner), outside the
  workspace so stable builds stay clean; `cargo xtask fuzz-round` is
  the generator-based round, not libFuzzer.
- No CI exists; the gates are the `xtask` commands and cargo. The
  strings lint needs the vault; the oracle needs the converter.
- The project reserves the `efterscript` GitHub organisation and wants
  a genuine `efterscript` seed crate.

## Goals / Non-Goals

**Goals:** a publishable crate family with one facade; behaviour and
goldens untouched; safety enforced by the compiler; the public tier
green on every push; parsers fuzzed.

**Non-Goals:** publishing (a later deliberate step); the README and
other documents; the emulator bridge (its link line already uses the
unchanged library name); nightly-only CI steps.

## Decisions

**D1. Rename in one commit, directories included.** Package names,
directory names, the workspace `members` and `[workspace.dependencies]`
table, and every `use`/`extern` path change together, mechanically
(`ps_vm::` → `efterscript_vm::` etc.), with `lib.name` set only where
a file name must survive: `efterscript-platen` keeps `name = "platen"`
for its library so the static library and its symbols are unchanged.
Verified by the full gate chain and byte-identical goldens.
*Alternative:* keep directories under the old names — a permanent
mismatch between path and package name for no benefit.

**D2. The facade is thin.** `crates/efterscript` depends on
`efterscript-remelt` and `efterscript-platen` and re-exports their
public surfaces under two modules (`distill` and `session`) plus the
top-level entry points (`distill`, `distill_into`, `Options`, `Params`,
`Report`, `Job`, `JobConfig`); it defines nothing of its own beyond
docs. The CLI depends on the facade. *Alternative:* a facade with its
own API — premature; the entry points are already the API.

**D3. Safety attribute placement.** `#![forbid(unsafe_code)]` in
`efterscript-vm`, `-graphics`, `-fonts`, `-pdf`, `-remelt`, `-codec`
(already), and the CLI; `efterscript-platen` gets `#![deny(unsafe_code)]`
at the crate level with `#![allow(unsafe_code)]` on the `ffi` module
only, so unsafe code cannot appear elsewhere in it.

**D4. The workflow.** One GitHub Actions workflow with a matrix of one
Linux job on the pinned stable toolchain (`rust-version` from the
workspace), caching cargo, running the exact gate commands the
project already uses; the WebAssembly check adds the
`wasm32-unknown-emscripten` target through `rustup` (no Emscripten
needed for a check). The lint and oracle steps are listed in a comment
as private-only. *Alternative:* a nightly fuzz job — added later when
the fuzz targets prove stable.

**D5. Fuzz targets.** A `fuzz/` crate per library that has a parser
(`codec`: inflate, lzw, predictor; `vm`: scanner (moved), filter chain
over bytes, JPEG marker walker, and a whole-program distillation
target that runs the VM with a mock backend; `fonts`: Type 1, CFF,
TrueType), each outside the workspace like today, each seeded by a
tiny corpus copied from `corpus/unit` at first run; `cargo xtask
fuzz-smoke` builds and runs each target for a bounded time (`-max_total_
time=30`) when `cargo fuzz` and a nightly toolchain are found, else
skips with a message. Panics found become corpus files and fixes.

**D6. Metadata.** Each manifest gains `description`, `keywords`,
`categories`, `readme`, and the family `repository` URL; `publish =
false` stays until the registry checks are done, with the flip
recorded as its own commit.

## Risks / Trade-offs

- [A missed path in the rename] → the workspace does not compile;
  the gates catch it.
- [The rename churns every open branch] → only the emulator branch is
  open and it does not touch repo A.
- [Fuzz targets need nightly] → gated behind availability; CI does
  not run them yet.
- [A first fuzz round finds real crashes] → that is the point; each
  becomes a corpus file with a fix before the change archives, or a
  recorded limit with its trigger.

## Open Questions

- None that change the specs or tasks.

## Implementation notes

Part 1 (D1, D2, D3, D6), as built.

**The rename.** The seven directories moved with `git mv` (git records
159 renames, no delete-and-add), the manifests were rewritten, and the
Rust paths were rewritten mechanically (`ps_vm::` → `efterscript_vm::`,
`ps_graphics::`, `ps_fonts::`, `pdf_out::` → `efterscript_pdf::`,
`codec::` → `efterscript_codec::`, `remelt::` → `efterscript_remelt::`)
across 88 source files. Three kinds of site were changed by hand: the
`#[path]` includes of the shared test inflater under `efterscript-pdf/
tests/common`, `xtask check-wasm`'s `-p efterscript-platen`, and the
`fetch-fonts` paths under `crates/efterscript-fonts/data` together with
`REUSE.toml` and the provenance note. The platen audit test's crate list
carries the new names. Two strings were deliberately left alone because
they are written into committed corpus files or compared against them:
the font-corpus generator's `ps-fonts` provenance comment lines in
`efterscript-fonts/tests/corpus_fonts.rs`, and the `pdf-out golden test`
marker in `efterscript-pdf/tests/golden.rs`; changing either would change
a corpus file or a test expectation. The fuzz crate moved with its
parent and its package became `efterscript-vm-fuzz`, depending on
`efterscript-vm` by the same `..` path. `openspec/config.yaml`'s crate
list and the README's layout block use the new names; the archived
changes and the living specs keep the historical names (the `pdf-out`
capability keeps its spec directory). The `platen` workspace dependency
gained `default-features = false` like the other feature-carrying crates,
since the facade now depends on it and the `--no-default-features` build
must reach it.

**`lib.name = "platen"`.** `efterscript-platen` declares `[lib] name =
"platen"`, so the archive is still `target/release/libplaten.a`, the
header is untouched at `crates/efterscript-platen/include/platen.h`, and
`use platen::…` is unchanged in the tests, the embedding guide, and the
facade. The emulator's build file names the header's directory by path
(`crates/platen/include` in `src/core/network/laserwriter.mk`, line 25,
of the emulator repository), which becomes
`crates/efterscript-platen/include`; that edit belongs to the emulator
repository and is not made here.

**The facade.** `crates/efterscript` re-exports three surfaces —
`distill` (`efterscript_remelt::*`), `session` (`platen::*`), and `vm`
(`efterscript_vm::*`) — and at the top level `distill`, `distill_into`,
`distill_seekable`, `Distillation`, `Options`, `Params`, `Report`,
`Job`, `JobConfig`, `Progress`, `Finished`, and `Config`. D2 named only
the engine and the session library as dependencies; the VM was added
because `distill` takes a `Config` that the engine does not re-export,
so a caller depending on the facade alone could not otherwise build one
with injected capabilities (a `Default` suffices for the corpus test but
not for a host). `Outcome` exists in both the VM and the session library
with different meanings and is therefore not exported at the top level.
The command-line tool distils through the facade and keeps direct
dependencies on `efterscript-vm` and `efterscript-graphics` for its
`run` and `ir` modes, which drive the interpreter and the capture
backend without a document. The facade test distils
`corpus/unit/graphics/gray-image.ps` through `distill_into` with the
same seekable sink and provenance comment lines the corpus runner writes
and compares the bytes with the golden; a second test runs a `Job` to
one page.

**The safety boundary.** `#![forbid(unsafe_code)]` sits at the crate
root of `efterscript-vm`, `-graphics`, `-fonts`, `-pdf`, `-remelt`,
`-codec` (already), the facade, and the command-line tool;
`efterscript-platen` has `#![deny(unsafe_code)]` at the root and
`#![allow(unsafe_code)]` as an inner attribute of `ffi.rs` with the
reason. No unsafe code existed outside `ffi.rs`; the workspace compiles
unchanged. The verification is the attribute plus the compiling
workspace: a compile-fail test needs `trybuild`, a dependency, and a
doc test cannot express a failure to compile, so none was added.

**Metadata.** Every crate manifest carries `description` (in the
README's wording), `keywords` (at most five), `categories` from the
registry's fixed list, and `readme.workspace = true` against a
workspace-level `readme = "README.md"`, so the packaged crates all carry
the repository README; `publish = false` stays everywhere. The
private-phase comments are gone from the manifests. `cargo package
--list --allow-dirty` succeeds for all nine crates without `--no-verify`
and lists `README.md` in each; no per-crate README was needed.
`cargo doc --workspace --no-deps` builds without warnings after two
kinds of doc-only edit: the command-line binary is `doc = false`, since
a binary and a library both named `efterscript` would collide on one
output path, and seven intra-doc links that were already ambiguous or
pointed at private items (in `efterscript-vm`, `-fonts`, `-pdf`) became
`mod@` links or plain code spans.

Part 2 (D4, D5), as built.

**The workflow.** `.github/workflows/ci.yml`: one job (`public-tier`)
on `ubuntu-latest`, triggered by every push and pull request, with
`contents: read` permissions and a per-ref concurrency group that
cancels a superseded run. The toolchain is read from the workspace's
`rust-version` by a `sed` step, so the pin has one home;
`actions-rust-lang/setup-rust-toolchain@v1` installs it with `rustfmt`
and `clippy` and its own cache off, `rustup target add
wasm32-unknown-emscripten` adds the check target (no Emscripten SDK: the
step is a `cargo check`), and `Swatinem/rust-cache@v2` caches the
registry and `target/`. Every action is pinned to a major version tag
(`actions/checkout@v4`, `actions-rust-lang/setup-rust-toolchain@v1`,
`Swatinem/rust-cache@v2`); `dtolnay/rust-toolchain` was not used because
its refs are toolchain names, not version tags. The steps are the exact
local commands in the local order: `cargo fmt --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, `cargo test --workspace`,
`cargo run -q -p difftest -- run` followed by `git diff --exit-code --
corpus/golden` (the runner rewrites nothing on a match, so a golden that
differs is a working-tree change and the diff fails the step — the
spec's scenario), `parse-survival`, `fuzz-round`, `cargo build
--workspace --no-default-features`, `check-wasm`, and `cargo doc
--workspace --no-deps` under `RUSTDOCFLAGS=-D warnings`. The comment at
the top names the two private tiers, the commands that run them, and
`EFTERSCRIPT_HELLBOX`, and nothing about the vault's contents. `act` is
not installed; the workflow was verified by parsing the YAML and by
running every step's command locally in the workflow's order (the gate
record below). The first push is the remaining check. D4's "matrix of
one" became a plain single job: a one-entry matrix adds syntax and
nothing else.

**The fuzz crates.** Four, each outside the workspace (`exclude` in the
root manifest), each its own `[workspace]`, `publish = false`,
`[package.metadata] cargo-fuzz = true`, with `target/`, `corpus/`,
`artifacts/`, `coverage/`, and `Cargo.lock` ignored as the scanner's
crate already had:

- `crates/efterscript-codec/fuzz` (`efterscript-codec-fuzz`): `inflate`
  (whole and one byte at a time, required to agree), `lzw` (both
  `EarlyChange` values, whole and streamed, required to agree),
  `predictor` (the first four bytes choose the predictor, `Colors`,
  `BitsPerComponent`, and `Columns` from their allowed sets, `0xFF`
  selecting a value outside each; the output may never exceed the
  input).
- `crates/efterscript-vm/fuzz` (`efterscript-vm-fuzz`): `scan`
  (unchanged but for `rustfmt`), `filters` (one or two decode filters
  chosen with their parameters by the leading bytes, the layout
  documented in the target), `jpeg` (`stream_len` against the
  byte-at-a-time `MarkerWalker`, required to agree), `program` (the VM
  on the bytes with captured output, the recording backend, and
  `Limits::steps = 20_000`).
- `crates/efterscript-fonts/fuzz` (`efterscript-fonts-fuzz`): `type1`
  (`type1::file::parse_file`), `cff` (`cff::parse_fonts`, the first four
  fonts), `truetype` (`TrueTypeProgram::parse`); after a successful
  parse each interprets up to 32 named glyphs and CIDs 0–7 through
  `Program` (`fuzz_targets/glyphs.rs`, shared by `mod`), so charstrings
  are fuzzed and not only containers.
- `crates/efterscript-remelt/fuzz` (`efterscript-remelt-fuzz`):
  `distill` (program text through `distill` with `Options::default()`,
  captured output, `Limits::steps = 20_000`, into a `Vec<u8>`).

Every target discards its `Result`s; the only failures are panics and
the consistency assertions named above. D5 listed the distillation
target under the VM's crate; it lives in the engine's fuzz directory
because the round trip needs the engine.

**Helpers exposed for fuzzing.** The decoders behind `filter` are
crate-private, so `efterscript-vm` gained a `fuzzing` cargo feature and
a `fuzzing` module, compiled under the feature and for the crate's own
tests (so `cargo test` and clippy keep it building): `Filter`,
`PredictorParams`, and `decode_chain`, which drives a `Decoder` over
bytes the way the file table does — fed until the end marker, finished
if the base runs dry first — with unit tests pinning that. Only the
fuzz crate enables the feature, and the module is documented as not
part of the API. The `program` target reuses the boundary tests'
recording backend through `#[path = "../../tests/common/mod.rs"]`
rather than a copy or a feature-gated module: it needs only the crate's
public surface, so an include is the whole cost.

**Seeds.** Each target's `corpus/<target>/` is written by `fuzz-smoke`
when empty (the directories are ignored, not committed): `scan` the
first six `.ps` files by name of `corpus/unit/interp` and `filters`;
`program` `interp` and `graphics`; `distill` `graphics`, `pdfmark`, and
`patterns`; `filters` the `corpus/unit/filters` files plus ten streams
in the target's layout made with the project's own encoders — hex,
base-85, run-length, Flate, Flate with a PNG predictor, LZW under both
`EarlyChange` values, a sub-file, an `eexec` section, and a hex-then-
Flate chain; `inflate` three `deflate::compress` streams; `lzw` both
`lzw::encode` variants; `predictor` TIFF, PNG, and 16-bit PNG rows;
`type1` the test font builder's PFB and PFA; `cff` its CFF and
CID-keyed CFF; `truetype` its TrueType; `jpeg` the project's own
baseline JPEG (`tiny-jpeg`) whole and truncated. The resident outline
files under `crates/efterscript-fonts/data` were not used: at 280–400 KB
each they would set libFuzzer's input length far beyond what mutation
covers usefully, and the synthetic TrueType is a few kilobytes. `xtask`
gained a dependency on `efterscript-codec` for the encoders.

**`cargo xtask fuzz-smoke [--seconds N]`.** Always, first, `cargo
check` of every fuzz crate on the toolchain in use, into
`target/fuzz-check` so the library crates compile once for all four:
`libfuzzer-sys` compiles on stable (its build script compiles the
libFuzzer runtime with the system C++ compiler; only running needs the
nightly sanitizer support), so the targets cannot rot unnoticed. Then
the seeds. Then detection: `rustup run nightly cargo --version` must
answer a line naming `nightly`, and `cargo fuzz --version` a
`cargo-fuzz` line; if either is missing the task prints one line —
`fuzz-smoke: skipped the fuzz run: no nightly toolchain, no cargo-fuzz`
— and exits 0. With both, each target runs as `rustup run nightly cargo
fuzz run <target> -- -max_total_time=N` (default 30) from the crate the
fuzz directory belongs to; `rustup run` rather than `cargo +nightly`
because the `CARGO` a stable `cargo xtask` hands down is the toolchain's
own binary, which does not take `+nightly`. A crash fails the task after
every target has run. The detection and the skip line are unit-tested
from canned outputs; another test checks that every target in the
task's table has a `[[bin]]` in its manifest and non-empty seeds. The
bounded run itself has not happened here: no nightly and no `cargo-fuzz`
are installed, so the skip path is what ran, and the first fuzz round
is open until a machine with both runs `fuzz-smoke`.

**Gate record (part 2, final; replaces part 1's).** `cargo test
--workspace`: 1207 passed, 0 failed, 3 ignored (part 1's 1199 plus the
six `fuzz-smoke` and two `fuzzing` unit tests). `cargo clippy
--workspace --all-targets -- -D warnings`: clean. `cargo fmt --check`:
clean (the fuzz targets, outside the workspace, were formatted with
`rustfmt --edition 2024` directly). `difftest run`: 342 files, 342
passed; `git diff --exit-code -- corpus/golden`: no golden changed.
`parse-survival`: 342 files, 0 failed, no errors. `fuzz-round`: core
1300 programs, 0 failed; graphics 1300 programs, 0 failed.
`fuzz-smoke`: all four fuzz crates check on the stable toolchain, the
eleven targets seeded, the fuzz run skipped with `no nightly toolchain,
no cargo-fuzz`, exit 0. `lint-strings` against the vault: 1202 files
clean of 16 listed strings. `check-wasm`: the session front-end and
its dependencies compile for `wasm32-unknown-emscripten`. `cargo build
--workspace --no-default-features`: builds. `cargo doc --workspace
--no-deps` under `RUSTDOCFLAGS=-D warnings`: no warnings. `openspec
validate publication-hardening`: valid. The captured driver job through
`difftest oracle --profile default` with the host prelude: 1 file, 1
pass, output same. Not verified here: the emulator bridge's build (its
repository is outside this change; its header path edit is recorded
above), the workflow's first push, and the bounded fuzz run itself.
