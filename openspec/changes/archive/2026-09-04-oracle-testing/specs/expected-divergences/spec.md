# expected-divergences

## Purpose

The registry of reviewed decisions to behave differently from the
behaviour observed in other interpreters or specified by the reference,
one requirement per divergence, each tied to the corpus files that
declare it.

## ADDED Requirements

### Requirement: Registry discipline

Every expected divergence SHALL be a requirement in this specification
whose name is the slug corpus files declare with `% divergence:`, stating
the behaviour chosen, the behaviour it departs from without naming any
implementation, the reason, and the configuration that restores the
reference behaviour when one exists. A divergence SHALL be added only
through a change proposal.

#### Scenario: Slug resolution

- **GIVEN** a corpus file declaring a slug
- **THEN** a requirement with that name exists here and the oracle
  harness accepts the file

### Requirement: font-substitution

`findfont` of a name not defined in the job resolves to a resident face
instead of raising `invalidfont` as the reference specifies; chosen so
jobs that reference fonts they do not embed keep running; restored by
disabling substitution in the interpreter configuration.

#### Scenario: Declared on the substitution corpus files

- **GIVEN** the corpus files exercising alias and heuristic substitution
- **THEN** each carries `% divergence: font-substitution`
