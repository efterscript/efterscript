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
Vec<u8> }` — which needs the interpreter to suspend not only inside a
token but inside an operator's read of the job's source (see the
implementation notes: this was missing and is added in `ps-vm`); a job that reached `Ok` or `Error` before end of data is
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

Identity values are PostScript literal text (`(Fictional Press)`, `47.0`,
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

## Implementation notes

Recorded where the code departs from, or pins down, the text above.

- **Suspension inside an operator's read (amends D1).** D1 assumed
  `run`/`resume` covered incremental execution, but the scanner was the
  interpreter's only suspension point: an operator reading the job's
  source — `readline`, `readstring`, `readhexstring`, `read`, `token`,
  an `eexec` section, image data from `currentfile` — saw end of data
  at a piece boundary, which the core change's notes had left to this
  one. The captured driver job uses `readline`, `readhexstring`, and
  `eexec` on `currentfile`, so a job fed in pieces could not work
  without it. Built in `ps-vm`: `Stream` gains `more_may_come` (default
  false) and `unread`; the run file's stream reports more may come
  while the source does and fails an empty read with the internal
  `VmError::NeedMore` (not in `VmError::ALL`; named `ioerror` should it
  ever be reported, which a debug assertion forbids). The file table
  snapshots an entry whose bytes may grow — its pushback, position, and
  an `eexec` layer's cipher state — at the first read of an operator,
  records the bytes taken from the stream, and `rollback` restores them
  and hands the bytes back; `commit` at the start of every executed
  object and loop step forgets the snapshot (a length check per
  object). A layer that is starved mid-form-detection or mid-hex-pair
  returns what it took to its base first. `FileSource` turns `NeedMore`
  into end-of-input with "more may come", so the scanner over a file
  slot (an `eexec` section, `currentfile exec`) suspends as the run
  slot does. The loop, on `NeedMore` from an operator, rolls the file
  table back, refunds the step charged (a split job counts as an
  unsplit one), re-queues the operator as an `Object` frame, and
  returns `Outcome::Suspended`; a loop step's `NeedMore` leaves the
  frame in place. `resume` pumps the source before its first step so
  the re-queued operator sees the new bytes. `closefile` on the job's
  source now closes it for the rest of the run, so bytes fed later are
  discarded as a whole file's remainder was. A reading operator pops
  nothing before its read completes, so the operand stack needs no
  restoring; a partially filled string is overwritten by the re-run.
  Cost of the all-or-nothing re-run: an operator that needs more than
  one piece re-reads what arrived so far each time, quadratic in the
  number of pieces for one large read — a resumable acquisition is the
  follow-up if a host measures it. `crates/ps-vm/tests/chunked.rs` runs
  each case cut at every byte against the unsplit run (outcome, output,
  error output, backend log), including the budget's count.
- **The `Distillation` (D1).** `remelt` gained `Distillation<W>` (the
  generic noun, since the product name is not used) — `new(config,
  sink)`, `run(&[u8])` (a slice source, the unsplit form), `feed(&[u8])
  -> Outcome` (append; `run` on the first call, `resume` after; bytes
  after the job ended are ignored and the outcome repeated), `finish()
  -> (Report, W)`, plus `pages()`, `outcome()`, `is_done()`, and the
  interpreter accessors — and `Report.budget_exceeded`. `distill_into`
  is `new` + `run` + `finish`, so every golden is byte-identical.
- **`Job` as built (D1, D2).** `JobConfig { identity: Vec<(String,
  String)>, prelude, server_password, options: remelt::Options,
  step_budget: Option<u64> }`; `Job::new` parses each identity value
  with the scanner (`identity.rs`: a scalar, `[`…`]`, `<<`…`>>`, and
  the names `true`/`false`/`null`, which scan as names; anything else,
  or more than one value, is `JobError::Identity { key, detail }`),
  builds `Config` with captures and an empty readable `%stdin`, and
  discards the prelude's own output — it is the device's, not the
  job's. `feed` returns `Progress { replies, errors, done }` and
  `JobError::Finished` once done; `finish` returns `Finished { outcome,
  pdf, report, replies, errors }`. `Outcome` is `Ok`, `Error { name,
  offending }`, `Budget` (an `Error` outcome with the report's
  `budget_exceeded`): D2's `Outcome::Prelude` is dropped, since a job
  whose prelude fails is never created — `JobError::Prelude` reports it
  — and the C outcome code 3 stays reserved for stability. A token ends
  at a delimiter, so a piece that should complete a command ends with
  whitespace; the query scenario's reply arrives on the third feed
  because `=` is complete before `flush` is. The command-line tool's
  `--identity` keeps its bare-word convenience syntax (`Average`,
  `false`), which is not the scanner's; the two parsers stay separate.
- **`languagelevel` (D6)** was already a `systemdict` constant of 2;
  `corpus/unit/interp/languagelevel.ps` pins it.
- **C ABI as built (D3).** As sketched. Codes, all in the header:
  `platen_job_feed` returns `PLATEN_OK` (0) while the job waits,
  `PLATEN_DONE` (1) once it ended before its data did — on that call or
  any later one — and negative failures `PLATEN_ERR_ARGUMENT` (-1),
  `PLATEN_ERR_STATE` (-2: feed after finish, finish twice),
  `PLATEN_ERR_PANIC` (-3), `PLATEN_ERR_DOCUMENT` (-4); `platen_job_finish`
  returns `PLATEN_OUTCOME_OK/ERROR/BUDGET` (0/1/2) or a failure.
  `platen_job_pdf` is NULL with length 0 before `finish`;
  `platen_job_pages` counts live during the job. `platen_last_error` is
  a 512-byte static buffer, unguarded; the interface is single-threaded
  by contract. Every entry runs under `catch_unwind`; a panic poisons
  the job and sets the message, `platen_job_free` never fails and
  catches a panic in the drop. `panic = "abort"` is not set. The unit
  test poisons a job through the boundary helper itself and checks
  every function's failure answer.
- **Builds (D4).** `crate-type = ["staticlib", "rlib"]`. The repository
  has no CI configuration, so the target gate is `cargo xtask
  check-wasm` (`cargo check -p platen --target
  wasm32-unknown-emscripten`), documented in `docs/embedding.md` with
  the native and Emscripten links and the exported symbol list. No
  workspace crate needed a change for the target. The audit test scans
  the six library crates' sources for `std::thread`, `std::fs::`,
  `std::time`, `SystemTime`, `Instant` outside comment lines: no hits.
- **The private tier (4.1).** `crates/platen/tests/private.rs`
  (ignored; paths from `EFTERSCRIPT_PRIVATE_JOB` and
  `EFTERSCRIPT_PRIVATE_PRELUDE`) feeds the captured driver job in
  512-byte pieces with the host prelude: 73 feeds, one page, outcome
  ok, no reply or error bytes (the job's setup probes the device
  without printing), and the document, replies, and errors identical to
  the whole-file distillation with the same configuration.
- **Gates.** `cargo test --workspace` 915 passed, 0 failed, 3 ignored
  (from 886 / 2: the private-tier test is the third); clippy clean on
  all targets; fmt clean; `difftest run` 184 of 184, every pre-existing
  golden byte-identical; `parse-survival` 184 files, no failures;
  `fuzz-round` 2 600 programs (1 300 core, 1 300 graphics), 0 failed;
  `lint-strings` clean (778 files); `cargo xtask check-wasm` passes
  (platen and its five dependencies compile for
  `wasm32-unknown-emscripten`); `openspec validate platen` valid.
- **Follow-ups.** A cooperative yield if a job proves too long for a
  browser frame; a resumable read for a large image or string taken
  from the job's source in many small pieces; cross-job persistence,
  per D7, only if a host asks; a `Metrics`/`bytesavailable` probe is
  still undefined.
