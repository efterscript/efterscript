# Change: An execution budget for the command-line tool

## Why

The command-line tool is what the README offers for converting untrusted
documents, and it sets no execution budget: `{ } loop` runs until killed
from outside. The interpreter already has the bound — `Limits::steps`
raises `limitcheck` when a run has executed its allowance, with a grace
so an error handler can still report — and the session library exposes
it as a configuration field the emulator bridge sets, but the tool never
fills it in, so the one entry point a new user will reach for is the one
without the protection. This is a defect in the tool, not the engine,
and it should be fixed before the first published version: a default
that bounds every run, a flag to raise or remove it, and a documented
exit status for a run that spent its budget, so a caller can tell a
hostile or runaway document from a broken one.

## What Changes

- **A default budget** in all three modes (`run`, `ir`, `pdf`): every run
  is bounded unless the caller asks otherwise. The default is large
  enough that no corpus file, the generator's programs, or either
  captured driver job comes near it, and small enough to end a runaway
  document in seconds on this machine.
- **`--budget <n>`** on every mode sets the allowance in objects
  executed; `--budget unlimited` removes it. A non-integer or zero value
  is a usage error.
- **Exit status** for a run ended by the budget: distinct from an error
  and from success, reported on standard error in the same form as the
  error report, with the PDF still written for `pdf` (the interpreter
  leaves a document, as for any error).
- **Help text** names the flag and the default.
- Out of scope, with triggers: a wall-clock limit (the step budget is
  deterministic, a clock is not — when an embedder needs one it is the
  embedder's); memory and output-size limits (a separate change when a
  document shows the need); a budget on the prelude (it is the caller's
  own program).

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `remelt`: MODIFIED requirement "Command-line distillation" — the
  default budget, the flag, and the exit status.

## Impact

- Code: `crates/efterscript-cli/src/main.rs` (argument parsing shared by
  the three modes, the `Limits` in each `Config`, the exit code, help),
  a CLI test for the loop program and the flag, corpus untouched.
- No new dependencies. No library crate changes.
- Depends on `platen` and `interpreter-core` (the budget mechanism),
  archived.
