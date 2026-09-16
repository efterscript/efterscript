## ADDED Requirements

### Requirement: Random numbers

`rand` SHALL return an integer in 0 to 2^31 − 1 from a pseudo-random
generator whose state `srand` sets from any integer and `rrand`
returns, so that `srand` with a value returned by `rrand` reproduces
the sequence that followed, per their entries in PLRM3 §8.2. The
initial state SHALL be fixed, so a program that never calls `srand`
produces the same sequence in every run.

#### Scenario: A seed reproduces the sequence

- **WHEN** `7 srand rand rand rrand 7 srand rand rand rrand` is executed
- **THEN** the second pair of values equals the first pair and both `rrand` values are equal, and every `rand` value lies in 0 to 2^31 − 1

### Requirement: Clocks

`usertime` SHALL return an integer that never decreases across a
job and increases with execution; `realtime` SHALL return an integer
from the embedder's clock when one is installed and otherwise the
`usertime` value; both SHALL wrap to the most negative integer past
the largest, per their entries.

#### Scenario: Time moves forward

- **WHEN** `usertime 1000 { pop } repeat usertime exch sub 0 ge realtime type` is executed
- **THEN** the stack holds `true` and `/integertype`

## MODIFIED Requirements

### Requirement: languagelevel

`languagelevel` SHALL return 3.

#### Scenario: Level claim

- **GIVEN** `languagelevel =`
- **THEN** the output is `3`
