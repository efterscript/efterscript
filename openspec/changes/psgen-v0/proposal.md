# Change: psgen v0 — property-based program generation

## Why

The corpus is 145 hand-written files and the oracle tier now agrees
with all of them. Hand-written cases find the bugs their authors
imagine; the compatibility swamp is made of the ones nobody imagined.
A generator that produces unlimited well-formed PostScript programs
from a grammar, runs them against oracle-free properties in the public
tier and against the reference converter in the private tier, and
shrinks every failure to a small reproduction, turns the harness from a
regression net into a bug finder. Every generated case is the project's
own work, so the corpus grows without any provenance question. Two
shapes are fixed here that later generators build on: the seed-file
contract (committed seeds, never bulk output) and the property
interface a program is checked against.

## What Changes

- **A generator** (`tools/psgen`): a type-aware grammar over a curated
  operator set (stack, arithmetic, relational and boolean, dictionary
  and array construction and access, control flow with bounded loops,
  procedures and `bind`, `save`/`restore`, path construction, colour,
  line parameters, painting, clipping, text in resident fonts) that
  tracks a static operand-stack model so programs are well typed, with
  a configurable share of deliberately ill-typed operations to exercise
  the error machinery; a deterministic random source of the project's
  own; profiles selecting operator groups and sizes.
- **Oracle-free properties** checked in process: no panic; the
  execution budget holds; the outcome, output, and IR dump are
  identical across two runs; four metamorphic relations — wrapping a
  block in `save`/`restore` leaves later output and IR unchanged,
  wrapping a paint in `gsave`/`grestore` leaves the IR unchanged,
  translating the CTM shifts every IR coordinate by the translation,
  and reordering independent definitions leaves output unchanged; and
  the distilled PDF passes the writer's own structural checks.
- **Shrinking**: a failing program is reduced by statement-level
  delta debugging under the same failure predicate until minimal, and
  written with a `% psgen:` header recording the seed, profile, and
  property that failed.
- **Seeds and promotion**: `corpus/generated/<profile>.seeds` files
  list seeds and counts to run in CI; bulk output stays under the build
  directory; a minimised failure the user accepts is promoted by hand
  to `corpus/unit/` as an ordinary scenario.
- **Oracle use**: `psgen gen` writes programs to a directory that
  `difftest oracle` accepts as paths, so the private tier runs
  generated programs against the reference converter without new
  harness code; `psgen shrink` accepts an external predicate command
  for that case.
- **Execution budget** (`ps-vm`): an optional limit on executed
  objects in the interpreter's limits, raising `limitcheck` when
  exceeded, so no generated program can hang a run; unlimited by
  default.
- Out of scope, with triggers: fonts embedded in generated programs
  (when the synthesised-font builders are exposed to the generator),
  images (a later profile), composite text, coverage-guided generation
  (when a coverage signal exists), a grammar for malformed syntax (the
  scanner fuzz target already covers it).

## Capabilities

### New Capabilities
- `program-generation`: the generator's contract — determinism, the
  seed files, the properties, shrinking, and the promotion rule.

### Modified Capabilities
- `interpreter-core`: ADDED requirement for the execution budget.

## Impact

- Code: `tools/psgen` (grammar, model, random source, runner,
  properties, shrinker, CLI), `crates/ps-vm` (the budget, additive),
  `corpus/generated/` (seed files), `corpus/README.md`, `xtask`
  (`fuzz-round` running the seed files, and in the private tier the
  oracle over the output). No new dependencies.
- Depends on `oracle-testing` (archived).
