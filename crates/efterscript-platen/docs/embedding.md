<!--
SPDX-FileCopyrightText: 2026 EfterScript contributors
SPDX-License-Identifier: MIT
-->

# Embedding efterscript-platen

The `efterscript-platen` crate builds as a Rust library (`rlib`, Rust
path `platen`) for Rust hosts and as a static library (`staticlib`,
`libplaten.a`) for hosts in other languages, which use the C ABI
declared by [`include/platen.h`](../include/platen.h). The library
creates no threads, opens no files, and reads no clock, so it links
into headless programs and into an Emscripten program without host
shims.

## Rust

```rust
use platen::{Job, JobConfig, Outcome};

let mut job = Job::new(JobConfig {
    identity: vec![("product".into(), "(Fictional Press)".into())],
    ..JobConfig::default()
})?;
for piece in transport {
    let progress = job.feed(&piece)?;
    reply_channel.write_all(&progress.replies)?;
    if progress.done { break; }
}
let finished = job.finish()?;
assert!(matches!(finished.outcome, Outcome::Ok));
std::fs::write("out.pdf", &finished.pdf)?;
```

## Native static library

```sh
cargo build --release -p efterscript-platen
# target/release/libplaten.a
cc host.c -I crates/efterscript-platen/include target/release/libplaten.a -lm -o host
```

The archive is self-contained apart from the C runtime; on Linux the
Rust standard library additionally needs `-lpthread -ldl` with older
toolchains, and on macOS nothing. The header is hand-written and
versioned by `PLATEN_ABI_VERSION`; a configuration naming another
version is refused by `platen_job_new`.

## Emscripten

Install the target once (`rustup target add wasm32-unknown-emscripten`)
and check that the crate and everything under it compile for it with
`cargo xtask check-wasm`, which is the gate this crate keeps. A full
build needs the Emscripten toolchain (`emcc`) on the path so cargo can
link the target's standard library:

```sh
cargo build --release -p efterscript-platen --target wasm32-unknown-emscripten
# target/wasm32-unknown-emscripten/release/libplaten.a
emcc host.c -I crates/efterscript-platen/include \
  target/wasm32-unknown-emscripten/release/libplaten.a \
  -fwasm-exceptions -sWASM_LEGACY_EXCEPTIONS=1 \
  -sEXPORTED_FUNCTIONS=_platen_job_new,_platen_job_feed,_platen_job_read_replies,_platen_job_read_errors,_platen_job_finish,_platen_job_pdf,_platen_job_error_name,_platen_job_offending,_platen_job_pages,_platen_job_free,_platen_last_error,_malloc,_free \
  -sEXPORTED_RUNTIME_METHODS=ccall,cwrap,HEAPU8 \
  -sALLOW_MEMORY_GROWTH=1 \
  -o host.js
```

Rust compiles this target with WebAssembly exception handling on (the
legacy form), so the final link must pass `-fwasm-exceptions`; the
archive's objects reference the exception tag and the link fails with an
undefined `__cpp_exception` without it. Only the link line needs the
flag: C objects compiled without it link fine. Passing
`-sWASM_LEGACY_EXCEPTIONS=1` keeps the form explicit should the SDK's
default move.

The exported symbols, in the order the header declares them:

| Symbol | Purpose |
| --- | --- |
| `platen_job_new` | create a job from a `platen_config` |
| `platen_job_feed` | append bytes and execute as far as they allow |
| `platen_job_read_replies` | drain reply bytes (standard output) |
| `platen_job_read_errors` | drain error-report bytes (standard error) |
| `platen_job_finish` | end of data; run to completion; close the document |
| `platen_job_pdf` | the finished document's bytes |
| `platen_job_error_name` | the error name of an error outcome |
| `platen_job_offending` | the offending command of an error outcome |
| `platen_job_pages` | pages shown so far or in the document |
| `platen_job_free` | release the job |
| `platen_last_error` | the last failure's message |

A host calling from JavaScript allocates the configuration struct and
the byte buffers in the module's heap (`_malloc`), fills them, and
passes the pointers; the PDF bytes are read from `HEAPU8` at the pointer
`platen_job_pdf` returns before the job is freed. A long job blocks the
frame it runs in; the execution budget (`step_budget`) bounds it.

## Contract in brief

- One job per connection; nothing persists between jobs. A host that
  needs downloads to persist re-sends them per connection, which the
  drivers of the era do when their query says the download is absent.
- Standard output is the reply channel and standard error the error
  channel; the host merges or phrases them as it likes. `flush` does
  nothing extra: a feed returns whatever the program has written.
- A feed may split the program anywhere — inside a token, inside data
  an operator reads — and the job behaves as if it had arrived whole.
  A token ends at a delimiter, so a piece meant to complete a command
  ends with whitespace; otherwise the command runs when the next piece
  or `finish` delimits it.
- Every function is safe to call after a failure, except on a freed
  job. A panic is caught at the boundary; the job is then poisoned and
  every later call returns `PLATEN_ERR_PANIC`.

## Prebuilt archives

Every tagged release attaches the session library, built on the tagged
commit, to the GitHub release of the tag, so a host needs no Rust
toolchain:

- `libplaten-<version>-x86_64-unknown-linux-gnu.a` and
  `libplaten-<version>-aarch64-unknown-linux-gnu.a` — the host archives,
  one per Linux host; a host without one builds from a checkout.
- `libplaten-<version>-wasm32-unknown-emscripten-<emsdk>.a` — the
  Emscripten archive; `<emsdk>` is the Emscripten SDK version it was
  built with.
- `platen-<version>.h` — the header, identical to
  `include/platen.h` at that tag.
- `SHA256SUMS` — checksums of the files above.

An Emscripten archive is usable only by a program linked with the same
Emscripten version: Emscripten's runtime ABI is not semantically
versioned, and Rust's prebuilt standard library for the target is built
against one SDK release. The archive's file name carries that version
so a host's fetch step can refuse a mismatch before linking. The
current pair is Rust 1.98.0 with Emscripten SDK 6.0.7; the release
workflow declares both in one place and they change together. The link must also pass `-fwasm-exceptions` (see above); a
release's archive is checked by linking and running a small host with
exactly that flag.

A fetch step, given a version and a pinned SDK:

```sh
v=0.0.2; sdk=6.0.7
base=https://github.com/efterscript/efterscript/releases/download/v$v
curl -fsSLO $base/SHA256SUMS
curl -fsSLO $base/libplaten-$v-wasm32-unknown-emscripten-$sdk.a
curl -fsSLO $base/platen-$v.h
sha256sum --check --ignore-missing SHA256SUMS
```

