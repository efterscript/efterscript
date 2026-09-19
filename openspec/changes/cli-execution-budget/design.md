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
