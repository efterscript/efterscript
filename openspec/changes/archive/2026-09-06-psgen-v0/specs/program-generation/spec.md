# program-generation

## Purpose

The property-based generator: what programs it produces, how a run is
reproduced from a seed, which properties every program is checked
against, how failures shrink, and how a generated case becomes corpus.

## ADDED Requirements

### Requirement: Deterministic generation from seeds

`psgen gen --profile <name> --seed <n> --count <k> --out <dir>` SHALL
write `k` programs whose text depends only on the profile, the seed,
and the index; the same arguments SHALL produce byte-identical files on
any machine. Each program SHALL begin with a `% psgen:` header naming
the profile, seed, and index, and SHALL be well formed for the
scanner. Profiles SHALL select operator groups and size bounds, and the
`core` and `graphics` profiles SHALL exist.

#### Scenario: Reproducible

- **WHEN** the same profile, seed, and count are generated twice
- **THEN** the two directories are byte-identical

#### Scenario: Well typed by construction

- **GIVEN** the `core` profile with the ill-typed share set to zero and
  100 programs
- **THEN** every program ends without an error under the interpreter

### Requirement: Oracle-free properties

`psgen check` SHALL run each program in process and report a failure
when: the interpreter panics; the execution budget is exceeded; two
runs differ in outcome, output, or IR dump; wrapping the program body
in `save`/`restore` changes output after the block; wrapping a paint in
`gsave`/`grestore` changes the IR; translating the CTM by a vector
changes any IR coordinate other than by that vector; reordering two
independent definitions changes output; or the distilled PDF fails the
writer's structural check. The report SHALL name the property, the
seed, and the index.

#### Scenario: Determinism property

- **GIVEN** a program under the `core` profile
- **WHEN** it is checked
- **THEN** its two runs are compared and a difference is reported as a
  determinism failure with the seed and index

#### Scenario: Translation property

- **GIVEN** a program under the `graphics` profile that fills a
  rectangle
- **WHEN** it is checked with the translation relation
- **THEN** the IR of the translated variant equals the original's with
  every coordinate shifted by the vector, else a failure names the
  first differing operation

### Requirement: Shrinking

`psgen shrink <file> --property <name>` (or `--predicate <command>`)
SHALL remove statements while the failure persists until no single
statement can be removed, and SHALL write the minimal program with a
header recording the original seed, index, and property. The minimal
program SHALL still fail the same property.

#### Scenario: A planted failure shrinks

- **GIVEN** a generated program to which a statement causing a known
  property failure was appended among fifty others
- **WHEN** it is shrunk
- **THEN** the result holds that statement and at most the statements
  it depends on, and still fails

### Requirement: Seed files and promotion

`corpus/generated/<profile>.seeds` SHALL list seed and count pairs;
`cargo xtask fuzz-round` SHALL generate and check every listed pair and
fail on any property failure, writing programs only under the build
directory. A minimised failure SHALL enter the corpus only by hand as a
`corpus/unit/` file with expectation headers.

#### Scenario: Seed files run in CI

- **WHEN** `cargo xtask fuzz-round` runs
- **THEN** every seed file's pairs are generated and checked and the
  summary counts programs and failures per profile
