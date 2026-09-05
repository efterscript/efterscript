# oracle-testing Specification

## Purpose
Differential testing against a reference converter under strict
isolation: the profile contract, the comparison, the verdicts, and the
lint that keeps every oracle's identity out of the public repository.

## Requirements

### Requirement: Isolation of the reference converter

The public repository SHALL NOT contain the name, package, invocation,
or output of any reference converter or third-party checker. The
harness SHALL learn the converter only from a profile file located
through `EFTERSCRIPT_ORACLE_PROFILE`, or by name under the vault's
`oracles/` directory when `EFTERSCRIPT_HELLBOX` is set. Without a
profile the oracle subcommand SHALL print that it is skipping and exit
successfully. All converter outputs SHALL be written under the build
directory only.

#### Scenario: No profile

- **WHEN** `difftest oracle` runs with neither variable set
- **THEN** it prints one line saying the oracle tier is skipped and
  exits with status 0

#### Scenario: Profile by name from the vault

- **GIVEN** `EFTERSCRIPT_HELLBOX` set and `difftest oracle --profile
  default`
- **THEN** the profile `oracles/default.toml` from the vault is used

### Requirement: Comparison

For each corpus file the harness SHALL produce EfterScript's PDF and
the converter's PDF, render both through the profile's rasteriser to
raw PNM at the profile's resolution, and compare page count, media box
(within 0.5 units), and pixels: a pixel differs when any channel differs
by more than the profile's threshold, and a page fails when the
differing fraction exceeds the profile's limit. When the profile
provides a text extractor, extracted text SHALL be compared after
whitespace normalisation. The harness SHALL also run the file through
the reference interpreter and compare standard output with
EfterScript's after normalising real-number formatting and trailing
whitespace. Each file SHALL be given the profile's timeout.

#### Scenario: Identical pages pass

- **GIVEN** a corpus file whose two renderings differ in no pixel beyond
  the threshold
- **THEN** the verdict is pass

#### Scenario: Missing page fails

- **GIVEN** a corpus file for which the converter produces two pages and
  EfterScript one
- **THEN** the verdict is fail with the page counts in the report

#### Scenario: Output channel compared

- **GIVEN** a corpus file with `% expect-output:` lines
- **THEN** the report shows whether the reference interpreter's output
  equals EfterScript's after normalisation, independently of the raster
  verdict

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

### Requirement: Denylist lint

A lint SHALL read `oracles/denylist.txt` from the vault when
`EFTERSCRIPT_HELLBOX` names one, scan every text file tracked in the
public repository for each listed string case-insensitively, and fail
naming file and line on any hit; without the vault it SHALL skip with a
message.

#### Scenario: A leaked name

- **GIVEN** a scratch copy of the repository with a listed string in a
  comment
- **WHEN** the lint runs against it with the vault present
- **THEN** it fails naming the file and line
