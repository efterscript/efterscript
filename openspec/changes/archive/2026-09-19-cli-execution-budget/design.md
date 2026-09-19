# Design: An execution budget for the command-line tool

See proposal.md and the spec delta.

## Context

- `Limits::steps: Option<u64>` bounds a run; on exhaustion the VM raises
  `limitcheck` on the current object, grants a grace of a sixteenth (at
  least 1000 objects) for the error handler, then raises on every object
  after a second exhaustion, so a handler that loops still ends. The
  outcome reaches the caller through the existing `Outcome`.
- The session library takes `step_budget` and reports a distinct
  `PLATEN_OUTCOME_BUDGET`; the emulator bridge sets 100 million.
- The CLI builds a `Config` per mode with default limits and maps the
  outcome to exit status 0 or 1 (2 is usage).

## Goals / Non-Goals

**Goals:** every CLI run bounded by default; the bound adjustable and
removable; a budget exit distinguishable from an error; no change to any
library crate or golden.

**Non-Goals:** wall-clock, memory, or output limits; budgeting the
prelude separately; changing the VM's grace behaviour.

## Decisions

**D1. The default is 100 million objects.** The same figure the emulator
bridge chose after running the captured jobs: the largest corpus file
and both captured driver jobs execute well under one million objects,
the generator's programs far less, and a tight loop of 100 million
objects ends in a few seconds in a release build (measure and record
the figure in the notes). *Alternative:* unlimited by default with the
flag opt-in — leaves the documented untrusted-input path unprotected,
which is the defect.

**D2. One flag, parsed once.** `--budget <n>` accepts a positive
integer or the word `unlimited`; parsing lives in the shared argument
pass so all three modes get it, and the value goes into
`Limits::steps` of each mode's `Config`. Zero and non-integers are usage
errors (exit 2, the existing path).

**D3. Exit status 3.** The VM's outcome for an exhausted budget is
distinguishable from other errors (check how `Outcome` carries it; the
session library already distinguishes it, so the VM does). The CLI maps
it to 3 and prints a one-line report on standard error in the existing
`%%[ … ]%%` form; for `pdf` the document is still written first, as for
any error. *Alternative:* exit 1 like any error — a caller could not
tell a hostile document from a broken one, which is the point of the
status.

**D4. Verification.** A CLI integration test (the tool's own `tests/`,
spawning the binary): the loop program ends with status 3 and the
report line; `--budget 1000` on a program of a few thousand objects ends
with 3 while the default does not; `--budget unlimited` accepted;
`--budget 0` and `--budget x` give 2; `pdf` with an exhausted budget
still writes the file. The corpus and goldens are untouched.

## Risks / Trade-offs

- [A legitimate large job hits the default] → the report names the flag;
  100 million is two orders of magnitude above any job seen.
- [The grace lets a handler print after exhaustion] → intended; the
  second exhaustion ends it regardless.

## Open Questions

- None.

## Implementation notes

- **Parsing (D2).** `budget_args` in `crates/efterscript-cli/src/main.rs`
  runs once in `main`, before the mode dispatch: it takes every
  `--budget <value>` pair out of the argument list wherever it stands
  (the last one wins) and hands the rest to the existing dispatch, so
  `run`, `ir`, and `pdf` share one parser and `pdf_args` never sees the
  flag. `unlimited` gives `None`; a positive integer gives `Some(n)`;
  zero, a negative or fractional number, another word, or a missing
  value is reported as `efterscript: --budget …` followed by the usage
  text, exit 2. The default `Some(100_000_000)` goes into
  `Limits::steps` of every mode's `Config` through one `limits` helper.
- **Discrimination (D3).** The VM's `Outcome` has no budget variant; the
  interpreter exposes `budget_exceeded()` and the distillation `Report`
  carries it as `budget_exceeded`. The tool applies the session
  library's rule: `Outcome::Error(_)` with the flag set is the budget,
  any other `Error` is the program's, and an `Ok` outcome after the flag
  was set (a handler caught the `limitcheck` and ended within the grace)
  is success, exactly as the session library reports it. No library
  crate changed.
- **Exit status and report (D3).** Status 3 (`BUDGET_SPENT`). After the
  interpreter's own `%%[ Error: limitcheck; OffendingCommand: … ]%%` line
  the tool prints one more in the same form on standard error:
  `%%[ Budget: spent after <n> objects; --budget <n> raises it, --budget unlimited removes it ]%%`.
  In `pdf` mode `distill_into` has closed the document before the
  status is decided, so the file is complete as for any error.
- **Measurements (D1).** `{ } loop` under the default: **6.4 s** in a
  release build (`time cargo run --release -p efterscript-cli -- run
  loop.ps`, user 6.405 s), **129 s** in a debug build, on this machine.
  Both captured driver jobs distil under the default with the host
  prelude and exit 0; the tool has no counter output, so their object
  counts (prelude included) were bracketed by bisecting `--budget` on
  the exit status: the directory listing needs between 5 703 and 5 742
  objects, the bitmap-font variant between 15 000 and 15 039 — four
  orders of magnitude under the default.
- **Verification (D4), one deviation.** The always-run runaway test
  spends `--budget 1000000` rather than the default, because a debug
  test build spends 100 million objects in over two minutes; the
  default-budget runaway is the same test marked `#[ignore]` with a
  600 s bound (`cargo test -p efterscript-cli --test budget --
  --ignored`; passed in 140 s). The spec's "still running under an
  external timeout" half of the adjustable scenario is not a test;
  `--budget unlimited` is verified as accepted on a program that ends.
  The corpus and goldens are untouched (342 files, byte-identical).
- **Documentation.** The usage text gains an "options of every mode"
  block naming the flag, the default, and status 3; the README's
  command-line paragraph names the budget, the flag, and the four exit
  statuses.
