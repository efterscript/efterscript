# oracle-testing

## MODIFIED Requirements

### Requirement: Verdicts and divergence headers

Verdicts SHALL be `pass`, `fail`, `expected-divergence`, and `skipped`.
A corpus file carrying `% divergence: <slug>` SHALL be reported as
expected-divergence when it mismatches and as `divergence-closed` when
it matches; the slug SHALL name a requirement in the
`expected-divergences` specification, and the harness SHALL fail the
run when a slug has no such requirement. A corpus file carrying
`% oracle: skip <reason>` SHALL be reported as skipped with the reason
and SHALL not be converted or compared. The summary SHALL count each
verdict and exit non-zero only on `fail`.

#### Scenario: Declared divergence

- **GIVEN** the font-substitution corpus file carrying `% divergence:
  font-substitution` and a converter that raises an error for the
  unknown font
- **THEN** the verdict is expected-divergence and the run's status is
  unaffected

#### Scenario: Unknown slug

- **GIVEN** a corpus file with `% divergence: no-such-record`
- **THEN** the run fails before comparing, naming the file and slug

#### Scenario: Skipped scenario

- **GIVEN** a corpus file with `% oracle: skip build without a graphics
  backend`
- **THEN** the verdict is skipped with that reason, no converter command
  runs for it, and the run's status is unaffected
