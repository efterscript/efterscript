<!--
SPDX-FileCopyrightText: 2026 EfterScript contributors
SPDX-License-Identifier: MIT
-->

# Embedding platen

`platen` builds as a Rust library (`rlib`) for Rust hosts and as a
static library (`staticlib`) for hosts in other languages, which use
the C ABI declared by [`include/platen.h`](../include/platen.h). The
library creates no threads, opens no files, and reads no clock, so it
links into headless programs and into an Emscripten program without
host shims.

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
cargo build --release -p platen
# target/release/libplaten.a
cc host.c -I crates/platen/include target/release/libplaten.a -lm -o host
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
cargo build --release -p platen --target wasm32-unknown-emscripten
# target/wasm32-unknown-emscripten/release/libplaten.a
emcc host.c -I crates/platen/include \
  target/wasm32-unknown-emscripten/release/libplaten.a \
  -sEXPORTED_FUNCTIONS=_platen_job_new,_platen_job_feed,_platen_job_read_replies,_platen_job_read_errors,_platen_job_finish,_platen_job_pdf,_platen_job_error_name,_platen_job_offending,_platen_job_pages,_platen_job_free,_platen_last_error,_malloc,_free \
  -sEXPORTED_RUNTIME_METHODS=ccall,cwrap,HEAPU8 \
  -sALLOW_MEMORY_GROWTH=1 \
  -o host.js
```

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
