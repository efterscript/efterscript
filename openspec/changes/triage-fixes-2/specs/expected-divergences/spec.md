# expected-divergences

## ADDED Requirements

### Requirement: procedure-nesting-limit

Procedure bodies nested deeper than the scanner's limit raise
`limitcheck` while scanning, where other interpreters scan arbitrarily
deep nesting; the reference makes such limits implementation-dependent,
and the limit bounds memory during scanning of untrusted input.

#### Scenario: Declared

- **GIVEN** the corpus file nesting procedures beyond the limit
- **THEN** it carries `% divergence: procedure-nesting-limit`
