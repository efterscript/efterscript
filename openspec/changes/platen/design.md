# Design: platen

See proposal.md and the spec deltas. This document fixes the job
object, how incremental execution maps onto the interpreter, the C
ABI's shape, and the build.

## Context

- `Interp::run` executes a source and returns `Outcome::{Ok, Error,
  Suspended}`; `Suspended` means the source ended mid-token and
  `resume` continues when more bytes arrive. Capture streams collect
  standard output and error output.
- `remelt::distill_into` builds an interpreter from a `Config`, installs
  the graphics backend with a `PdfSink` over any `Write`, runs, and
  finishes the document. `Config` carries identity, prelude, and the
  server password since the identity change.
- The emulator side will run one job per driver connection, compose
  its own status text, and link the library into its Emscripten build.

## Decisions

**D1. `Job` owns the interpreter and the sink.** `Job::new(JobConfig)`
builds `Config` from the host's entries (identity, prelude, password,
budget, streams as captures), constructs the interpreter (prelude
errors surface as `JobError::Prelude`), installs the backend with a
`PdfSink` over `Vec<u8>` behind the usual shared cell, and keeps a
growable run source. `feed` appends bytes, calls `run` or `resume`, and
drains the captures into a `Progress { replies: Vec<u8>, errors:
Vec<u8> }`; a job that reached `Ok` or `Error` before end of data is
done and later feeds are refused. `finish` marks the source ended,
runs to completion, finishes the sink, and returns `Finished {
outcome, pdf, report, replies, errors }`. *Alternative:* a callback per
output byte — the C ABI must not call back into the host.

**D2. Outcome.** `Outcome::Ok`, `Outcome::Error { name, offending }`,
`Outcome::Budget` (the execution budget hit, surfaced distinctly from
`limitcheck` so a host can tell a runaway from a resource error),
`Outcome::Prelude { name }`. `Report` from the writer is included for
page count and notes.

**D3. C ABI.** `platen.h` (hand-written, `PLATEN_ABI_VERSION 1`):

```c
typedef struct platen_job platen_job;
typedef struct { const char *key; const char *value; } platen_entry; /* value: PostScript literal text */
typedef struct {
  uint32_t abi_version;
  const platen_entry *identity; size_t identity_len;
  const uint8_t *prelude; size_t prelude_len;
  int32_t server_password;
  int compress; int embed_all_fonts;
  uint64_t step_budget; /* 0 = unlimited */
} platen_config;
platen_job *platen_job_new(const platen_config *cfg);            /* NULL on failure; platen_last_error() explains */
int platen_job_feed(platen_job *, const uint8_t *, size_t);       /* 0 ok, 1 finished early, <0 error */
size_t platen_job_read_replies(platen_job *, uint8_t *buf, size_t cap); /* drains; call until 0 */
size_t platen_job_read_errors(platen_job *, uint8_t *buf, size_t cap);
int platen_job_finish(platen_job *);                              /* outcome code: 0 ok, 1 error, 2 budget, 3 prelude */
const uint8_t *platen_job_pdf(const platen_job *, size_t *len);   /* valid until free */
const char *platen_job_error_name(const platen_job *);            /* "" when none */
const char *platen_job_offending(const platen_job *);
uint32_t platen_job_pages(const platen_job *);
void platen_job_free(platen_job *);
const char *platen_last_error(void);                              /* thread-local-free: a static buffer, last failure */
```

Identity values are PostScript literal text (`(LaserWriter)`, `47.0`,
`true`, `/name`, `[…]`) parsed with the scanner into objects, which
keeps the ABI to strings. All allocations are the job's; the host
copies what it needs. Panics are caught at the boundary and turned
into error codes (`catch_unwind`), because unwinding across C is
undefined. *Alternative:* a generated header — a dependency for one
small file.

**D4. Builds.** `[lib] crate-type = ["staticlib", "rlib"]`; the
`#[no_mangle] extern "C"` functions live in `src/ffi.rs`; `panic =
"abort"` is not set (the boundary catches unwinds instead). CI adds
`rustup target add wasm32-unknown-emscripten` and `cargo check -p
platen --target wasm32-unknown-emscripten`; a `docs/embedding.md` in
the crate explains linking with `emcc … libplaten.a
-sEXPORTED_FUNCTIONS=…` and the native static-library link. The
library uses no threads, no `std::fs`, no `std::time` (a grep test
enforces the three over `platen`'s dependency closure's own sources,
i.e. the workspace crates, since external crates are none).

**D5. Streams.** Standard output is the reply channel and standard
error the error channel, per the interpreter's existing routing; the
host merges or phrases as it likes. `flush` does nothing extra: the
capture already holds the bytes; `feed` drains whatever exists.

**D6. `languagelevel`** returns the integer 2 from `systemdict`.

**D7. Persistence.** Per the decision, none between jobs. The charter's
persistent-parent model is recorded as deferred; a host needing it can
re-send server-level downloads per connection, which the drivers of
the era do when the query says the download is absent.

## Risks / Trade-offs

- [A long job blocks a browser frame] → the budget bounds it; a
  cooperative yield is the recorded follow-up if measurements demand.
- [`Suspended` semantics at odd split points] → the query scenario
  splits inside a token; proptests split random programs at random
  points and compare with the unsplit run.
- [Panics inside the interpreter reach C] → `catch_unwind` at every
  entry point, then the job is poisoned and every call returns the
  failure code.
