# Design: psgen v0

See proposal.md. This document fixes the generator's model, the random
source, the property runner, the shrinker, and the budget.

## Context

- `difftest` runs programs in process through `Interp` with capture
  streams and the graphics backend, and its oracle subcommand takes
  paths. `ps_graphics::dump` gives a canonical IR text. `remelt::distill`
  produces a PDF; `pdf-out`'s structural checks exist as test helpers.
- `Limits` covers stacks only; nothing bounds execution.
- `tools/psgen` is a stub; `corpus/generated/` is empty.

## Goals / Non-Goals

**Goals:** programs that are well formed and mostly well typed, cheap
to generate and run by the thousand, checkable without an oracle, and
shrinkable; a seed-file contract CI can run.

**Non-Goals:** fonts beyond the resident set, images, composite text,
syntax fuzzing, coverage guidance.

## Decisions

**D1. Grammar with a static stack model.** The generator keeps a model
of the operand stack as a list of abstract types (int, real, bool,
string, name, array, proc, dict, mark, any) and of the dictionary
stack's definitions (name → type). Each production consumes and
produces model types; an operator is chosen from those whose inputs
the model can satisfy, else a literal is pushed. Procedures are
generated with their own model and a declared signature so calls type
check. Loops are `repeat` and `for` with small literal counts, `loop`
only with an `exit` reached by a counter; `stopped` wraps a share of
blocks. An `ill_typed` share replaces one operand's type at a chosen
operation so the error machinery runs; the program ends in `stopped`
handling in that case so it terminates normally. *Alternative:* a
purely random token stream — nearly every program dies on its first
operator and exercises nothing.

**D2. Own random source.** A 64-bit xorshift seeded from the seed and
index; no dependency; deterministic across platforms because the
generator uses only integer arithmetic.

**D3. Program shape.** A program is a list of statements, each a line;
the `graphics` profile prefixes a page size and ends with `showpage`;
the `core` profile prints its final stack with `pstack` so output
carries the computation. Statements are the unit for shrinking and
for the metamorphic wrappers.

**D4. Runner and properties.** `psgen check` runs a program through
`Interp` with capture streams, the graphics backend with a collecting
sink, and the budget; then the variants: rerun (determinism);
`save`/`restore` around the statements chosen as the "block" (all but
the final print), comparing the output produced after the block;
`gsave`/`grestore` around one paint statement, comparing IR; `N M
translate` prepended, comparing IR with every coordinate shifted
(stroke matrices and image matrices translate their offsets; text
matrices too); two `def` statements with disjoint names swapped,
comparing output; `remelt::distill` to memory and the structural check
from `pdf-out`'s test support promoted to a small `pdf_out::check`
module behind a `check` feature (or duplicated into psgen if the
feature is undesirable — decide at implementation, record). A failure
is a `Failure { property, seed, index, detail }`.

**D5. Shrinker.** Statement-level delta debugging (ddmin over the
statement list, then single-statement removal until fixed point) under
a predicate: a named property re-evaluated in process, or an external
command given the file path and returning non-zero for "still fails"
(for oracle failures). Removing statements can make a program ill
typed; that is fine, since the predicate decides. The result keeps the
header and adds `% psgen: shrunk from seed S index I property P`.

**D6. Budget in `ps-vm`.** `Limits { steps: Option<u64> }`, counted in
the execution loop per object executed; exceeding raises
`VmError::LimitCheck` on the current object through the normal error
path (so `stopped` can catch it — the generator's programs must not
loop in a handler; the budget is checked again after the handler, so a
handler that loops is stopped too, recorded). `psgen` sets it from the
profile (default 200 000).

**D7. Seed files and the xtask.** `corpus/generated/core.seeds` and
`graphics.seeds` hold `seed count` lines with `#` comments;
`cargo xtask fuzz-round [--profile p] [--oracle name]` generates to
`target/psgen/<profile>/<seed>/`, checks, and with `--oracle` (private
tier) invokes `difftest oracle` over the directory; exit non-zero on
any failure; summary per profile. Initial seed files: ten seeds of one
hundred programs each per profile, chosen so the run stays under a
minute in debug builds.

**D8. First fuzz round is part of the change.** The implementer runs
both profiles with more seeds than the files list (thousands of
programs), shrinks every failure, and records the minimal programs and
their classification in the notes; any interpreter bug found is
reported, not fixed here, unless trivial and clearly in scope. In the
private tier, the oracle over the generated output adds the reference
comparison; its failures are triaged the same way.

## Risks / Trade-offs

- [Properties too weak to find anything] → the first round tells; the
  translation and save/restore relations are strong for the graphics
  and VM layers respectively.
- [Generated programs are unreadable when they fail] → shrinking is in
  v0 for this reason.
- [Budget changes error semantics] → opt-in, unset by default, and
  raised as an ordinary `limitcheck`.
